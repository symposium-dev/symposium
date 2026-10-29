//! The package-manager wire protocol: JSON-RPC 2.0 over stdio,
//! newline-delimited.
//!
//! One JSON object per line, in both directions. Nothing in these payloads
//! needs an embedded newline, so the simpler framing is enough: there is no
//! `Content-Length` header. A PM process is long-lived: Symposium spawns it
//! once per invocation, sends [`INITIALIZE`] before anything else, and then
//! sends one request at a time, each answered before the next is sent.
//!
//! The operations are [`PackageManager`](super::PackageManager)'s.
//!
//! See the [PM interface sub-RFD](https://github.com/symposium-dev/symposium/blob/main/md/rfds/registry-centric-plugins/pm-interface/README.md).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use symposium_install::UpdateLevel;

use super::{PackageId, PluginInfo, ProjectInfo, UnvalidatedPlugin};

/// The protocol version this SDK speaks. Negotiation is strict: Symposium
/// refuses a PM reporting a version it does not know, and that PM contributes
/// no plugins.
pub const PROTOCOL_VERSION: u32 = 1;

pub const INITIALIZE: &str = "initialize";
pub const ACTIVE_PLUGINS: &str = "active_plugins";
pub const LOAD_PLUGIN: &str = "load_plugin";
pub const LIST_DEPS: &str = "list_deps";
pub const PROJECT_INFO: &str = "project_info";
pub const SEARCH: &str = "search";
pub const FETCH: &str = "fetch";
pub const REFRESH: &str = "refresh";

/// Sent once, before any other method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeParams {
    /// The version Symposium speaks.
    pub protocol_version: u32,
    /// The workspace this PM answers for, if there is one. Fixed for the
    /// connection's lifetime, which is why `list_deps` takes no arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<PathBuf>,
    /// A cache directory for this PM alone. It caches whatever it likes there,
    /// and Symposium may delete it at any time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_dir: Option<PathBuf>,
}

/// The PM's answer to [`InitializeParams`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    /// The version the PM speaks.
    pub protocol_version: u32,
    /// The name this PM owns: the `pm` component of every id it mints.
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivePluginsParams {
    /// The workspace's dependency set. A registry ignores it; an ecosystem
    /// transport reports what these dependencies embed.
    #[serde(default)]
    pub deps: Vec<PackageId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadPluginParams {
    /// The id to resolve. Its version component may be a requirement.
    pub id: PackageId,
    /// Under `none`, serve only what is already on disk.
    #[serde(default)]
    pub update: UpdateLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchParams {
    pub query: String,
}

/// The params of `fetch`. The result is a
/// [`FetchedPackage`](super::FetchedPackage).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchParams {
    pub id: PackageId,
    #[serde(default)]
    pub update: UpdateLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshParams {
    #[serde(default)]
    pub update: UpdateLevel,
}

/// The result shape of `refresh`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshResult {
    /// Whether a remote source was pulled.
    pub refreshed: bool,
}

/// Unvalidated plugins, the result shape of `load_plugin`. `active_plugins`
/// answers with a [`PluginListing`](super::PluginListing): the same, plus
/// whether that is every plugin (`complete`, true when left out).
#[derive(Debug, Serialize, Deserialize)]
pub struct PluginsResult {
    pub plugins: Vec<UnvalidatedPlugin>,
}

/// Infos, the result shape of `search`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub plugins: Vec<PluginInfo>,
}

/// Ids, the result shape of `list_deps`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListDepsResult {
    pub deps: Vec<PackageId>,
}

/// The result shape of `project_info`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectInfoResult {
    #[serde(default)]
    pub project: ProjectInfo,
}

// --- JSON-RPC envelope ---

/// A request. Every request carries an `id`: the protocol has no
/// notifications.
#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl Request {
    pub fn new(id: u64, method: impl Into<String>, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.into(),
            params,
        }
    }
}

/// A response: exactly one of `result` / `error` is set.
#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ResponseError>,
}

impl Response {
    pub fn ok(id: u64, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: u64, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(ResponseError {
                code,
                message: message.into(),
            }),
        }
    }
}

/// The error half of a response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseError {
    pub code: i32,
    pub message: String,
}

/// Error codes. Whatever the code, Symposium treats a failed operation the
/// same way: the PM contributes nothing to it, logged, and one broken PM never
/// aborts a sync or a hook.
pub mod error_code {
    /// Standard JSON-RPC: the method is not implemented.
    pub const METHOD_NOT_FOUND: i32 = -32601;
    /// Standard JSON-RPC: the params did not deserialize.
    pub const INVALID_PARAMS: i32 = -32602;
    /// Standard JSON-RPC: the operation failed.
    pub const INTERNAL_ERROR: i32 = -32603;
    /// The request was well-formed but semantically invalid.
    pub const INVALID_INPUT: i32 = -32003;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_and_response_are_single_line_json() {
        let req = Request::new(
            1,
            LOAD_PLUGIN,
            Some(serde_json::json!({"id": {"pm": "cargo", "name": "serde", "version": "*"}})),
        );
        let line = serde_json::to_string(&req).unwrap();
        assert!(
            !line.contains('\n'),
            "framing depends on one line per message"
        );
        let back: Request = serde_json::from_str(&line).unwrap();
        assert_eq!(back.method, LOAD_PLUGIN);
        assert_eq!(back.id, 1);

        let resp = Response::ok(1, serde_json::json!({"plugins": []}));
        let line = serde_json::to_string(&resp).unwrap();
        assert!(!line.contains('\n'));
        let back: Response = serde_json::from_str(&line).unwrap();
        assert!(back.error.is_none());
    }

    #[test]
    fn update_serializes_to_the_documented_spellings() {
        let spelled = |update| serde_json::to_string(&update).unwrap();
        assert_eq!(spelled(UpdateLevel::None), "\"none\"");
        assert_eq!(spelled(UpdateLevel::Check), "\"check\"");
        assert_eq!(spelled(UpdateLevel::Fetch), "\"fetch\"");
    }
}
