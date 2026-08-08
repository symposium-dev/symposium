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
//!   not at construction. A hook event whose predicates never reference a
//!   dependency starts no PM at all, which is what keeps `PreToolUse` cheap.
//! - **Degrade, never abort.** A PM that fails to spawn, fails its handshake,
//!   dies mid-request, or times out contributes *nothing*, logged as a warning.
//!   One broken PM must never break a sync or a hook, exactly as an in-process
//!   plugin that fails to load is dropped rather than surfaced.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, bail};
use symposium_install::UpdateLevel;
use symposium_sdk::pm::protocol::{self, *};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

use super::update_of;
use super::{FetchedPackage, PackageId, PackageManager, PluginInfo, PluginOffer, WorkspaceInfo};

/// How long a single request may take before the PM is considered dead.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// How to start a package-manager binary.
pub struct RemotePmCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    /// Context handed over in `initialize`.
    pub workspace: Option<PathBuf>,
    pub cache_dir: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
}

impl RemotePmCommand {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            workspace: None,
            cache_dir: None,
            env: BTreeMap::new(),
        }
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

    /// Forward an environment variable to the child. Passed explicitly rather
    /// than inherited so the contract is visible: this is how the test
    /// harness's fake cargo reaches the cargo PM.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }
}

/// A live connection to a PM process.
struct Connection {
    /// Kept so the child is killed when the connection drops.
    _child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: AtomicU64,
}

/// A package manager running in another process.
pub struct RemotePm {
    /// The name Symposium knows this PM by. Confirmed against the name the PM
    /// reports at `initialize`; a mismatch is a hard failure, since the name is
    /// the `pm` component of every id and routing depends on it.
    name: String,
    command: RemotePmCommand,
    /// `None` until first use; `Some(Err)` once the PM has failed, so a broken
    /// PM is not respawned on every call.
    state: Mutex<Option<Result<Connection, String>>>,
    /// What the PM reported at `initialize`. Recorded for diagnostics: the
    /// server harness already answers empty for operations a PM does not
    /// implement, so nothing needs to gate on this.
    capabilities: Mutex<Vec<String>>,
}

impl RemotePm {
    pub fn new(name: impl Into<String>, command: RemotePmCommand) -> Self {
        Self {
            name: name.into(),
            command,
            state: Mutex::new(None),
            capabilities: Mutex::new(Vec::new()),
        }
    }

    /// Send one request, spawning and handshaking on first use.
    ///
    /// Returns `Err` for every failure mode: spawn, handshake, transport,
    /// timeout, or a JSON-RPC error response. Callers decide how loudly to
    /// complain; the plugin-loading paths degrade to empty.
    async fn call<R: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<R> {
        let mut guard = self.state.lock().await;
        if guard.is_none() {
            *guard = Some(self.connect().await.map_err(|e| format!("{e:#}")));
        }
        let conn = match guard.as_mut().expect("just set") {
            Ok(conn) => conn,
            Err(e) => bail!("package manager `{}` is unavailable: {e}", self.name),
        };

        let result = request(conn, method, params).await;
        if result.is_err() {
            // A transport failure means the process is no longer trustworthy;
            // mark it dead so later calls fail fast instead of hanging again.
            *guard = Some(Err(format!("`{method}` failed")));
        }
        let value = result?;
        serde_json::from_value(value)
            .with_context(|| format!("decoding `{method}` result from `{}`", self.name))
    }

    /// Spawn the binary and complete the `initialize` handshake.
    async fn connect(&self) -> Result<Connection> {
        let mut cmd = Command::new(&self.command.program);
        cmd.args(&self.command.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Inherit stderr so a PM's diagnostics reach the user's terminal /
            // log rather than filling an unread pipe and blocking the child.
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        for (k, v) in &self.command.env {
            cmd.env(k, v);
        }

        let mut child = cmd
            .spawn()
            .with_context(|| format!("spawning `{}`", self.command.program.display()))?;
        let stdin = child.stdin.take().context("child stdin was not piped")?;
        let stdout = BufReader::new(child.stdout.take().context("child stdout was not piped")?);
        let mut conn = Connection {
            _child: child,
            stdin,
            stdout,
            next_id: AtomicU64::new(1),
        };

        let params = serde_json::to_value(InitializeParams {
            protocol_version: PROTOCOL_VERSION,
            workspace: self.command.workspace.clone(),
            cache_dir: self.command.cache_dir.clone(),
            env: self.command.env.clone(),
        })?;
        let value = request(&mut conn, protocol::INITIALIZE, params)
            .await
            .context("initialize handshake failed")?;
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
        tracing::debug!(
            pm = %self.name,
            capabilities = ?init.capabilities,
            "package manager initialized"
        );
        *self.capabilities.lock().await = init.capabilities;
        Ok(conn)
    }

    /// Offers, or an empty list with a warning: the best-effort contract every
    /// plugin-loading path holds.
    async fn offers(&self, method: &str, params: serde_json::Value) -> Vec<PluginOffer> {
        match self.call::<OffersResult>(method, params).await {
            Ok(r) => r.offers,
            Err(e) => {
                tracing::warn!(pm = %self.name, method, error = %format!("{e:#}"), "package manager call failed");
                Vec::new()
            }
        }
    }
}

/// Write one request line and read the matching response.
///
/// Requests are issued under the connection lock, so responses arrive in order
/// and a mismatched id means the PM is misbehaving.
async fn request(
    conn: &mut Connection,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value> {
    let id = conn.next_id.fetch_add(1, Ordering::Relaxed);
    let request = Request::new(id, method, Some(params));
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

    let buf = tokio::time::timeout(REQUEST_TIMEOUT, exchange)
        .await
        .with_context(|| format!("`{method}` timed out after {REQUEST_TIMEOUT:?}"))??;

    let response: Response =
        serde_json::from_str(buf.trim()).with_context(|| format!("malformed response: {buf:?}"))?;
    if response.id != id {
        bail!("response id {} does not match request id {id}", response.id);
    }
    match (response.result, response.error) {
        (Some(v), None) => Ok(v),
        (None, Some(e)) => bail!("{} (code {})", e.message, e.code),
        _ => bail!("response set neither or both of `result` and `error`"),
    }
}

#[async_trait::async_trait]
impl PackageManager for RemotePm {
    fn name(&self) -> &str {
        &self.name
    }

    async fn active_plugins(&self, deps: &[PackageId]) -> Vec<PluginOffer> {
        let params = match serde_json::to_value(ActivePluginsParams {
            deps: deps.to_vec(),
        }) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(pm = %self.name, error = %e, "encoding active_plugins params");
                return Vec::new();
            }
        };
        self.offers(protocol::ACTIVE_PLUGINS, params).await
    }

    async fn load_plugin(&self, id: &PackageId) -> Vec<PluginOffer> {
        let params = match serde_json::to_value(LoadPluginParams { id: id.clone() }) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(pm = %self.name, error = %e, "encoding load_plugin params");
                return Vec::new();
            }
        };
        self.offers(protocol::LOAD_PLUGIN, params).await
    }

    async fn workspace_info(&self) -> Result<Option<WorkspaceInfo>> {
        Ok(self
            .call::<WorkspaceInfoResult>(protocol::WORKSPACE_INFO, serde_json::Value::Null)
            .await?
            .workspace)
    }

    async fn list_deps(&self) -> Result<Vec<PackageId>> {
        Ok(self
            .call::<ListDepsResult>(protocol::LIST_DEPS, serde_json::Value::Null)
            .await?
            .deps)
    }

    async fn search(&self, query: &str) -> Result<Vec<PluginInfo>> {
        Ok(self
            .call::<SearchResult>(
                protocol::SEARCH,
                serde_json::to_value(SearchParams {
                    query: query.to_string(),
                })?,
            )
            .await?
            .plugins)
    }

    async fn fetch(&self, id: &PackageId, update: UpdateLevel) -> Result<FetchedPackage> {
        let result: protocol::FetchResult = self
            .call(
                protocol::FETCH,
                serde_json::to_value(FetchParams {
                    id: id.clone(),
                    update: update_of(update),
                })?,
            )
            .await?;
        Ok(FetchedPackage {
            id: result.id,
            root: result.root,
        })
    }

    async fn refresh(&self, update: UpdateLevel, force: bool) -> Result<bool> {
        Ok(self
            .call::<RefreshResult>(
                protocol::REFRESH,
                serde_json::to_value(RefreshParams {
                    update: update_of(update),
                    force,
                })?,
            )
            .await?
            .refreshed)
    }
}
