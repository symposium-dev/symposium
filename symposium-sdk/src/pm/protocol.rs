//! The package-manager wire protocol: JSON-RPC 2.0 over stdio,
//! newline-delimited.
//!
//! One JSON object per line, in both directions. Nothing in these payloads
//! needs an embedded newline, so the simpler framing is enough: there is no
//! `Content-Length` header. A PM process is long-lived: Symposium spawns it
//! once per invocation, sends [`INITIALIZE`] before anything else, and may have
//! several requests in flight, matched by `id`.
//!
//! See the [PM interface sub-RFD](https://github.com/symposium-dev/symposium/blob/main/md/rfds/registry-centric-plugins/pm-interface/README.md).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{PackageId, PluginInfo, PluginOffer};

/// The protocol version this SDK speaks. Negotiation is strict: Symposium
/// refuses a PM reporting a version it does not know, and that PM contributes
/// no plugins.
pub const PROTOCOL_VERSION: u32 = 1;

pub const INITIALIZE: &str = "initialize";
pub const ACTIVE_PLUGINS: &str = "active_plugins";
pub const LOAD_PLUGIN: &str = "load_plugin";
pub const LIST_DEPS: &str = "list_deps";
pub const SEARCH: &str = "search";
pub const FETCH: &str = "fetch";
pub const REFRESH: &str = "refresh";

/// How aggressively an already-cached package should be refreshed.
///
/// Mirrors Symposium's `UpdateLevel`. `None` carries the contract that matters
/// most: it must not make a network call, which is what keeps per-event hook
/// dispatch offline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Update {
    /// Serve from cache; never touch the network.
    #[default]
    None,
    /// Check for a newer version, and take it if there is one.
    Check,
    /// Re-acquire even when the cache looks current.
    Fetch,
}

/// Sent once, before any other method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeParams {
    /// The version Symposium speaks.
    pub protocol_version: u32,
    /// The workspace this PM answers for, if there is one. Fixed for the
    /// connection's lifetime, which is why `list_deps` takes no arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<PathBuf>,
    /// Symposium's cache directory, for PMs that want to cache under it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_dir: Option<PathBuf>,
    /// Environment overrides the PM should honor (e.g. `SYMPOSIUM_CARGO`
    /// pointing at a specific cargo binary). Passed explicitly rather than
    /// inherited so the contract is visible.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

/// The PM's answer to [`InitializeParams`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    /// The version the PM speaks.
    pub protocol_version: u32,
    /// The name this PM owns: the `pm` component of every id it mints.
    pub name: String,
    /// Optional operations this PM implements. Symposium skips the rest.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
}

/// Capability names a PM may report.
pub mod capability {
    /// Implements `search`.
    pub const SEARCH: &str = "search";
    /// Implements `list_deps`: i.e. has a notion of a workspace.
    pub const LIST_DEPS: &str = "list_deps";
    /// Implements `refresh`: i.e. has remote content to pull.
    pub const REFRESH: &str = "refresh";
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchParams {
    pub query: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchParams {
    pub id: PackageId,
    #[serde(default)]
    pub update: Update,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchResult {
    /// The exact resolved id: never a requirement.
    pub id: PackageId,
    /// Where the content landed. The PM owns this directory and guarantees it
    /// stays valid for the connection's lifetime.
    pub root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshParams {
    #[serde(default)]
    pub update: Update,
    /// Ignore the source's auto-update opt-out (an explicit `plugin sync`).
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshResult {
    /// Whether a remote source was actually pulled, so callers can report what
    /// synced. False for a PM whose content is already local.
    pub refreshed: bool,
}

/// Offers, the result shape of `active_plugins` and `load_plugin`.
#[derive(Debug, Serialize, Deserialize)]
pub struct OffersResult {
    pub offers: Vec<PluginOffer>,
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

// --- JSON-RPC envelope ---

/// A request. `id` is absent for a notification, which this protocol does not
/// currently use.
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseError {
    pub code: i32,
    pub message: String,
}

/// Error codes. Beyond these, any failure degrades to "this PM contributes
/// nothing", logged: one broken PM never aborts a sync or a hook.
pub mod error_code {
    /// Standard JSON-RPC: the method is not implemented.
    pub const METHOD_NOT_FOUND: i32 = -32601;
    /// Standard JSON-RPC: the params did not deserialize.
    pub const INVALID_PARAMS: i32 = -32602;
    /// The package does not exist. Skipped gracefully, reported in `status`.
    pub const NOT_FOUND: i32 = -32001;
    /// A network operation failed. Falls back to cache.
    pub const NETWORK: i32 = -32002;
    /// The request was well-formed but semantically invalid.
    pub const INVALID_INPUT: i32 = -32003;
    /// Credentials are needed. Reported to the user with setup instructions.
    pub const AUTH_REQUIRED: i32 = -32004;
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

        let resp = Response::ok(1, serde_json::json!({"offers": []}));
        let line = serde_json::to_string(&resp).unwrap();
        assert!(!line.contains('\n'));
        let back: Response = serde_json::from_str(&line).unwrap();
        assert!(back.error.is_none());
    }

    #[test]
    fn update_serializes_to_the_documented_spellings() {
        assert_eq!(serde_json::to_string(&Update::None).unwrap(), "\"none\"");
        assert_eq!(serde_json::to_string(&Update::Check).unwrap(), "\"check\"");
        assert_eq!(serde_json::to_string(&Update::Fetch).unwrap(), "\"fetch\"");
    }
}
