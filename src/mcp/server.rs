//! The meta-server itself.
//!
//! One MCP server is registered with the agent, exposing two tools rather
//! than every plugin server's tools directly: `list_tools` describes what is
//! available, `execute` runs a script against it.
//!
//! Two constraints shape the process:
//!
//! * **stdout carries JSON-RPC.** Anything else written there corrupts the
//!   stream, so all reporting goes to stderr.
//! * **Startup has no side effects.** Some clients probe a server by
//!   spawning a throwaway copy before the real session, so anything done at
//!   startup happens twice, from a process nobody will talk to.

use std::sync::Arc;

use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool, ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServiceExt};
use serde_json::{Map, Value, json};

/// Tool that describes what is reachable.
pub const LIST_TOOLS: &str = "list_tools";
/// Tool that runs a script.
pub const EXECUTE: &str = "execute";

/// Serves the two meta-tools over stdio.
#[derive(Clone)]
pub struct MetaServer {
    catalog: Arc<super::catalog::Catalog>,
    limits: super::sandbox::Limits,
}

impl MetaServer {
    pub fn new(catalog: Arc<super::catalog::Catalog>, limits: super::sandbox::Limits) -> Self {
        Self { catalog, limits }
    }

    /// Run a model-written script with the workspace's tools in scope.
    async fn execute(&self, script: &str) -> CallToolResult {
        let (namespaces, problems) = self.catalog.namespaces().await;
        if namespaces.is_empty() {
            let mut message =
                String::from("No MCP tools are in scope, so there is nothing to call.");
            for problem in &problems {
                message.push_str(&format!("\n  {problem}"));
            }
            return CallToolResult::error(vec![ContentBlock::text(message)]);
        }

        // Tool calls cross back to this runtime: backing servers are child
        // processes owned here, and their I/O cannot be polled from the
        // engine's thread.
        let (calls, mut receiver) = super::dispatch::channel();
        let catalog = Arc::clone(&self.catalog);
        // A server whose answer did not match its declared output schema is
        // reported alongside the result. Collected here because the notice is
        // for the model, not for the script, which receives the value either
        // way.
        let notices: Arc<std::sync::Mutex<Vec<String>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let collected = Arc::clone(&notices);
        let pump = tokio::spawn(async move {
            while let Some(call) = receiver.recv().await {
                let answer = match catalog.call(&call.server, &call.tool, call.args).await {
                    Ok(outcome) => {
                        if let Some(notice) = outcome.notice {
                            let mut seen = collected.lock().expect("notice lock");
                            // The same tool answering off-shape ten times is
                            // one fact about that tool, not ten.
                            if !seen.contains(&notice) {
                                seen.push(notice);
                            }
                        }
                        Ok(outcome.value)
                    }
                    Err(e) => Err(e),
                };
                let _ = call.reply.send(answer);
            }
        });

        let outcome = super::sandbox::Sandbox::new(self.limits)
            .run_script_with(script, &namespaces, calls)
            .await;
        // Aborted, not awaited: the sandbox owns the only senders, so waiting
        // for the channel to close means waiting on the engine thread. Once
        // the script is over, anything still queued is past its deadline.
        pump.abort();

        // Schema mismatches join the pre-existing problems, so everything the
        // model needs to interpret the result arrives with it.
        let mut problems = problems;
        problems.extend(notices.lock().expect("notice lock").drain(..));

        match outcome {
            Ok(outcome) => CallToolResult::success(vec![ContentBlock::text(render_outcome(
                &outcome, &problems,
            ))]),
            Err(e) => {
                // Tagged rather than prose, so the model can tell a limit it
                // exceeded from a mistake in its own code.
                let detail =
                    serde_json::to_string(&e).unwrap_or_else(|_| format!("{{\"error\":\"{e}\"}}"));
                CallToolResult::error(vec![ContentBlock::text(detail)])
            }
        }
    }

    /// The inventory carried in `execute`'s description.
    ///
    /// Naming the servers costs a few tokens each and saves a round trip: a
    /// small workspace needs no discovery call at all. Their tools are not
    /// listed here, so the cost grows with servers rather than with schemas.
    fn execute_description(&self) -> String {
        // Read rather than cached: the set can change mid-session.
        let servers = self.catalog.server_names();
        let mut text = String::from(
            "Run a JavaScript program with the workspace's MCP tools in scope.\n\n\
             Each server is an object whose methods return promises, so a program \
             can call several tools, filter between them, and return only what \
             matters:\n\n  \
             const rows = await sqlx.query({ sql: \"SELECT id FROM users\" });\n  \
             return rows.filter(r => r.id > 100);\n\n\
             Write an async arrow function or a statement body that returns a value. \
             Write plain JavaScript: no type annotations, interfaces, or generics.\n\n",
        );
        if servers.is_empty() {
            text.push_str(
                "No MCP servers apply to this workspace. Nothing is in scope for `execute`.",
            );
        } else {
            text.push_str(&format!(
                "Servers in scope: {}.\nCall `{LIST_TOOLS}` for their tools and signatures.",
                servers.join(", ")
            ));
        }
        text
    }

    fn tool_definitions(&self) -> Vec<Tool> {
        let list = Tool::new(
            LIST_TOOLS,
            "Describe the MCP tools available in this workspace. Returns names and \
             descriptions by default; pass a filter or a higher detail level for full \
             TypeScript signatures.",
            object_schema(json!({
                "type": "object",
                "properties": {
                    "servers": {
                        "type": "array", "items": {"type": "string"},
                        "description": "Restrict to these servers."
                    },
                    "tools": {
                        "type": "array", "items": {"type": "string"},
                        "description": "Restrict to these tool names."
                    },
                    "pattern": {
                        "type": "string",
                        "description": "Glob matched against tool names."
                    },
                    "detail": {
                        "type": "string", "enum": ["names", "signatures", "full"],
                        "description": "How much to return. Defaults to names."
                    }
                }
            })),
        )
        .annotate(ToolAnnotations::new().read_only(true));

        let execute = Tool::new(
            EXECUTE,
            self.execute_description(),
            object_schema(json!({
                "type": "object",
                "properties": {
                    "script": {
                        "type": "string",
                        "description": "The JavaScript program to run."
                    }
                },
                "required": ["script"]
            })),
        )
        // A script can call any tool in scope, so the annotation has to
        // describe the worst of them. Marking it read-only would let a
        // filtering client treat arbitrary tool use as safe.
        .annotate(
            ToolAnnotations::new()
                .read_only(false)
                .destructive(true)
                .open_world(true),
        );

        vec![list, execute]
    }
}

impl ServerHandler for MetaServer {
    fn get_info(&self) -> ServerInfo {
        // The negotiated version is left to the SDK: answering with a version
        // the client did not offer is a hard error that closes the transport.
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("symposium", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Symposium exposes this workspace's MCP tools through two tools. \
                 Start with `execute`; its description names the servers in scope.",
            )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(self.tool_definitions()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        match request.name.as_ref() {
            LIST_TOOLS => {
                let args = request.arguments.map(Value::Object).unwrap_or(Value::Null);
                let query = super::catalog::Query::from_arguments(&args);
                let body = self.catalog.describe(&query).await;
                Ok(CallToolResult::success(vec![ContentBlock::text(body)]).into())
            }
            EXECUTE => {
                let args = request.arguments.map(Value::Object).unwrap_or(Value::Null);
                let Some(script) = args.get("script").and_then(Value::as_str) else {
                    return Err(McpError::invalid_params(
                        "`execute` requires a `script` string",
                        None,
                    ));
                };
                Ok(self.execute(script).await.into())
            }
            other => Err(McpError::invalid_params(
                format!("no such tool: {other}. Available: {LIST_TOOLS}, {EXECUTE}"),
                None,
            )),
        }
    }
}

/// Present a script's result to the model.
///
/// Console output and truncation are reported alongside the value rather than
/// folded into it, so a script that logged its way to an answer is readable
/// without the log being mistaken for the answer.
fn render_outcome(outcome: &super::sandbox::Outcome, problems: &[String]) -> String {
    let mut text = outcome.json.clone();

    if let Some(original) = outcome.truncated_from {
        text.push_str(&format!(
            "\n\n[result truncated from {original} bytes. Return less data \
             — filter or select fields inside the script.]"
        ));
    }
    if !outcome.logs.is_empty() {
        text.push_str("\n\nconsole:\n");
        for line in &outcome.logs {
            text.push_str(&format!("  {line}\n"));
        }
        if outcome.logs_dropped {
            text.push_str("  [further output dropped]\n");
        }
    }
    for problem in problems {
        text.push_str(&format!("\n[{problem}]"));
    }
    text
}

fn object_schema(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

/// Serve until the client disconnects.
pub async fn serve(
    catalog: Arc<super::catalog::Catalog>,
    limits: super::sandbox::Limits,
) -> anyhow::Result<()> {
    let service = MetaServer::new(Arc::clone(&catalog), limits)
        .serve(rmcp::transport::io::stdio())
        .await?;
    let outcome = service.waiting().await;
    // Backing servers outlive the connection otherwise; their process groups
    // die with this process, but a clean close lets them exit on their own.
    catalog.shutdown().await;
    outcome?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::catalog::Catalog;
    use crate::mcp::supervisor::RestartPolicy;

    /// A server with no backing processes; enough to inspect what it
    /// advertises.
    fn test_server(names: &[&str]) -> MetaServer {
        let tmp = tempfile::tempdir().expect("temp dir");
        let sym = Arc::new(crate::config::Symposium::from_dir(tmp.path()));
        let resolution = crate::mcp::resolve::Resolution {
            servers: names
                .iter()
                .map(|name| crate::mcp::resolve::ResolvedServer {
                    name: name.to_string(),
                    command: crate::mcp::resolve::ServerCommand::Path("/usr/bin/true".into()),
                    args: Vec::new(),
                    env: Vec::new(),
                    cwd: None,
                    startup_timeout: std::time::Duration::from_secs(30),
                    tool_call_timeout: std::time::Duration::from_secs(60),
                    enabled_tools: None,
                    disabled_tools: None,
                    requirements: Vec::new(),
                })
                .collect(),
            rejected: Vec::new(),
        };
        let catalog = Catalog::new(
            sym,
            resolution,
            RestartPolicy::default(),
            false,
            tmp.path().to_path_buf(),
        );
        MetaServer::new(Arc::new(catalog), crate::mcp::sandbox::Limits::default())
    }

    #[test]
    fn advertises_exactly_two_tools() {
        let names: Vec<String> = test_server(&[])
            .tool_definitions()
            .iter()
            .map(|t| t.name.to_string())
            .collect();
        assert_eq!(names, vec![LIST_TOOLS.to_string(), EXECUTE.to_string()]);
    }

    /// A client filtering on read-only annotations must not conclude that
    /// running arbitrary code is safe.
    #[test]
    fn execute_is_annotated_as_destructive() {
        let tools = test_server(&[]).tool_definitions();
        let execute = tools.iter().find(|t| t.name == EXECUTE).unwrap();
        let annotations = execute.annotations.as_ref().expect("annotations");
        assert_eq!(annotations.read_only_hint, Some(false));
        assert_eq!(annotations.destructive_hint, Some(true));
        assert_eq!(annotations.open_world_hint, Some(true));
    }

    #[test]
    fn list_tools_is_annotated_read_only() {
        let tools = test_server(&[]).tool_definitions();
        let list = tools.iter().find(|t| t.name == LIST_TOOLS).unwrap();
        assert_eq!(
            list.annotations.as_ref().and_then(|a| a.read_only_hint),
            Some(true)
        );
    }

    /// The inventory rides in the description so a small workspace needs no
    /// discovery round trip.
    #[test]
    fn execute_description_names_servers_in_scope() {
        let server = test_server(&["sqlx", "sea_orm"]);
        let text = server.execute_description();
        assert!(text.contains("Servers in scope: sqlx, sea_orm."), "{text}");
        assert!(text.contains(LIST_TOOLS), "should point at the detail tool");
    }

    #[test]
    fn execute_description_is_explicit_when_nothing_applies() {
        let text = test_server(&[]).execute_description();
        assert!(text.contains("No MCP servers apply"), "{text}");
    }

    /// The model is shown TypeScript declarations, so it has to be told not
    /// to reply in TypeScript.
    #[test]
    fn execute_description_warns_against_type_annotations() {
        let text = test_server(&[]).execute_description();
        assert!(text.contains("no type annotations"), "{text}");
    }
}
