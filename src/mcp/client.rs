//! Talking to one backing MCP server.
//!
//! Everything protocol-shaped is confined here. The rest of `mcp` works in
//! `serde_json::Value`, so a future SDK change touches this file and nothing
//! else — worth insisting on, given the SDK moved a major version during this
//! work.
//!
//! Two behaviors are less obvious than they look:
//!
//! * **A tool failure is not a protocol failure.** A server reports "table
//!   not found" as a successful response carrying an error flag. Conflating
//!   the two loses the message the model needs.
//! * **A result has to be unwrapped.** The wire form is a content envelope,
//!   but a script wants the value. See [`unwrap_result`].

use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::header::{HeaderName, HeaderValue};
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock, ProtocolVersion, Tool};
use rmcp::service::{ClientInitializeError, RoleClient, RunningService};
use rmcp::transport::auth::{AuthClient, AuthorizationManager};
use rmcp::transport::streamable_http_client::{
    AuthRequiredError, InsufficientScopeError, StreamableHttpClient,
    StreamableHttpClientTransportConfig,
};
use rmcp::transport::{StreamableHttpClientTransport, TokioChildProcess};
use serde_json::{Map, Value};

use super::{credentials, endpoint};

/// Everything needed to start a backing server.
#[derive(Debug, Clone)]
pub struct SpawnSpec {
    pub name: String,
    pub startup_timeout: Duration,
    pub kind: SpawnKind,
}

#[derive(Debug, Clone)]
pub enum SpawnKind {
    Child {
        command: PathBuf,
        args: Vec<String>,
        env: Vec<(String, String)>,
        /// Directory to run in. A server that reads the project it serves needs
        /// this; four of the seven client config formats have it.
        cwd: Option<PathBuf>,
    },
    Http {
        url: String,
        headers: Vec<(String, String)>,
        /// Where stored OAuth credentials for this server are looked for.
        config_dir: PathBuf,
    },
}

/// Why talking to a backing server failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientError {
    /// Spawn or handshake did not finish in time; `detail` is the stderr tail.
    StartupTimeout {
        server: String,
        limit_secs: u64,
        detail: Option<String>,
    },
    /// The process could not be started, or died during the handshake.
    StartupFailed { server: String, detail: String },
    /// A single call did not finish in time.
    CallTimeout {
        server: String,
        tool: String,
        limit_secs: u64,
    },
    /// The connection broke, or the server rejected the request.
    Protocol { server: String, detail: String },
    /// The tool ran and reported failure. Distinct from the above: the
    /// message is the server's own, and belongs in front of the model.
    Tool { message: String },
    /// A remote server answered with an authorization challenge. Retrying
    /// cannot help, so callers treat it as terminal until the user logs in.
    AuthRequired { server: String, challenge: String },
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StartupTimeout {
                server,
                limit_secs,
                detail,
            } => match detail {
                Some(tail) => write!(f, "{server} did not start within {limit_secs}s: {tail}"),
                None => write!(f, "{server} did not start within {limit_secs}s"),
            },
            Self::StartupFailed { server, detail } => {
                write!(f, "{server} failed to start: {detail}")
            }
            Self::CallTimeout {
                server,
                tool,
                limit_secs,
            } => write!(f, "{server}.{tool} did not answer within {limit_secs}s"),
            Self::Protocol { server, detail } => write!(f, "{server}: {detail}"),
            Self::Tool { message } => write!(f, "{message}"),
            // Names the command, because this message is read by the agent and
            // relayed to a user who otherwise has no way to know what to do.
            Self::AuthRequired { server, .. } => write!(
                f,
                "{server} requires authorization; run `cargo agents mcp login {server}`"
            ),
        }
    }
}

impl std::error::Error for ClientError {}

/// The `WWW-Authenticate` challenge behind a handshake failure, if any.
///
/// Read from the error chain rather than its text, so a reworded message does
/// not silently turn a login prompt back into an opaque failure. The chain has
/// to be entered through `TransportError`'s field: that variant renders the
/// transport error into its own message without exposing it as a source, so
/// walking `source()` from the top finds nothing.
fn auth_challenge(error: &ClientInitializeError) -> Option<String> {
    let ClientInitializeError::TransportError { error, .. } = error else {
        return None;
    };

    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(error.error.as_ref());
    while let Some(error) = current {
        if let Some(required) = error.downcast_ref::<AuthRequiredError>() {
            return Some(required.www_authenticate_header.clone());
        }
        if let Some(scope) = error.downcast_ref::<InsufficientScopeError>() {
            return Some(scope.www_authenticate_header.clone());
        }
        current = error.source();
    }
    None
}

/// A connected backing server.
pub struct BackingServer {
    name: String,
    service: RunningService<RoleClient, ()>,
    protocol_version: ProtocolVersion,
}

impl BackingServer {
    /// Spawn the server and complete the handshake.
    ///
    /// The SDK waits on the handshake indefinitely, so the deadline is
    /// imposed here. A server fetched on first use can spend most of its
    /// budget just downloading.
    pub async fn spawn(spec: &SpawnSpec) -> Result<Self, ClientError> {
        match &spec.kind {
            SpawnKind::Child {
                command,
                args,
                env,
                cwd,
            } => Self::spawn_child(spec, command, args, env, cwd.as_deref()).await,
            SpawnKind::Http {
                url,
                headers,
                config_dir,
            } => Self::connect_http(spec, url, headers, config_dir).await,
        }
    }

    async fn spawn_child(
        spec: &SpawnSpec,
        program: &PathBuf,
        args: &[String],
        env: &[(String, String)],
        cwd: Option<&std::path::Path>,
    ) -> Result<Self, ClientError> {
        let mut command = tokio::process::Command::new(program);
        command.args(args);
        for (key, value) in env {
            command.env(key, value);
        }
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }

        // Stderr is captured from spawn rather than after the handshake:
        // when startup fails, its tail is the only account of why.
        let (transport, stderr) = TokioChildProcess::builder(command)
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| ClientError::StartupFailed {
                server: spec.name.clone(),
                detail: e.to_string(),
            })?;

        let started = tokio::time::timeout(spec.startup_timeout, ().serve(transport)).await;

        let service = match started {
            Ok(Ok(service)) => service,
            Ok(Err(e)) => {
                let detail = match drain(stderr).await {
                    Some(tail) if !tail.is_empty() => format!("{e}: {tail}"),
                    _ => e.to_string(),
                };
                return Err(ClientError::StartupFailed {
                    server: spec.name.clone(),
                    detail,
                });
            }
            Err(_) => {
                // A server that hung mid-handshake usually said why on stderr.
                let detail = drain(stderr).await.filter(|tail| !tail.is_empty());
                return Err(ClientError::StartupTimeout {
                    server: spec.name.clone(),
                    limit_secs: spec.startup_timeout.as_secs(),
                    detail,
                });
            }
        };

        Ok(Self::from_service(spec.name.clone(), service))
    }

    async fn connect_http(
        spec: &SpawnSpec,
        url: &str,
        headers: &[(String, String)],
        config_dir: &Path,
    ) -> Result<Self, ClientError> {
        let mut custom = std::collections::HashMap::new();
        for (key, value) in headers {
            let name =
                HeaderName::from_bytes(key.as_bytes()).map_err(|_| ClientError::StartupFailed {
                    server: spec.name.clone(),
                    detail: format!("`{key}` is not a valid header name"),
                })?;
            let value = HeaderValue::from_str(value).map_err(|_| ClientError::StartupFailed {
                server: spec.name.clone(),
                detail: format!("value for header `{key}` is not valid"),
            })?;
            custom.insert(name, value);
        }

        let checked = endpoint::check_url(url).map_err(|e| ClientError::StartupFailed {
            server: spec.name.clone(),
            detail: e.to_string(),
        })?;
        endpoint::check_resolved(&checked)
            .await
            .map_err(|e| ClientError::StartupFailed {
                server: spec.name.clone(),
                detail: e.to_string(),
            })?;

        // Redirects are not followed: a 302 to a new host would escape the
        // check just performed, which is the documented SSRF route.
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| ClientError::StartupFailed {
                server: spec.name.clone(),
                detail: e.to_string(),
            })?;

        let config =
            StreamableHttpClientTransportConfig::with_uri(url.to_string()).custom_headers(custom);

        // A stored token turns the plain client into an authorizing one, which
        // also refreshes on expiry. Without one the connection is attempted
        // anyway: a server may not require auth, and one that does answers with
        // the challenge that tells the user to log in.
        match Self::authorizing_client(spec, url, config_dir, http.clone()).await {
            Some(client) => Self::serve_streamable(spec, client, config).await,
            None => Self::serve_streamable(spec, http, config).await,
        }
    }

    async fn authorizing_client(
        spec: &SpawnSpec,
        url: &str,
        config_dir: &Path,
        http: reqwest::Client,
    ) -> Option<AuthClient<reqwest::Client>> {
        let store = credentials::FileCredentialStore::new(config_dir, &spec.name);
        if !store.exists() {
            return None;
        }

        let mut manager = match AuthorizationManager::new(url).await {
            Ok(manager) => manager,
            Err(e) => {
                tracing::warn!(server = %spec.name, error = %e, "could not prepare authorization");
                return None;
            }
        };
        manager.set_credential_store(store);

        match manager.initialize_from_store().await {
            Ok(true) => Some(AuthClient::new(http, manager)),
            Ok(false) => None,
            Err(e) => {
                tracing::warn!(server = %spec.name, error = %e, "stored credentials unusable");
                None
            }
        }
    }

    async fn serve_streamable<C>(
        spec: &SpawnSpec,
        client: C,
        config: StreamableHttpClientTransportConfig,
    ) -> Result<Self, ClientError>
    where
        C: StreamableHttpClient + Send + Sync + 'static,
    {
        let transport = StreamableHttpClientTransport::with_client(client, config);

        // No stderr to quote for a remote server; the transport error is the
        // whole account of a failure.
        match tokio::time::timeout(spec.startup_timeout, ().serve(transport)).await {
            Ok(Ok(service)) => Ok(Self::from_service(spec.name.clone(), service)),
            Ok(Err(e)) => Err(match auth_challenge(&e) {
                Some(challenge) => ClientError::AuthRequired {
                    server: spec.name.clone(),
                    challenge,
                },
                None => ClientError::StartupFailed {
                    server: spec.name.clone(),
                    detail: e.to_string(),
                },
            }),
            Err(_) => Err(ClientError::StartupTimeout {
                server: spec.name.clone(),
                limit_secs: spec.startup_timeout.as_secs(),
                detail: None,
            }),
        }
    }

    fn from_service(name: String, service: RunningService<RoleClient, ()>) -> Self {
        let protocol_version = service
            .peer_info()
            .map(|info| info.protocol_version.clone())
            .unwrap_or_default();
        Self {
            name,
            service,
            protocol_version,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// The version actually negotiated.
    ///
    /// Worth recording rather than assuming: structured output arrived in
    /// 2025-06-18, and servers pinning an earlier version are deployed today.
    pub fn protocol_version(&self) -> &ProtocolVersion {
        &self.protocol_version
    }

    pub async fn list_tools(&self) -> Result<Vec<Tool>, ClientError> {
        self.service
            .list_all_tools()
            .await
            .map_err(|e| ClientError::Protocol {
                server: self.name.clone(),
                detail: e.to_string(),
            })
    }

    pub async fn call(
        &self,
        tool: &str,
        args: Value,
        timeout: Duration,
    ) -> Result<Value, ClientError> {
        let params = CallToolRequestParams::new(tool.to_string());
        let params = match args {
            Value::Object(map) => params.with_arguments(map),
            Value::Null => params.with_arguments(Map::new()),
            // A non-object argument has no place in the wire form.
            other => params.with_arguments(Map::from_iter([("value".to_string(), other)])),
        };

        let response = tokio::time::timeout(timeout, self.service.call_tool(params))
            .await
            .map_err(|_| ClientError::CallTimeout {
                server: self.name.clone(),
                tool: tool.to_string(),
                limit_secs: timeout.as_secs(),
            })?
            .map_err(|e| ClientError::Protocol {
                server: self.name.clone(),
                detail: e.to_string(),
            })?;

        unwrap_result(&response)
    }

    /// Close the connection, giving the server a chance to exit cleanly.
    pub async fn shutdown(self) {
        let _ = self.service.cancel().await;
    }
}

/// Reduce a tool response to the value a script should see.
///
/// The order matters. A server may set the error flag *and* return structured
/// content; checking content first would return the payload and silently drop
/// the failure.
pub fn unwrap_result(result: &CallToolResult) -> Result<Value, ClientError> {
    if result.is_error.unwrap_or(false) {
        return Err(ClientError::Tool {
            message: joined_text(&result.content).unwrap_or_else(|| "tool failed".to_string()),
        });
    }

    if let Some(structured) = &result.structured_content {
        return Ok(unwrap_framework_envelope(structured.clone()));
    }

    // All-text is the common case, and multi-block text results are ordinary.
    if let Some(text) = joined_text(&result.content) {
        return Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)));
    }

    // Mixed or binary content has no scalar form; hand back the envelope
    // rather than inventing one.
    Ok(serde_json::to_value(&result.content).unwrap_or(Value::Null))
}

/// Unwrap the single-key envelope one major server framework adds around
/// non-object return values.
///
/// It wraps any scalar or list in `{"result": ...}` and flags it on the
/// output schema. Passing that through would hand the model a wrapper it
/// never asked for, and the framework is common enough that this is not an
/// edge case.
fn unwrap_framework_envelope(value: Value) -> Value {
    let Value::Object(map) = &value else {
        return value;
    };
    if map.len() == 1 {
        if let Some(inner) = map.get("result") {
            return inner.clone();
        }
    }
    value
}

fn joined_text(content: &[ContentBlock]) -> Option<String> {
    let mut parts = Vec::new();
    for block in content {
        match block {
            ContentBlock::Text(text) => parts.push(text.text.clone()),
            // A single non-text block means this is not a text result.
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("\n"))
}

async fn drain(stderr: Option<tokio::process::ChildStderr>) -> Option<String> {
    use tokio::io::AsyncReadExt;
    let mut stderr = stderr?;
    let mut buffer = Vec::new();
    let _ = tokio::time::timeout(Duration::from_millis(200), stderr.read_to_end(&mut buffer)).await;
    let text = String::from_utf8_lossy(&buffer).trim().to_string();
    // Only the tail is useful, and an unbounded one could carry secrets far.
    Some(
        text.chars()
            .rev()
            .take(400)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::{CallToolResult, ContentBlock};
    use serde_json::json;

    fn text_result(parts: &[&str]) -> CallToolResult {
        CallToolResult::success(parts.iter().map(|t| ContentBlock::text(*t)).collect())
    }

    // -- unwrapping --

    #[test]
    fn structured_content_is_returned_directly() {
        let result = CallToolResult::structured(json!({"rows": [1, 2]}));
        assert_eq!(unwrap_result(&result).unwrap(), json!({"rows": [1, 2]}));
    }

    /// A server can report failure *and* attach structured content. Reading
    /// the content first would return a payload and lose the error.
    #[test]
    fn error_flag_wins_over_structured_content() {
        let mut result = CallToolResult::structured(json!({"rows": []}));
        result.is_error = Some(true);
        result.content = vec![ContentBlock::text("table not found")];

        let err = unwrap_result(&result).unwrap_err();
        assert_eq!(
            err,
            ClientError::Tool {
                message: "table not found".to_string()
            }
        );
    }

    #[test]
    fn text_that_parses_as_json_is_parsed() {
        let result = text_result(&[r#"{"ok": true}"#]);
        assert_eq!(unwrap_result(&result).unwrap(), json!({"ok": true}));
    }

    #[test]
    fn plain_text_is_returned_as_a_string() {
        let result = text_result(&["up to date"]);
        assert_eq!(unwrap_result(&result).unwrap(), json!("up to date"));
    }

    /// Multi-block text results are ordinary, not an edge case.
    #[test]
    fn several_text_blocks_are_joined() {
        let result = text_result(&["line one", "line two"]);
        assert_eq!(unwrap_result(&result).unwrap(), json!("line one\nline two"));
    }

    /// One widely used server framework wraps every non-object return value
    /// in a single-key envelope.
    #[test]
    fn framework_result_envelope_is_unwrapped() {
        let result = CallToolResult::structured(json!({"result": [1, 2, 3]}));
        assert_eq!(unwrap_result(&result).unwrap(), json!([1, 2, 3]));
    }

    /// An ordinary object that happens to have one key is not an envelope
    /// unless that key is the envelope's.
    #[test]
    fn single_key_objects_are_not_mistaken_for_envelopes() {
        let result = CallToolResult::structured(json!({"rows": [1]}));
        assert_eq!(unwrap_result(&result).unwrap(), json!({"rows": [1]}));
    }

    #[test]
    fn error_without_text_still_reports_failure() {
        let mut result = CallToolResult::success(vec![]);
        result.is_error = Some(true);
        assert!(matches!(
            unwrap_result(&result).unwrap_err(),
            ClientError::Tool { .. }
        ));
    }
}
