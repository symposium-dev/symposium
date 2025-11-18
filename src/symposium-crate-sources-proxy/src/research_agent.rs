//! Research agent that handles a single crate research request.
//!
//! When a user calls the `rust_crate_query` tool, a research agent is spawned
//! to investigate the crate sources and return findings. Each research agent:
//! 1. Creates a new sub-agent session with crate_sources_mcp tools
//! 2. Sends the research prompt to the sub-agent
//! 3. Waits for the sub-agent to complete its investigation
//! 4. Returns the findings to the original caller

use crate::state::ResearchState;
use indoc::formatdoc;
use sacp::{
    schema::{
        NewSessionRequest, RequestPermissionOutcome, RequestPermissionRequest,
        RequestPermissionResponse,
    },
    Handled, JrMessageHandler, MessageAndCx,
};
use sacp_proxy::McpServiceRegistry;
use std::sync::Arc;

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

/// Create a NewSessionRequest for the research agent.
pub fn research_agent_session_request(
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
