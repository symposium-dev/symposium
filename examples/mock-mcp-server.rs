//! A configurable MCP server, for exercising the meta-server against
//! behavior real servers exhibit but cannot be asked to reproduce on demand.
//!
//! Driven entirely by a JSON config, so one binary plays every role a test
//! needs — a slow start, a hang, a crash partway through, a response too big
//! to pass on, an older protocol version:
//!
//! ```json
//! {
//!   "name": "sqlx",
//!   "startup_delay_ms": 0,
//!   "fail_startup_times": 0,
//!   "tools": [
//!     { "name": "query", "inputSchema": { "type": "object" },
//!       "behavior": { "kind": "echo" } },
//!     { "name": "slow",  "behavior": { "kind": "hang" } },
//!     { "name": "boom",  "behavior": { "kind": "crash_after", "calls": 1 } }
//!   ]
//! }
//! ```
//!
//! An `examples/` target rather than a `[[bin]]`: `cargo test` builds it, but
//! it is not installed alongside `cargo-agents`.
//!
//! `fail_startup_times` counts across runs via a sibling file, so a restart
//! policy can be tested without the harness having to rewrite the config.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rmcp::handler::server::ServerHandler;
use rmcp::model::*;
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServiceExt};
use serde::Deserialize;
use serde_json::{Map, Value, json};

#[derive(Debug, Deserialize)]
struct Config {
    #[serde(default = "default_name")]
    name: String,
    /// Delay before serving, for exercising a startup timeout.
    #[serde(default)]
    startup_delay_ms: u64,
    /// Exit during startup this many times before succeeding, for exercising
    /// restart policy.
    #[serde(default)]
    fail_startup_times: usize,
    /// Protocol version to report. Older versions predate structured output.
    #[serde(default)]
    protocol_version: Option<String>,
    /// Appended to on every start, so a test can assert a server did *not*
    /// start.
    #[serde(default)]
    startup_log: Option<PathBuf>,
    #[serde(default)]
    tools: Vec<ToolConfig>,
}

fn default_name() -> String {
    "mock".to_string()
}

#[derive(Debug, Clone, Deserialize)]
struct ToolConfig {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(rename = "inputSchema", default)]
    input_schema: Option<Value>,
    #[serde(rename = "outputSchema", default)]
    output_schema: Option<Value>,
    #[serde(default)]
    behavior: Behavior,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Behavior {
    /// Return the arguments as structured content.
    #[default]
    Echo,
    /// Return a fixed string.
    Text { text: String },
    /// Return a tool error, as distinct from a protocol error.
    Error { message: String },
    /// Never answer, for exercising a per-call timeout.
    Hang,
    /// Answer normally, then exit the process once `calls` have been served.
    CrashAfter { calls: usize },
    /// Return a payload of the given size, for exercising a result cap.
    Bloat { bytes: usize },
    /// Answer after a delay.
    Delay { ms: u64, text: String },
}

#[derive(Clone)]
struct Mock {
    config: Arc<Config>,
    calls: Arc<AtomicUsize>,
}

impl ServerHandler for Mock {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(self.config.name.clone(), "0.0.0"));
        if let Some(version) = &self.config.protocol_version {
            // The field is private and there is no constructor from a string;
            // its `Deserialize` is the supported way in.
            if let Ok(parsed) = serde_json::from_value(json!(version)) {
                info = info.with_protocol_version(parsed);
            }
        }
        info
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let tools = self
            .config
            .tools
            .iter()
            .map(|tool| {
                let schema = tool
                    .input_schema
                    .clone()
                    .unwrap_or_else(|| json!({"type": "object"}));
                let mut definition = Tool::new(
                    tool.name.clone(),
                    tool.description.clone().unwrap_or_default(),
                    as_object(schema),
                );
                if let Some(output) = tool.output_schema.clone() {
                    definition = definition.with_raw_output_schema(Arc::new(as_object(output)));
                }
                definition
            })
            .collect();
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let Some(tool) = self
            .config
            .tools
            .iter()
            .find(|t| t.name == request.name.as_ref())
        else {
            return Err(McpError::invalid_params(
                format!("no such tool: {}", request.name),
                None,
            ));
        };

        let args = request.arguments.map(Value::Object).unwrap_or(Value::Null);
        let served = self.calls.fetch_add(1, Ordering::SeqCst) + 1;

        match &tool.behavior {
            Behavior::Echo => Ok(CallToolResult::structured(args).into()),
            Behavior::Text { text } => {
                Ok(CallToolResult::success(vec![ContentBlock::text(text)]).into())
            }
            Behavior::Error { message } => {
                Ok(CallToolResult::error(vec![ContentBlock::text(message)]).into())
            }
            Behavior::Hang => {
                // Outlives any plausible test timeout without pinning a CPU.
                std::future::pending::<()>().await;
                unreachable!()
            }
            Behavior::Delay { ms, text } => {
                tokio::time::sleep(std::time::Duration::from_millis(*ms)).await;
                Ok(CallToolResult::success(vec![ContentBlock::text(text)]).into())
            }
            Behavior::Bloat { bytes } => {
                Ok(CallToolResult::structured(json!({ "blob": "x".repeat(*bytes) })).into())
            }
            Behavior::CrashAfter { calls } => {
                if served > *calls {
                    // Abrupt, as a real server dying mid-session would be.
                    std::process::exit(70);
                }
                Ok(CallToolResult::structured(json!({ "served": served })).into())
            }
        }
    }
}

fn as_object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

/// Count startup failures across runs.
///
/// A restart policy can only be tested if the server fails a fixed number of
/// times and then recovers, which needs state the process itself does not
/// keep.
fn should_fail_startup(config_path: &PathBuf, budget: usize) -> bool {
    if budget == 0 {
        return false;
    }
    let counter = config_path.with_extension("startups");
    let so_far: usize = std::fs::read_to_string(&counter)
        .ok()
        .and_then(|t| t.trim().parse().ok())
        .unwrap_or(0);
    let _ = std::fs::write(&counter, (so_far + 1).to_string());
    so_far < budget
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let mut config_path: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        if arg == "--config" {
            config_path = args.next().map(PathBuf::from);
        }
    }
    let Some(config_path) = config_path else {
        anyhow::bail!("usage: mock-mcp-server --config <path>");
    };

    let config: Config = serde_json::from_str(&std::fs::read_to_string(&config_path)?)?;

    if let Some(path) = &config.startup_log {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(file, "{}", config.name);
        }
    }

    if should_fail_startup(&config_path, config.fail_startup_times) {
        eprintln!("mock-mcp-server: failing startup on purpose");
        std::process::exit(1);
    }
    if config.startup_delay_ms > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(config.startup_delay_ms)).await;
    }

    let mock = Mock {
        config: Arc::new(config),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let service = mock.serve(rmcp::transport::io::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
