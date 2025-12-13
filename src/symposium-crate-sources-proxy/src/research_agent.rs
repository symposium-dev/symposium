//! Research agent coordination and MCP tool implementation.
//!
//! Provides the `rust_crate_query` MCP tool and handles research session lifecycle:
//! 1. User calls rust_crate_query tool with crate name and research prompt
//! 2. Tool handler spawns a new sub-agent session with crate_sources_mcp tools
//! 3. Sends the research prompt to the sub-agent
//! 4. Auto-approves permissions and logs session notifications
//! 5. Collects responses and returns findings to the user

use crate::{crate_sources_mcp, state::ResearchState};
use indoc::formatdoc;
use sacp::{
    mcp_server::{McpServer, McpServiceRegistry},
    schema::{
        NewSessionRequest, NewSessionResponse, PromptRequest, PromptResponse,
        RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
        SessionNotification,
    },
    Handled, JrConnectionCx, JrMessageHandler, MessageCx, ProxyToConductor,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{pin::pin, sync::Arc};
use tokio::sync::mpsc;

/// Handler for auto-approving permission requests from research sessions.
pub struct PermissionAutoApprover {
    state: Arc<ResearchState>,
}

impl PermissionAutoApprover {
    pub fn new(state: Arc<ResearchState>) -> Self {
        Self { state }
    }
}

impl JrMessageHandler for PermissionAutoApprover {
    type Role = ProxyToConductor;

    fn describe_chain(&self) -> impl std::fmt::Debug {
        "permission-auto-approver"
    }

    async fn handle_message(
        &mut self,
        message: MessageCx,
        _cx: JrConnectionCx<Self::Role>,
    ) -> Result<Handled<MessageCx>, sacp::Error> {
        sacp::util::MatchMessage::new(message)
            .if_request(async |request: RequestPermissionRequest, request_cx| {
                // Auto-approve all permissions for research sessions
                if self.state.is_research_session(&request.session_id) {
                    tracing::debug!(
                        "Auto-approving permission request for research session {}",
                        request.session_id
                    );

                    // Find the first option that looks like "allow" and use it.
                    for option in &request.options {
                        match option.kind {
                            sacp::schema::PermissionOptionKind::AllowOnce
                            | sacp::schema::PermissionOptionKind::AllowAlways => {
                                request_cx.respond(RequestPermissionResponse {
                                    outcome: RequestPermissionOutcome::Selected {
                                        option_id: option.id.clone(),
                                    },
                                    meta: None,
                                })?;
                                return Ok(Handled::Yes);
                            }
                            sacp::schema::PermissionOptionKind::RejectOnce
                            | sacp::schema::PermissionOptionKind::RejectAlways => {}
                        }
                    }
                }

                Ok(Handled::No((request, request_cx)))
            })
            .await
            .if_notification({
                let state = self.state.clone();
                async move |notification: SessionNotification| {
                    // Log all notifications for research sessions
                    if state.is_research_session(&notification.session_id) {
                        tracing::debug!("Research session notification: {:?}", notification);
                        return Ok(Handled::Yes);
                    }

                    Ok(Handled::No(notification))
                }
            })
            .await
            .done()
    }
}

/// Create a NewSessionRequest for the research agent.
pub fn research_agent_session_request(
    sub_agent_mcp_registry: McpServiceRegistry<ProxyToConductor>,
) -> Result<NewSessionRequest, sacp::Error> {
    let cwd = std::env::current_dir().map_err(|_| sacp::Error::internal_error())?;
    let mut new_session_req = NewSessionRequest {
        cwd,
        mcp_servers: vec![],
        meta: None,
    };
    sub_agent_mcp_registry.add_registered_mcp_servers_to(&mut new_session_req);
    Ok(new_session_req)
}

/// Build the research prompt with context and instructions for the sub-agent.
pub fn build_research_prompt(user_prompt: &str) -> String {
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

/// Parameters for the rust_crate_query tool
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RustCrateQueryParams {
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

/// Output from the rust_crate_query tool
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct RustCrateQueryOutput {
    /// The research findings
    result: serde_json::Value,
}

/// Build the MCP server for crate research queries
pub fn build_server(state: Arc<ResearchState>) -> McpServer<ProxyToConductor> {
    McpServer::new()
        .instructions(indoc::indoc! {"
            Research Rust crate source code and APIs. Essential for working with unfamiliar crates.

            When to use:
            - Before using a new crate: get usage examples and understand the API
            - When compilation fails: verify actual method signatures, available fields, correct types
            - When implementation details matter: explore how features work internally
            - When documentation is unclear: see concrete code examples
        "})
        .tool_fn(
            "rust_crate_query",
            indoc::indoc! {r#"
                Research a Rust crate by examining its actual source code.

                Examples:
                - "Show me how to create a tokio::runtime::Runtime and spawn tasks"
                - "What fields are available on serde::Deserialize? I'm getting a compilation error"
                - "How do I use async-trait with associated types?"
                - "What's the signature of reqwest::Client::get()?"

                The research agent will examine the crate sources and return relevant code examples, signatures, and implementation details.
            "#},
            {
                async move |input: RustCrateQueryParams, mcp_cx: sacp::mcp_server::McpContext<ProxyToConductor>| {
                    let RustCrateQueryParams {
                        crate_name,
                        crate_version,
                        prompt,
                    } = input;

                    tracing::info!(
                        "Received crate query for '{}' version: {:?}",
                        crate_name,
                        crate_version
                    );
                    tracing::debug!("Research prompt: {}", prompt);

                    let cx = mcp_cx.connection_cx();

                    // Create a channel for receiving responses from the sub-agent's return_response_to_user calls
                    let (response_tx, mut response_rx) = mpsc::channel::<serde_json::Value>(32);

                    // Create a fresh MCP service registry for this research session
                    let sub_agent_mcp_registry = McpServiceRegistry::new()
                        .with_mcp_server(
                            "rust-crate-sources",
                            crate_sources_mcp::build_server(response_tx.clone()),
                        )
                        .map_err(|e| anyhow::anyhow!("Failed to create MCP registry: {}", e))?;

                    // Spawn the sub-agent session with the per-instance MCP registry
                    let NewSessionResponse {
                        session_id,
                        modes: _,
                        meta: _,
                    } = cx
                        .send_request_to(sacp::Agent, research_agent_session_request(
                            sub_agent_mcp_registry,
                        ).map_err(|e| anyhow::anyhow!("Failed to create session request: {}", e))?)
                        .block_task()
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to spawn research session: {}", e))?;

                    tracing::info!("Research session created: {}", session_id);

                    // Register this session_id in shared state so permission requests are auto-approved
                    state.register_session(&session_id);

                    let mut responses = vec![];
                    let (result, _) = futures::future::select(
                        // Collect responses from the response channel
                        pin!(async {
                            while let Some(response) = response_rx.recv().await {
                                responses.push(response);
                            }
                            Ok::<(), anyhow::Error>(())
                        }),
                        pin!(async {
                            let research_prompt = build_research_prompt(&prompt);
                            let prompt_request = PromptRequest {
                                session_id: session_id.clone(),
                                prompt: vec![research_prompt.into()],
                                meta: None,
                            };

                            let PromptResponse {
                                stop_reason,
                                meta: _,
                            } = cx
                                .send_request_to(sacp::Agent, prompt_request)
                                .block_task()
                                .await
                                .map_err(|e| anyhow::anyhow!("Prompt request failed: {}", e))?;

                            tracing::info!(
                                "Research complete for session {session_id} ({stop_reason:?})"
                            );

                            Ok::<(), anyhow::Error>(())
                        }),
                    )
                    .await
                    .factor_first();
                    result?;

                    // Unregister the session now that research is complete
                    state.unregister_session(&session_id);

                    // Return the accumulated responses
                    let response = if responses.len() == 1 {
                        responses.pop().expect("singleton")
                    } else {
                        serde_json::Value::Array(responses)
                    };

                    tracing::info!("Research complete for '{}'", crate_name);

                    Ok(RustCrateQueryOutput { result: response })
                }
            },
            |f, args, cx| Box::pin(f(args, cx)),
        )
}
