//! The server side of the package-manager protocol: implement
//! [`PackageManagerServer`], call [`run`], and you have a PM binary.
//!
//! ```no_run
//! use symposium_sdk::pm::server::{PackageManagerServer, run};
//! use symposium_sdk::pm::{PluginOffer, protocol::InitializeParams};
//!
//! struct MyPm;
//!
//! #[async_trait::async_trait]
//! impl PackageManagerServer for MyPm {
//!     fn name(&self) -> &str { "mypm" }
//!     async fn initialize(&mut self, _params: &InitializeParams) -> anyhow::Result<()> { Ok(()) }
//!     async fn load_plugin(&self, _id: &symposium_sdk::pm::PackageId) -> Vec<PluginOffer> {
//!         Vec::new()
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     run(MyPm).await
//! }
//! ```
//!
//! Every method except `name` and `initialize` has a default that answers
//! "nothing", so a PM implements only what its ecosystem supports. Report the
//! rest via [`capabilities`](PackageManagerServer::capabilities) so Symposium
//! can skip calls it knows will be empty.

use anyhow::Result;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::protocol::{self, *};
use super::{PackageId, PluginInfo, PluginOffer};

/// What a package-manager binary implements.
#[async_trait::async_trait]
pub trait PackageManagerServer: Send + Sync {
    /// The name this PM owns: the `pm` component of every id it mints.
    fn name(&self) -> &str;

    /// Take the per-invocation context. Called once, before anything else.
    async fn initialize(&mut self, params: &InitializeParams) -> Result<()>;

    /// Optional operations this PM implements; see [`protocol::capability`].
    fn capabilities(&self) -> Vec<String> {
        Vec::new()
    }

    /// The plugins this PM activates for the workspace's dependency set.
    async fn active_plugins(&self, _deps: &[PackageId]) -> Vec<PluginOffer> {
        Vec::new()
    }

    /// The plugin(s) a specific id maps to.
    async fn load_plugin(&self, id: &PackageId) -> Vec<PluginOffer>;

    /// The workspace's dependencies in this PM's ecosystem.
    async fn list_deps(&self) -> Result<Vec<PackageId>> {
        Ok(Vec::new())
    }

    /// Packages matching a partial query.
    async fn search(&self, _query: &str) -> Result<Vec<PluginInfo>> {
        Ok(Vec::new())
    }

    /// Acquire a package's content, canonicalizing the id's version.
    async fn fetch(&self, id: &PackageId, _update: Update) -> Result<FetchResult> {
        anyhow::bail!("{} cannot fetch `{id}`", self.name())
    }

    /// Pull this PM's backing source. `false` when there is nothing remote.
    async fn refresh(&self, _update: Update, _force: bool) -> Result<bool> {
        Ok(false)
    }
}

/// Serve the protocol on stdin/stdout until stdin closes.
///
/// Requests are handled one at a time in arrival order. Symposium may have
/// several in flight; serializing here is the simplest correct answer, and the
/// per-request work is I/O against a local cache.
pub async fn run<P: PackageManagerServer + 'static>(mut pm: P) -> Result<()> {
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    let mut stdout = tokio::io::stdout();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let request: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                // No id to answer against, so this can only be logged. Symposium
                // captures our stderr at debug level.
                eprintln!("malformed request: {e}");
                continue;
            }
        };
        let response = dispatch(&mut pm, request).await;
        let mut buf = serde_json::to_vec(&response)?;
        buf.push(b'\n');
        stdout.write_all(&buf).await?;
        stdout.flush().await?;
    }
    Ok(())
}

async fn dispatch<P: PackageManagerServer>(pm: &mut P, request: Request) -> Response {
    let id = request.id;
    let params = request.params.unwrap_or(serde_json::Value::Null);

    macro_rules! parse {
        ($t:ty) => {
            match decode::<$t>(params) {
                Ok(v) => v,
                Err(e) => return Response::err(id, error_code::INVALID_PARAMS, e.to_string()),
            }
        };
    }
    macro_rules! reply {
        ($v:expr) => {
            match serde_json::to_value($v) {
                Ok(v) => Response::ok(id, v),
                Err(e) => Response::err(id, error_code::INVALID_INPUT, e.to_string()),
            }
        };
    }

    match request.method.as_str() {
        protocol::INITIALIZE => {
            let p: InitializeParams = parse!(InitializeParams);
            if p.protocol_version != PROTOCOL_VERSION {
                return Response::err(
                    id,
                    error_code::INVALID_INPUT,
                    format!(
                        "unsupported protocol version {} (this PM speaks {PROTOCOL_VERSION})",
                        p.protocol_version
                    ),
                );
            }
            match pm.initialize(&p).await {
                Ok(()) => reply!(InitializeResult {
                    protocol_version: PROTOCOL_VERSION,
                    name: pm.name().to_string(),
                    capabilities: pm.capabilities(),
                }),
                Err(e) => Response::err(id, error_code::INVALID_INPUT, format!("{e:#}")),
            }
        }
        protocol::ACTIVE_PLUGINS => {
            let p: ActivePluginsParams = parse!(ActivePluginsParams);
            reply!(OffersResult {
                offers: pm.active_plugins(&p.deps).await,
            })
        }
        protocol::LOAD_PLUGIN => {
            let p: LoadPluginParams = parse!(LoadPluginParams);
            reply!(OffersResult {
                offers: pm.load_plugin(&p.id).await,
            })
        }
        protocol::LIST_DEPS => match pm.list_deps().await {
            Ok(deps) => reply!(ListDepsResult { deps }),
            Err(e) => Response::err(id, error_code::INVALID_INPUT, format!("{e:#}")),
        },
        protocol::SEARCH => {
            let p: SearchParams = parse!(SearchParams);
            match pm.search(&p.query).await {
                Ok(plugins) => reply!(SearchResult { plugins }),
                Err(e) => Response::err(id, error_code::NETWORK, format!("{e:#}")),
            }
        }
        protocol::FETCH => {
            let p: FetchParams = parse!(FetchParams);
            match pm.fetch(&p.id, p.update).await {
                Ok(r) => reply!(r),
                Err(e) => Response::err(id, error_code::NOT_FOUND, format!("{e:#}")),
            }
        }
        protocol::REFRESH => {
            let p: RefreshParams = parse!(RefreshParams);
            match pm.refresh(p.update, p.force).await {
                Ok(refreshed) => reply!(RefreshResult { refreshed }),
                Err(e) => Response::err(id, error_code::NETWORK, format!("{e:#}")),
            }
        }
        other => Response::err(
            id,
            error_code::METHOD_NOT_FOUND,
            format!("unknown method `{other}`"),
        ),
    }
}

/// Deserialize params, treating a missing `params` as an empty object so
/// no-argument methods can be called without one.
fn decode<T: DeserializeOwned>(params: serde_json::Value) -> Result<T, serde_json::Error> {
    if params.is_null() {
        serde_json::from_value(serde_json::Value::Object(Default::default()))
    } else {
        serde_json::from_value(params)
    }
}
