//! Research agent that handles a single crate research request.
//!
//! When a user calls the `rust_crate_query` tool, a research agent is spawned
//! to investigate the crate sources and return findings. Each research agent:
//! 1. Creates a new sub-agent session with crate_sources_mcp tools
//! 2. Sends the research prompt to the sub-agent
//! 3. Waits for the sub-agent to complete its investigation
//! 4. Returns the findings to the original caller

use crate::{crate_research_mcp, crate_sources_mcp, state::ResearchState};
use indoc::formatdoc;
use sacp::{
    schema::{
        NewSessionRequest, NewSessionResponse, PromptRequest, PromptResponse,
        RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    },
    Handled, JrConnectionCx, JrMessageHandler, MessageAndCx,
};
use sacp_proxy::McpServiceRegistry;
use sacp_rmcp::McpServiceRegistryRmcpExt;
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
    fn describe_chain(&self) -> impl std::fmt::Debug {
        "permission-auto-approver"
    }

    async fn handle_message(
        &mut self,
        message: MessageAndCx,
    ) -> Result<Handled<MessageAndCx>, sacp::Error> {
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
            .done()
    }
}

/// Run a research agent to investigate a Rust crate.
///
/// This function:
/// 1. Creates a fresh MCP service registry with a per-instance response channel
/// 2. Sends NewSessionRequest with the sub-agent MCP server (containing get_rust_crate_source + return_response_to_user)
/// 3. Receives session_id from the agent
/// 4. Registers the session_id in shared ResearchState so the main loop knows this is a research session
/// 5. Sends PromptRequest with the user's research prompt
/// 6. Waits for responses from the sub-agent via return_response_to_user calls
/// 7. Accumulates all responses and sends them back through request.response_tx
/// 8. Cleans up the session_id from ResearchState
pub async fn run(
    cx: JrConnectionCx,
    state: Arc<ResearchState>,
    request: crate_research_mcp::ResearchRequest,
) -> Result<(), sacp::Error> {
    tracing::info!(
        "Handling research request for crate '{}' version {:?}",
        request.crate_name,
        request.crate_version
    );

    // Create a channel for receiving responses from the sub-agent's return_response_to_user calls
    let (response_tx, mut response_rx) = mpsc::channel::<serde_json::Value>(32);

    // Create a fresh MCP service registry for this research session
    // The SubAgentService instance holds the response_tx to send findings back
    let sub_agent_mcp_registry = McpServiceRegistry::default()
        .with_rmcp_server("rust-crate-sources", move || {
            crate_sources_mcp::SubAgentService::new(response_tx.clone())
        })?;

    // Spawn the sub-agent session with the per-instance MCP registry
    let NewSessionResponse {
        session_id,
        modes: _,
        meta: _,
    } = cx
        .send_request(research_agent_session_request(sub_agent_mcp_registry)?)
        .block_task()
        .await?;

    tracing::info!("Research session created: {}", session_id);

    // Register this session_id in shared state so the main loop knows it's a research session
    state.register_session(&session_id);

    let mut responses = vec![];
    let (result, _) = futures::future::select(
        // Collect responses from the response channel
        pin!(async {
            while let Some(response) = response_rx.recv().await {
                responses.push(response);
            }
            Ok::<(), sacp::Error>(())
        }),
        pin!(async {
            let research_prompt = build_research_prompt(&request.prompt);
            let prompt_request = PromptRequest {
                session_id: session_id.clone(),
                prompt: vec![research_prompt.into()],
                meta: None,
            };

            let PromptResponse {
                stop_reason,
                meta: _,
            } = cx.send_request(prompt_request).block_task().await?;

            tracing::info!("Research complete for session {session_id} ({stop_reason:?})",);

            Ok::<(), sacp::Error>(())
        }),
    )
    .await
    .factor_first();
    result?;

    // Unregister the session now that research is complete
    state.unregister_session(&session_id);

    // Send back the accumulated responses
    request
        .response_tx
        .send(if responses.len() == 1 {
            responses.pop().expect("singleton")
        } else {
            serde_json::Value::Array(responses)
        })
        .map_err(|_| sacp::Error::internal_error())?;

    Ok(())
}

/// Create a NewSessionRequest for the research agent.
fn research_agent_session_request(
    sub_agent_mcp_registry: McpServiceRegistry,
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
