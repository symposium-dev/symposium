//! The server side of the package-manager protocol: implement
//! [`PackageManager`], call [`run`] with a function that builds it from the
//! `initialize` handshake, and you have a PM binary.
//!
//! ```no_run
//! use symposium_sdk::pm::server::run;
//! use symposium_sdk::pm::{PackageId, PackageManager, UnvalidatedPlugin, UpdateLevel};
//!
//! struct MyPm;
//!
//! #[async_trait::async_trait]
//! impl PackageManager for MyPm {
//!     fn name(&self) -> &str { "mypm" }
//!     async fn load_plugin(
//!         &self,
//!         _id: &PackageId,
//!         _update: UpdateLevel,
//!     ) -> anyhow::Result<Vec<UnvalidatedPlugin>> {
//!         Ok(Vec::new())
//!     }
//! }
//!
//! #[tokio::main(flavor = "current_thread")]
//! async fn main() -> anyhow::Result<()> {
//!     run(|_params| async { Ok(MyPm) }).await
//! }
//! ```
//!
//! Every method except `name` and `load_plugin` has a default that answers
//! "nothing", so a PM implements only what its ecosystem supports.

use std::future::Future;

use anyhow::Result;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::PackageManager;
use super::protocol::{self, *};

/// Serve the protocol on stdin/stdout until stdin closes.
///
/// The PM is built by `init` from the `initialize` handshake, so it never
/// exists without the context it answers for; a request before `initialize`
/// is answered with an error. Requests are handled one at a time in arrival
/// order, which is how Symposium sends them.
pub async fn run<P, F, Fut>(init: F) -> Result<()>
where
    P: PackageManager,
    F: FnOnce(InitializeParams) -> Fut,
    Fut: Future<Output = Result<P>>,
{
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    let mut stdout = tokio::io::stdout();
    let mut init = Some(init);
    let mut pm: Option<P> = None;

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let request: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                // No id to answer against, so this can only be logged. Symposium
                // logs our stderr at debug level.
                eprintln!("malformed request: {e}");
                continue;
            }
        };
        let response = if request.method == protocol::INITIALIZE {
            match init.take() {
                Some(init) => initialize(init, request, &mut pm).await,
                None => Response::err(request.id, error_code::INVALID_INPUT, "already initialized"),
            }
        } else {
            match &pm {
                Some(pm) => dispatch(pm, request).await,
                None => Response::err(
                    request.id,
                    error_code::INVALID_INPUT,
                    "`initialize` must be the first request",
                ),
            }
        };
        let mut buf = serde_json::to_vec(&response)?;
        buf.push(b'\n');
        stdout.write_all(&buf).await?;
        stdout.flush().await?;
    }
    Ok(())
}

/// Check the protocol version, build the PM, and answer with its name.
async fn initialize<P, F, Fut>(init: F, request: Request, pm: &mut Option<P>) -> Response
where
    P: PackageManager,
    F: FnOnce(InitializeParams) -> Fut,
    Fut: Future<Output = Result<P>>,
{
    let id = request.id;
    let params: InitializeParams = match decode(request.params) {
        Ok(p) => p,
        Err(e) => return Response::err(id, error_code::INVALID_PARAMS, e.to_string()),
    };
    if params.protocol_version != PROTOCOL_VERSION {
        return Response::err(
            id,
            error_code::INVALID_INPUT,
            format!(
                "unsupported protocol version {} (this PM speaks {PROTOCOL_VERSION})",
                params.protocol_version
            ),
        );
    }
    match init(params).await {
        Ok(built) => {
            let result = InitializeResult {
                protocol_version: PROTOCOL_VERSION,
                name: built.name().to_string(),
            };
            *pm = Some(built);
            reply(id, result)
        }
        Err(e) => failed(id, e),
    }
}

fn failed(id: u64, e: anyhow::Error) -> Response {
    Response::err(id, error_code::INTERNAL_ERROR, format!("{e:#}"))
}

fn reply(id: u64, value: impl serde::Serialize) -> Response {
    match serde_json::to_value(value) {
        Ok(v) => Response::ok(id, v),
        Err(e) => Response::err(id, error_code::INTERNAL_ERROR, e.to_string()),
    }
}

async fn dispatch<P: PackageManager>(pm: &P, request: Request) -> Response {
    let id = request.id;
    let params = request.params;

    macro_rules! parse {
        ($t:ty) => {
            match decode::<$t>(params) {
                Ok(v) => v,
                Err(e) => return Response::err(id, error_code::INVALID_PARAMS, e.to_string()),
            }
        };
    }
    macro_rules! answer {
        ($result:expr, |$v:ident| $wrap:expr) => {
            match $result {
                Ok($v) => reply(id, $wrap),
                Err(e) => failed(id, e),
            }
        };
    }

    match request.method.as_str() {
        protocol::ACTIVE_PLUGINS => {
            let p: ActivePluginsParams = parse!(ActivePluginsParams);
            answer!(pm.active_plugins(&p.deps).await, |listing| listing)
        }
        protocol::LOAD_PLUGIN => {
            let p: LoadPluginParams = parse!(LoadPluginParams);
            answer!(pm.load_plugin(&p.id, p.update).await, |plugins| {
                PluginsResult { plugins }
            })
        }
        protocol::LIST_DEPS => answer!(pm.list_deps().await, |deps| ListDepsResult { deps }),
        protocol::PROJECT_INFO => {
            answer!(pm.project_info().await, |project| ProjectInfoResult {
                project
            })
        }
        protocol::SEARCH => {
            let p: SearchParams = parse!(SearchParams);
            answer!(pm.search(&p.query).await, |plugins| SearchResult {
                plugins
            })
        }
        protocol::FETCH => {
            let p: FetchParams = parse!(FetchParams);
            answer!(pm.fetch(&p.id, p.update).await, |fetched| fetched)
        }
        protocol::REFRESH => {
            let p: RefreshParams = parse!(RefreshParams);
            answer!(pm.refresh(p.update).await, |refreshed| {
                RefreshResult { refreshed }
            })
        }
        other => Response::err(
            id,
            error_code::METHOD_NOT_FOUND,
            format!("unknown method `{other}`"),
        ),
    }
}

/// Deserialize params, treating missing `params` as an empty object so
/// no-argument methods can be called without one.
fn decode<T: DeserializeOwned>(params: Option<serde_json::Value>) -> Result<T, serde_json::Error> {
    serde_json::from_value(params.unwrap_or_else(|| serde_json::Value::Object(Default::default())))
}
