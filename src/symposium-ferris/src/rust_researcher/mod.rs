//! The rust_researcher tool - research Rust crates using an LLM sub-agent.
//!
//! This tool spawns a sub-agent session that has access to crate source fetching
//! tools. The sub-agent researches the user's question and returns findings.

use std::sync::{Arc, Mutex};

use indoc::formatdoc;
use sacp::{
    ProxyToConductor,
    mcp_server::{McpContext, McpServerBuilder},
    schema::{
        RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse, StopReason,
    },
    util::MatchMessage,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

mod sub_agent_mcp;

/// Parameters for the rust_researcher tool
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RustResearcherParams {
    /// Name of the Rust crate to research
    pub crate_name: String,
    /// Optional semver range (e.g., "1.0", "^1.2", "~1.2.3")
    /// Defaults to latest version if not specified
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crate_version: Option<String>,
    /// Research prompt describing what information you need about the crate.
    /// Examples:
    /// - "How do I use the derive macro for custom field names?"
    /// - "What are the signatures of all methods on tokio::runtime::Runtime?"
    /// - "Show me an example of using async-trait with associated types"
    pub prompt: String,
}

/// Output from the rust_researcher tool
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct RustResearcherOutput {
    /// The research findings
    pub result: Vec<serde_json::Value>,
}

/// Register the rust_researcher tool with the MCP server builder.
pub fn register(
    builder: McpServerBuilder<ProxyToConductor, impl sacp::JrResponder<ProxyToConductor>>,
    enabled: bool,
) -> McpServerBuilder<ProxyToConductor, impl sacp::JrResponder<ProxyToConductor>> {
    builder.tool_fn_mut(
        "rust_researcher",
        indoc::indoc! {r#"
            Research a Rust crate by examining its actual source code using an LLM sub-agent.

            The researcher agent will explore the crate sources and return relevant code
            examples, signatures, and implementation details.

            Examples:
            - "Show me how to create a tokio::runtime::Runtime and spawn tasks"
            - "What fields are available on serde::Deserialize? I'm getting a compilation error"
            - "How do I use async-trait with associated types?"
            - "What's the signature of reqwest::Client::get()?"
        "#},
        async move |input: RustResearcherParams, mcp_cx: McpContext<ProxyToConductor>| {
            if !enabled {
                return Err(sacp::util::internal_error(
                    "rust_researcher tool is not enabled",
                ));
            }
            run_research(input, mcp_cx).await
        },
        sacp::tool_fn_mut!(),
    )
}

/// Build the research prompt with context and instructions for the sub-agent.
fn build_research_prompt(user_prompt: &str) -> String {
    formatdoc! {"
        <agent_instructions>
        You are an expert Rust programmer who has been asked advice on a particular question.
        You have available to you an MCP server that can fetch the sources for Rust crates.
        When you have completed researching the answer to the question, you can invoke the
        `return_response_to_user` tool. If you are answering a question with more than one
        answer, you can invoke the tool more than once and all the invocations will be returned.

        IMPORTANT: You are a *researcher*, you are not here to make changes. Do NOT edit files,
        make git commits, or perform any other permanent changes.

        The research prompt provided by the user is as follows. If you encounter critical
        ambiguities, use the return_response_to_user tool to request a refined prompt and
        describe the ambiguities you encountered.
        </agent_instructions>

        <research_prompt>
        {user_prompt}
        </research_prompt>
    "}
}

/// Run a research query using a sub-agent session.
async fn run_research(
    input: RustResearcherParams,
    mcp_cx: McpContext<ProxyToConductor>,
) -> Result<RustResearcherOutput, sacp::Error> {
    let RustResearcherParams {
        crate_name,
        crate_version,
        prompt,
    } = input;

    tracing::info!(
        crate_name = %crate_name,
        crate_version = ?crate_version,
        "Starting rust researcher"
    );
    tracing::debug!(prompt = %prompt, "Research prompt");

    let cx = mcp_cx.connection_cx();

    // Create a channel for receiving responses from the sub-agent's return_response_to_user calls
    let responses: Arc<Mutex<Vec<serde_json::Value>>> = Default::default();
    let mcp_server = sub_agent_mcp::build_server(responses.clone());

    // Spawn the sub-agent session with the per-instance MCP server
    // Use current directory since we don't have access to session cwd here
    let cwd = std::env::current_dir().unwrap_or_default();

    cx.build_session(cwd)
        .with_mcp_server(mcp_server)?
        .block_task()
        .run_until(async |mut active_session| {
            tracing::debug!(session_id = ?active_session.session_id(), "Research session active");

            active_session.send_prompt(build_research_prompt(&prompt))?;
            tracing::debug!("Sent research prompt to session");

            loop {
                match active_session.read_update().await? {
                    sacp::SessionMessage::SessionMessage(message_cx) => {
                        MatchMessage::new(message_cx)
                            .if_request(async |request: RequestPermissionRequest, request_cx| {
                                approve_tool_request(request, request_cx)
                            })
                            .await
                            .otherwise(async |message| {
                                // Log any other messages, we don't care about them
                                tracing::trace!(?message);
                                Ok(())
                            })
                            .await?
                    }

                    // Once the turn is over, we stop.
                    sacp::SessionMessage::StopReason(stop_reason) => match stop_reason {
                        StopReason::EndTurn => {
                            // Once the agent finishes its turn, results should have been collected
                            let result =
                                std::mem::take(&mut *responses.lock().expect("not poisoned"));
                            return Ok(RustResearcherOutput { result });
                        }

                        // Other stop reasons are an error
                        StopReason::MaxTokens
                        | StopReason::MaxTurnRequests
                        | StopReason::Refusal
                        | StopReason::Cancelled => {
                            return Err(sacp::util::internal_error(format!(
                                "researcher stopped early: {stop_reason:?}"
                            )));
                        }
                    },

                    // Anything else, just ignore
                    _ => {}
                }
            }
        })
        .await
}

fn approve_tool_request(
    request: RequestPermissionRequest,
    request_cx: sacp::JrRequestCx<RequestPermissionResponse>,
) -> Result<(), sacp::Error> {
    let outcome = request
        .options
        .iter()
        .find(|option| match option.kind {
            sacp::schema::PermissionOptionKind::AllowOnce
            | sacp::schema::PermissionOptionKind::AllowAlways => true,
            sacp::schema::PermissionOptionKind::RejectOnce
            | sacp::schema::PermissionOptionKind::RejectAlways => false,
        })
        .map(|option| RequestPermissionOutcome::Selected {
            option_id: option.id.clone(),
        })
        .unwrap_or(RequestPermissionOutcome::Cancelled);

    request_cx.respond(RequestPermissionResponse {
        outcome,
        meta: None,
    })
}
