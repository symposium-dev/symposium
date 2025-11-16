//! Research agent that handles a single crate research request.
//!
//! When a user calls the `rust_crate_query` tool, a research agent is spawned
//! to investigate the crate sources and return findings. Each research agent:
//! 1. Creates a new sub-agent session with crate_sources_mcp tools
//! 2. Sends the research prompt to the sub-agent
//! 3. Waits for the sub-agent to complete its investigation
//! 4. Returns the findings to the original caller

use crate::{crate_research_mcp, ResearchState};
use sacp::{
    schema::{NewSessionRequest, NewSessionResponse},
    JrConnectionCx,
};
use sacp_proxy::McpServiceRegistry;
use std::sync::Arc;

/// Run a research agent to investigate a Rust crate.
///
/// This function:
/// 1. Sends NewSessionRequest with the sub-agent MCP server (containing get_rust_crate_source + return_response_to_user)
/// 2. Receives session_id from the agent
/// 3. Registers the session_id in shared ResearchState so the main loop knows this is a research session
/// 4. Sends PromptRequest with the user's research prompt
/// 5. Waits for the sub-agent to call return_response_to_user
/// 6. Sends the response back through request.response_tx (owned by this function)
/// 7. Cleans up the session_id from ResearchState
pub async fn run(
    cx: JrConnectionCx,
    state: Arc<ResearchState>,
    sub_agent_mcp_registry: McpServiceRegistry,
    request: crate_research_mcp::ResearchRequest,
) -> Result<(), sacp::Error> {
    tracing::info!(
        "Handling research request for crate '{}' version {:?}",
        request.crate_name,
        request.crate_version
    );

    // Get current working directory for the new session
    let cwd = std::env::current_dir().map_err(|e| sacp::Error::internal_error())?;

    // Add sub-agent MCP servers to the new session request
    let mut new_session_req = NewSessionRequest {
        cwd,
        mcp_servers: vec![],
        meta: None,
    };
    sub_agent_mcp_registry.add_registered_mcp_servers_to(&mut new_session_req);

    // Spawn the sub-agent session
    let NewSessionResponse {
        session_id,
        modes: _,
        meta: _,
    } = cx.send_request(new_session_req).block_task().await?;

    tracing::info!("Research session created: {}", session_id);

    // Register this session_id in shared state so the main loop knows it's a research session
    {
        let mut active_sessions = state.active_research_session_ids.lock().unwrap();
        active_sessions.insert(session_id.to_string());
    }

    // TODO: Implementation steps:
    // 1. Send NewSessionRequest with sub-agent MCP server
    // 2. Get session_id back
    // 3. Store session_id → request.response_tx in shared state
    // 4. Send PromptRequest(session_id, request.prompt)
    // 5. Wait for sub-agent to call return_response_to_user

    // Placeholder: immediately send a response
    let placeholder_response = format!(
        "Research request received for '{}'. Session spawning not yet implemented.",
        request.crate_name
    );

    request
        .response_tx
        .send(placeholder_response)
        .map_err(|_| sacp::Error::internal_error())?;

    Ok(())
}
