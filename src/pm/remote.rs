//! The client side of the package-manager protocol: a [`PackageManager`]
//! backed by a subprocess.
//!
//! [`RemotePm`] spawns a PM binary, speaks newline-delimited JSON-RPC to it
//! over stdio, and implements the same trait an in-process PM does, so
//! everything above `pm/` is unaware of the boundary.
//!
//! Two properties matter more than the plumbing:
//!
//! - **Lazy spawn.** The process starts on the first operation that needs it,
//!   not at construction. A hook event whose recorded inputs are unchanged
//!   since the last sync asks no PM where the workspace is or what it depends
//!   on, so it starts none unless a package has to be loaded, which is what
//!   keeps `PreToolUse` cheap.
//! - **Degrade, never abort.** A PM that fails to spawn, fails its handshake,
//!   dies mid-request, or times out answers with an error, which the caller
//!   records as that PM failing: one broken PM must never break a sync or a
//!   hook.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use symposium_install::UpdateLevel;
use symposium_sdk::pm::protocol::{self, *};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

use super::{
    FetchedPackage, PackageId, PackageManager, PluginInfo, PluginListing, ProjectInfo,
    UnvalidatedPlugin,
};

/// How long a single request may take before the PM is considered dead, unless
/// the command sets its own. A hook makes several requests, but a PM that
/// times out is dead for the rest of the invocation, so one hung PM costs one
/// timeout, not one per request.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// How to start a package-manager binary.
pub struct RemotePmCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    /// Context handed over in `initialize`.
    pub workspace: Option<PathBuf>,
    pub cache_dir: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    /// How long one request may take before the PM is considered dead.
    pub timeout: Duration,
}

impl RemotePmCommand {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            workspace: None,
            cache_dir: None,
            env: BTreeMap::new(),
            timeout: DEFAULT_REQUEST_TIMEOUT,
        }
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn workspace(mut self, dir: Option<PathBuf>) -> Self {
        self.workspace = dir;
        self
    }

    pub fn cache_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cache_dir = Some(dir.into());
        self
    }

    /// Set an environment variable for the child, on top of the environment it
    /// inherits from Symposium.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }
}

/// A live connection to a PM process.
///
/// Dropping it closes the PM's stdin, so the PM exits once it finishes the
/// request it is on, and so do any `cargo` processes it started. It is killed
/// only when it stops answering.
struct Connection {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    timeout: Duration,
}

/// A package manager running in another process.
pub struct RemotePm {
    /// The name Symposium knows this PM by. Confirmed against the name the PM
    /// reports at `initialize`; a mismatch is a hard failure, since the name is
    /// the `pm` component of every id and routing depends on it.
    name: String,
    command: RemotePmCommand,
    /// `None` until first use; `Some(Err)` once the PM has failed, so a broken
    /// PM is not respawned on every call. Also `Some(Err)` while a request is
    /// in flight, so a request abandoned midway leaves the PM marked unusable
    /// rather than a connection whose next read is the abandoned answer.
    state: Mutex<Option<Result<Connection, String>>>,
    /// The PM's answers to `canonical_name`, so each name costs one request.
    canonical: std::sync::Mutex<BTreeMap<String, String>>,
}

impl RemotePm {
    pub fn new(name: impl Into<String>, command: RemotePmCommand) -> Self {
        Self {
            name: name.into(),
            command,
            state: Mutex::new(None),
            canonical: std::sync::Mutex::new(BTreeMap::new()),
        }
    }

    /// Send one request, spawning and handshaking on first use. `params` is
    /// left out of the request when there are none.
    ///
    /// Returns `Err` for every failure mode: spawn, handshake, transport,
    /// timeout, or a JSON-RPC error response.
    ///
    /// Only a transport failure marks the PM dead. An error response is the PM
    /// working as intended (a package that does not exist, a search that needs
    /// the network), and the next request may well succeed.
    async fn call<R: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<R> {
        let mut guard = self.state.lock().await;
        let mut conn = match guard.take() {
            Some(Ok(conn)) => conn,
            Some(Err(e)) => {
                *guard = Some(Err(e.clone()));
                bail!("package manager `{}` is unavailable: {e}", self.name);
            }
            None => match self.connect().await {
                Ok(conn) => conn,
                Err(e) => {
                    let e = format!("{e:#}");
                    *guard = Some(Err(e.clone()));
                    bail!("package manager `{}` is unavailable: {e}", self.name);
                }
            },
        };
        *guard = Some(Err(format!("a `{method}` request was abandoned")));

        let reply = match request(&mut conn, method, params).await {
            Ok(reply) => {
                *guard = Some(Ok(conn));
                reply
            }
            Err(e) => {
                // The process is no longer trustworthy: kill it, and mark it
                // dead so later calls fail fast instead of hanging again.
                let _ = conn.child.start_kill();
                *guard = Some(Err(format!("`{method}` failed: {e:#}")));
                return Err(e);
            }
        };
        let value = reply.map_err(|e| {
            anyhow::anyhow!(
                "`{}` answered `{method}` with an error: {} (code {})",
                self.name,
                e.message,
                e.code
            )
        })?;
        serde_json::from_value(value)
            .with_context(|| format!("decoding `{method}` result from `{}`", self.name))
    }

    /// Spawn the binary and complete the `initialize` handshake.
    async fn connect(&self) -> Result<Connection> {
        let mut cmd = Command::new(&self.command.program);
        cmd.args(&self.command.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Logged rather than inherited: on the hook path, anything on our
            // stderr reaches the agent.
            .stderr(Stdio::piped())
            // A connection is dropped between requests, or when a handshake or
            // request fails or is abandoned. A PM stuck mid-answer would
            // otherwise outlive Symposium, once per invocation that spawned it.
            .kill_on_drop(true);
        for (k, v) in &self.command.env {
            cmd.env(k, v);
        }

        let mut child = cmd
            .spawn()
            .with_context(|| format!("spawning `{}`", self.command.program.display()))?;
        let stdin = child.stdin.take().context("child stdin was not piped")?;
        let stdout = BufReader::new(child.stdout.take().context("child stdout was not piped")?);
        if let Some(stderr) = child.stderr.take() {
            let pm = self.name.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!(pm = %pm, "{line}");
                }
            });
        }
        let mut conn = Connection {
            child,
            stdin,
            stdout,
            next_id: 1,
            timeout: self.command.timeout,
        };

        let params = Some(serde_json::to_value(InitializeParams {
            protocol_version: PROTOCOL_VERSION,
            workspace: self.command.workspace.clone(),
            cache_dir: self.command.cache_dir.clone(),
        })?);
        let value = request(&mut conn, protocol::INITIALIZE, params)
            .await
            .context("initialize handshake failed")?
            .map_err(|e| {
                anyhow::anyhow!(
                    "initialize handshake refused: {} (code {})",
                    e.message,
                    e.code
                )
            })?;
        let init: InitializeResult =
            serde_json::from_value(value).context("decoding initialize result")?;

        if init.protocol_version != PROTOCOL_VERSION {
            bail!(
                "`{}` speaks protocol version {}, but this symposium speaks {PROTOCOL_VERSION}",
                self.name,
                init.protocol_version
            );
        }
        if init.name != self.name {
            bail!(
                "`{}` reports its name as `{}`; ids would route to the wrong package manager",
                self.name,
                init.name
            );
        }
        tracing::debug!(pm = %self.name, "package manager initialized");
        Ok(conn)
    }

    /// The PM's plugins. Each one decodes on its own, so a plugin this
    /// Symposium cannot read (a manifest field it does not know, say) is
    /// dropped with a warning rather than taking the rest of the answer with
    /// it.
    ///
    /// A plugin dropped that way is one of the listing's errors, as it would
    /// be had the PM failed to read it itself.
    async fn plugins(&self, method: &str, params: serde_json::Value) -> Result<PluginListing> {
        let answer: PluginListing<serde_json::Value> = self.call(method, Some(params)).await?;
        let mut errors = answer.errors;
        let plugins: Vec<UnvalidatedPlugin> = answer
            .plugins
            .into_iter()
            .filter_map(|plugin| match serde_json::from_value(plugin) {
                Ok(plugin) => Some(plugin),
                Err(e) => {
                    errors.push(format!("a plugin from `{method}` does not decode: {e}"));
                    None
                }
            })
            .collect();
        Ok(PluginListing { plugins, errors })
    }
}

/// What a PM answered: a result, or an error it reported on purpose.
type Reply = std::result::Result<serde_json::Value, ResponseError>;

/// Write one request line and read the matching response.
///
/// The outer `Err` is a transport failure: the PM died, timed out, or wrote
/// something that is not a response to this request. The inner one is an
/// error response, which leaves the connection usable.
///
/// Requests are issued under the connection lock, so responses arrive in order
/// and a mismatched id means the PM is misbehaving.
async fn request(
    conn: &mut Connection,
    method: &str,
    params: Option<serde_json::Value>,
) -> Result<Reply> {
    let id = conn.next_id;
    conn.next_id += 1;
    let request = Request::new(id, method, params);
    let mut line = serde_json::to_vec(&request)?;
    line.push(b'\n');

    let exchange = async {
        conn.stdin.write_all(&line).await?;
        conn.stdin.flush().await?;
        let mut buf = String::new();
        let n = conn.stdout.read_line(&mut buf).await?;
        if n == 0 {
            bail!("package manager closed its output while handling `{method}`");
        }
        Ok::<_, anyhow::Error>(buf)
    };

    let timeout = conn.timeout;
    let buf = tokio::time::timeout(timeout, exchange)
        .await
        .with_context(|| format!("`{method}` timed out after {timeout:?}"))??;

    let response: Response =
        serde_json::from_str(buf.trim()).with_context(|| format!("malformed response: {buf:?}"))?;
    if response.id != id {
        bail!("response id {} does not match request id {id}", response.id);
    }
    match (response.result, response.error) {
        (Some(v), None) => Ok(Ok(v)),
        (None, Some(e)) => Ok(Err(e)),
        _ => bail!("response set neither or both of `result` and `error`"),
    }
}

#[async_trait::async_trait]
impl PackageManager for RemotePm {
    fn name(&self) -> &str {
        &self.name
    }

    async fn canonical_name(&self, name: &str) -> Result<String> {
        if let Some(known) = self.canonical.lock().unwrap().get(name) {
            return Ok(known.clone());
        }
        let params = serde_json::to_value(CanonicalName {
            name: name.to_string(),
        })?;
        let canonical = self
            .call::<CanonicalName>(protocol::CANONICAL_NAME, Some(params))
            .await?
            .name;
        self.canonical
            .lock()
            .unwrap()
            .insert(name.to_string(), canonical.clone());
        Ok(canonical)
    }

    async fn active_plugins(&self, deps: &[PackageId]) -> Result<PluginListing> {
        let params = serde_json::to_value(ActivePluginsParams {
            deps: deps.to_vec(),
        })?;
        self.plugins(protocol::ACTIVE_PLUGINS, params).await
    }

    async fn load_plugin(
        &self,
        id: &PackageId,
        update: UpdateLevel,
    ) -> Result<Vec<UnvalidatedPlugin>> {
        let params = serde_json::to_value(LoadPluginParams {
            id: id.clone(),
            update,
        })?;
        Ok(self.plugins(protocol::LOAD_PLUGIN, params).await?.plugins)
    }

    async fn list_deps(&self) -> Result<Vec<PackageId>> {
        Ok(self
            .call::<ListDepsResult>(protocol::LIST_DEPS, None)
            .await?
            .deps)
    }

    async fn project_info(&self) -> Result<ProjectInfo> {
        Ok(self
            .call::<ProjectInfoResult>(protocol::PROJECT_INFO, None)
            .await?
            .project)
    }

    async fn search(&self, query: &str) -> Result<Vec<PluginInfo>> {
        let params = serde_json::to_value(SearchParams {
            query: query.to_string(),
        })?;
        Ok(self
            .call::<SearchResult>(protocol::SEARCH, Some(params))
            .await?
            .plugins)
    }

    async fn fetch(&self, id: &PackageId, update: UpdateLevel) -> Result<FetchedPackage> {
        let params = serde_json::to_value(FetchParams {
            id: id.clone(),
            update,
        })?;
        self.call(protocol::FETCH, Some(params)).await
    }

    async fn refresh(&self, update: UpdateLevel) -> Result<bool> {
        let params = serde_json::to_value(RefreshParams { update })?;
        Ok(self
            .call::<RefreshResult>(protocol::REFRESH, Some(params))
            .await?
            .refreshed)
    }
}
