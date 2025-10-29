//! Dialectic MCP Server implementation using the official rmcp SDK
//!
//! Provides get_selection, ide_operation, and present_walkthrough tools for AI assistants
//! to interact with the VSCode extension via IPC.

use crate::structured_logging;
use anyhow::Result;
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    service::RequestContext,
    tool, tool_handler, tool_router,
};
use rust_embed::RustEmbed;
use serde_json;
use std::future::Future;
use tracing::{debug, error, info, warn};

use crate::dialect::DialectInterpreter;
use crate::eg::Eg;
use crate::ipc::IPCCommunicator;
use crate::types::PresentWalkthroughParams;
use serde::{Deserialize, Serialize};

/// Embedded guidance files for agent initialization
#[derive(RustEmbed)]
#[folder = "src/guidance/"]
struct GuidanceFiles;

/// Parameters for the expand_reference tool
// ANCHOR: expand_reference_params
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExpandReferenceParams {
    /// The reference ID to expand
    pub id: String,
}
// ANCHOR_END: expand_reference_params

/// Parameters for the ide_operation tool
// ANCHOR: ide_operation_params
#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
struct IdeOperationParams {
    /// Dialect program to execute
    program: String,
}
// ANCHOR_END: ide_operation_params

/// Parameters for the spawn_taskspace tool
// ANCHOR: spawn_taskspace_params
#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
struct SpawnTaskspaceParams {
    /// Name for the new taskspace
    name: String,
    /// Description of the task to be performed
    task_description: String,
    /// Initial prompt to provide to the agent when it starts
    initial_prompt: String,
}
// ANCHOR_END: spawn_taskspace_params

/// Parameters for the log_progress tool
// ANCHOR: log_progress_params
#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
struct LogProgressParams {
    /// Progress message to display
    message: String,
    /// Category for visual indicator (info, warn, error, milestone, question)
    category: String,
}
// ANCHOR_END: log_progress_params

/// Parameters for the signal_user tool
// ANCHOR: signal_user_params
#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
struct SignalUserParams {
    /// Message describing why user attention is needed
    message: String,
}
// ANCHOR_END: signal_user_params

/// Parameters for the update_taskspace tool
// ANCHOR: update_taskspace_params
#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
struct UpdateTaskspaceParams {
    /// New name for the taskspace
    name: String,
    /// New description for the taskspace
    description: String,
}
// ANCHOR_END: update_taskspace_params

/// Parameters for the get_rust_crate_source tool
#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
struct GetRustCrateSourceParams {
    /// Name of the crate to search
    crate_name: String,
    /// Optional semver range (e.g., "1.0", "^1.2", "~1.2.3")
    version: Option<String>,
    /// Optional search pattern (regex)
    pattern: Option<String>,
}

/// Dialectic MCP Server
///
/// Implements the MCP server protocol and bridges to VSCode extension via IPC.
/// Uses the official rmcp SDK with tool macros for clean implementation.
#[derive(Clone)]
pub struct SymposiumServer {
    ipc: IPCCommunicator,
    interpreter: DialectInterpreter<IPCCommunicator>,
    tool_router: ToolRouter<SymposiumServer>,
    reference_handle: crate::actor::ReferenceHandle,
}

#[tool_router]
impl SymposiumServer {
    /// Assemble the complete /yiasou initialization prompt
    /// Get taskspace context via IPC
    async fn get_taskspace_context(
        &self,
    ) -> Result<(Option<String>, Option<String>, Option<String>)> {
        match self.ipc.get_taskspace_state().await {
            Ok(state) => Ok((state.name, state.description, state.initial_prompt)),
            Err(e) => {
                warn!("Failed to get taskspace context via IPC: {}", e);
                // Log the error but don't fail the prompt assembly
                tracing::warn!("Failed to get taskspace context: {}", e);
                Ok((None, None, None))
            }
        }
    }

    /// Check if we're currently in a taskspace by looking for task-UUID directory structure
    fn is_in_taskspace(&self) -> bool {
        let result = crate::ipc::extract_project_info().is_ok();
        if !result {
            if let Err(e) = crate::ipc::extract_project_info() {
                warn!("extract_project_info failed: {}", e);
            }
        }
        result
    }

    pub async fn new(options: crate::Options) -> Result<Self> {
        // Try to discover VSCode PID by walking up the process tree
        let current_pid = std::process::id();
        let shell_pid = match crate::pid_discovery::find_vscode_pid_from_mcp(current_pid).await? {
            Some((vscode_pid, shell_pid)) => {
                info!("Discovered VSCode PID: {vscode_pid} and shell PID: {shell_pid}");
                Some(shell_pid)
            }
            None => {
                info!(
                    "Could not discover VSCode PID from process tree - continuing without shell PID"
                );
                None
            }
        };

        // Connect to the global message bus daemon (started by VSCode extension or other clients)

        // Create shared reference handle for both IPC and MCP tools
        let reference_handle = crate::actor::ReferenceHandle::new();

        let mut ipc = IPCCommunicator::new(shell_pid, reference_handle.clone(), options).await?;

        // Initialize IPC connection to message bus daemon (not directly to VSCode)
        ipc.initialize().await?;
        info!("IPC communication with message bus daemon initialized");

        // Set up log forwarding to subscribers
        Self::setup_log_forwarding(&ipc);

        // Send unsolicited Polo message to announce our presence
        ipc.send_polo().await?;

        // Initialize Dialect interpreter with IDE functions
        let mut interpreter = DialectInterpreter::new(ipc.clone());
        interpreter.add_standard_ide_functions();

        Ok(Self {
            ipc: ipc.clone(),
            interpreter,
            tool_router: Self::tool_router(),
            reference_handle,
        })
    }

    /// Get a reference to the IPC communicator
    pub fn ipc(&self) -> &IPCCommunicator {
        &self.ipc
    }

    /// Set up log forwarding to subscribers via IPC
    fn setup_log_forwarding(ipc: &IPCCommunicator) {
        let mut log_rx = structured_logging::add_log_subscriber();
        let ipc = ipc.clone();
        tokio::spawn(async move {
            while let Some((level, message)) = log_rx.recv().await {
                ipc.send_log_message(level, message).await;
            }
        });
    }

    /// Creates a new DialecticServer in test mode
    /// In test mode, IPC operations are mocked and don't require a VSCode connection
    pub fn new_test() -> Self {
        let reference_handle = crate::actor::ReferenceHandle::new();
        let ipc = IPCCommunicator::new_test(reference_handle.clone());
        info!("DialecticServer initialized in test mode");

        // Initialize Dialect interpreter with IDE functions for test mode
        let mut interpreter = DialectInterpreter::new(ipc.clone());
        interpreter.add_standard_ide_functions();

        Self {
            ipc,
            interpreter,
            tool_router: Self::tool_router(),
            reference_handle,
        }
    }

    /// Display a code walkthrough in VSCode
    ///
    /// Walkthroughs are structured guides with introduction, highlights, changes, and actions.
    /// Test tool to verify guidance loading by returning the assembled /yiasou prompt
    #[tool(
        description = "Test guidance loading by returning the assembled /yiasou prompt (temporary for Phase 1)"
    )]
    async fn test_yiasou_prompt(&self) -> Result<CallToolResult, McpError> {
        match self.assemble_yiasou_prompt().await {
            Ok(prompt) => Ok(CallToolResult::success(vec![Content::text(prompt)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to assemble yiasou prompt: {}",
                e
            ))])),
        }
    }

    /// Display a code walkthrough in VSCode using markdown with embedded XML elements.
    /// Accepts markdown content with special XML tags (comment, gitdiff, action, mermaid)
    /// as described in the dialectic guidance documentation.
    // ANCHOR: present_walkthrough_tool
    #[tool(description = "\
            Display a code walkthrough in the user's IDE.\n\
            Use this when the user\n\
            (1) requests a walkthrough or that you walk through code or\n\
            (2) asks that you explain how code works.\n\
            \n\
            Accepts markdown content with special code blocks.\n\
            \n\
            To find full guidelines for usage, use the `expand_reference` with `walkthrough-format.md`.\n\
            \n\
            Quick tips:\n\
            \n\
            Display a mermaid graph:\n\
            ```mermaid\n\
            (Mermaid content goes here)\n\
            ```\n\
            \n\
            Add a comment to a particular line of code:\n\
            ```comment\n\
            location: findDefinition(`symbol_name`)\n\
            \n\
            (Explanatory text goes here)\n\
            ```\n\
            \n\
            Add buttons that will let the user send you a message:\n\
            ```action\n\
            button: (what the user sees)\n\
            \n\
            (what message you will get)\n\
            ```\n\
        ")]
    async fn present_walkthrough(
        &self,
        Parameters(params): Parameters<PresentWalkthroughParams>,
    ) -> Result<CallToolResult, McpError> {
        // ANCHOR_END: present_walkthrough_tool
        debug!(
            "Received present_walkthrough tool call with markdown content ({} chars)",
            params.content.len()
        );

        // Parse markdown with XML elements and resolve Dialect expressions
        let mut parser =
            crate::walkthrough_parser::WalkthroughParser::new(self.interpreter.clone())
                .with_base_uri(params.base_uri.clone());
        let resolved_html = parser
            .parse_and_normalize(&params.content)
            .await
            .map_err(|e| {
                McpError::internal_error(
                    "Failed to parse walkthrough markdown",
                    Some(serde_json::json!({"error": e.to_string()})),
                )
            })?;

        // Convert baseURI to absolute path, fallback to current working directory
        let absolute_base_uri = std::path::Path::new(&params.base_uri)
            .canonicalize()
            .or_else(|_| crate::workspace_dir::current_dir())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| params.base_uri.clone());

        // Create resolved walkthrough with HTML content
        let resolved = crate::ide::ResolvedWalkthrough {
            content: resolved_html,
            base_uri: absolute_base_uri,
        };

        // Send resolved walkthrough to VSCode extension
        self.ipc.present_walkthrough(resolved).await.map_err(|e| {
            McpError::internal_error(
                "Failed to present walkthrough",
                Some(serde_json::json!({"error": e.to_string()})),
            )
        })?;

        // Log success
        info!("Walkthrough successfully sent to VSCode");

        Ok(CallToolResult::success(vec![Content::text(
            "Walkthrough successfully processed and presented in VSCode",
        )]))
    }

    /// Get the currently selected text from any active editor in VSCode
    ///
    /// Works with source files, review panels, and any other text editor.
    /// Returns null if no text is selected or no active editor is found.
    // ANCHOR: get_selection_tool
    #[tool(description = "\
            Get the currently selected text from any active editor in VSCode.\n\
            Works with source files, review panels, and any other text editor.\n\
            Returns null if no text is selected or no active editor is found.\
        ")]
    async fn get_selection(&self) -> Result<CallToolResult, McpError> {
        // ANCHOR_END: get_selection_tool
        // Request current selection from VSCode extension via IPC
        info!("Requesting current selection from VSCode extension...");

        let result = self.ipc.get_selection().await.map_err(|e| {
            McpError::internal_error(
                "IPC communication failed",
                Some(serde_json::json!({
                    "error": e.to_string()
                })),
            )
        })?;

        let status_msg = if result.selected_text.is_some() {
            "text selected"
        } else {
            "no selection"
        };

        info!("selection retrieved: {}", status_msg);

        // Convert result to JSON and return
        let json_content = Content::json(result).map_err(|e| {
            McpError::internal_error(
                "Serialization failed",
                Some(serde_json::json!({
                    "error": format!("Failed to serialize selection result: {}", e)
                })),
            )
        })?;

        Ok(CallToolResult::success(vec![json_content]))
    }

    /// Execute IDE operations using Dialect mini-language
    ///
    /// Provides access to VSCode's Language Server Protocol (LSP) capabilities
    /// through a composable function system for symbol resolution and reference finding.
    // ANCHOR: ide_operation_tool
    #[tool(description = "\
            Execute IDE operations using a structured JSON mini-language.\n\
            This tool provides access to VSCode's Language Server Protocol (LSP) capabilities\n\
            through a composable function system.\n\
            \n\
            Common operations:\n\
            - findDefinitions(\"MyFunction\") or findDefinition(\"MyFunction\") - list of locations where a symbol named `MyFunction` is defined\n\
            - findReferences(\"MyFunction\") - list of locations where a symbol named `MyFunction` is referenced\n\
            \n\
            To find full guidelines for usage, use the `expand_reference` with `walkthrough-format.md`.\n\
            ")]
    async fn ide_operation(
        &self,
        Parameters(params): Parameters<IdeOperationParams>,
    ) -> Result<CallToolResult, McpError> {
        // ANCHOR_END: ide_operation_tool
        debug!(
            "Received ide_operation tool call with program: {:?}",
            params.program
        );

        info!("Executing Dialect program...");

        let program = params.program;
        let mut interpreter = self.interpreter.clone();

        let result = tokio::task::spawn_blocking(move || {
            tokio::runtime::Handle::current().block_on(async move {
                // Parse and evaluate the Dialect program string
                interpreter.evaluate(&program).await
            })
        })
        .await
        .map_err(|e| {
            McpError::internal_error(
                "Task execution failed",
                Some(serde_json::json!({
                    "error": e.to_string()
                })),
            )
        })?
        .map_err(|e| {
            McpError::internal_error(
                "Dialect execution failed",
                Some(serde_json::json!({
                    "error": e.to_string()
                })),
            )
        })?;

        info!("Dialect execution completed successfully");

        // Convert result to JSON and return
        let json_content = Content::json(result).map_err(|e| {
            McpError::internal_error(
                "Serialization failed",
                Some(serde_json::json!({
                    "error": format!("Failed to serialize Dialect result: {}", e)
                })),
            )
        })?;

        Ok(CallToolResult::success(vec![json_content]))
    }

    /// Create a synthetic pull request from Git commit range with AI insight comments
    ///
    /// Analyzes Git changes and extracts AI insight comments (💡❓TODO/FIXME) to create
    /// a PR-like review interface with structured file changes and comment threads.
    // ANCHOR: request_review_tool

    /// Expand a compact reference to get full context
    ///
    /// This tool allows LLMs to retrieve the full context for a compact symposium-ref reference.
    // ANCHOR: expand_reference_tool
    #[tool(description = "
        Expand a compact reference (denoted as `<symposium-ref id='..'/>`) to get full context. \
        Invoke with the contents of `id` attribute. Returns structured JSON with all available context data. \
    ")]
    async fn expand_reference(
        &self,
        Parameters(params): Parameters<ExpandReferenceParams>,
    ) -> Result<CallToolResult, McpError> {
        // ANCHOR_END: expand_reference_tool
        debug!("Expanding reference: {}", params.id);

        // First, try to get from reference actor
        if let Some(context) = self.reference_handle.get_reference(&params.id).await {
            info!("Reference {} expanded successfully", params.id);

            return Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&context).map_err(|e| {
                    McpError::internal_error(
                        "Failed to serialize reference context",
                        Some(serde_json::json!({
                            "error": e.to_string()
                        })),
                    )
                })?,
            )]));
        }

        // Not found in reference actor, try guidance files
        if let Some(file) = GuidanceFiles::get(&params.id) {
            let content = String::from_utf8_lossy(&file.data);

            info!("Guidance file {} loaded successfully", params.id);

            return Ok(CallToolResult::success(vec![Content::text(
                content.to_string(),
            )]));
        }

        // Special case: "yiasou" or "hi" returns the same content as @yiasou stored prompt
        if params.id == "yiasou" || params.id == "hi" {
            match self.assemble_yiasou_prompt().await {
                Ok(prompt_content) => {
                    info!("Yiasou prompt assembled successfully via expand_reference");

                    return Ok(CallToolResult::success(vec![Content::text(prompt_content)]));
                }
                Err(e) => {
                    return Err(McpError::internal_error(
                        "Failed to assemble yiasou prompt",
                        Some(serde_json::json!({
                            "error": e.to_string()
                        })),
                    ));
                }
            }
        }

        // Not found in either store
        info!("Reference {} not found", params.id);

        Err(McpError::invalid_params(
            "Reference not found",
            Some(serde_json::json!({
                "reference_id": params.id
            })),
        ))
    }

    /// Create a new taskspace with initial prompt
    ///
    /// This tool allows agents to spawn new taskspaces for collaborative work.
    /// The taskspace will be created with the specified name, description, and initial prompt.
    // ANCHOR: spawn_taskspace_tool
    #[tool(
        description = "Create a new taskspace with name, description, and initial prompt. \
                       The new taskspace will be launched with VSCode and the configured agent tool."
    )]
    async fn spawn_taskspace(
        &self,
        Parameters(params): Parameters<SpawnTaskspaceParams>,
    ) -> Result<CallToolResult, McpError> {
        // ANCHOR_END: spawn_taskspace_tool
        info!("Creating new taskspace: {}", params.name);

        // Send spawn_taskspace message to Symposium app via daemon
        match self
            .ipc
            .spawn_taskspace(
                params.name.clone(),
                params.task_description,
                params.initial_prompt,
            )
            .await
        {
            Ok(()) => {
                info!("Taskspace '{}' created successfully", params.name);

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Taskspace '{}' created successfully",
                    params.name
                ))]))
            }
            Err(e) => {
                error!("Failed to create taskspace '{}': {}", params.name, e);

                Err(McpError::internal_error(
                    "Failed to create taskspace",
                    Some(serde_json::json!({
                        "error": e.to_string(),
                        "taskspace_name": params.name
                    })),
                ))
            }
        }
    }

    /// Report progress from agent with visual indicators
    ///
    /// This tool allows agents to report their progress to the Symposium panel
    /// with different visual categories for better user awareness.
    // ANCHOR: log_progress_tool
    #[tool(description = "Report progress with visual indicators. \
                       Categories: 'info' or ℹ️, 'warn' or ⚠️, 'error' or ❌, 'milestone' or ✅, 'question' or ❓")]
    async fn log_progress(
        &self,
        Parameters(params): Parameters<LogProgressParams>,
    ) -> Result<CallToolResult, McpError> {
        // ANCHOR_END: log_progress_tool
        // Parse category string to enum (accept both text and emoji forms)
        let category = match params.category.to_lowercase().as_str() {
            "info" | "ℹ️" => crate::types::ProgressCategory::Info,
            "warn" | "⚠️" => crate::types::ProgressCategory::Warn,
            "error" | "❌" => crate::types::ProgressCategory::Error,
            "milestone" | "✅" => crate::types::ProgressCategory::Milestone,
            "question" | "❓" => crate::types::ProgressCategory::Question,
            _ => crate::types::ProgressCategory::Info, // Default to info for unknown categories
        };

        debug!("Logging progress: {} ({})", params.message, params.category);

        // Send log_progress message to Symposium app via daemon
        match self
            .ipc
            .log_progress(params.message.clone(), category)
            .await
        {
            Ok(()) => {
                info!("Progress logged successfully");

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Progress logged: {}",
                    params.message
                ))]))
            }
            Err(e) => {
                error!("Failed to log progress: {}", e);

                Err(McpError::internal_error(
                    "Failed to log progress",
                    Some(serde_json::json!({
                        "error": e.to_string(),
                        "message": params.message
                    })),
                ))
            }
        }
    }

    /// Request user attention for assistance
    ///
    /// This tool allows agents to signal when they need user attention,
    /// causing the taskspace to move toward the front of the Symposium panel.
    // ANCHOR: signal_user_tool
    #[tool(description = "Request user attention for assistance. \
                       The taskspace will be highlighted and moved toward the front of the panel.")]
    async fn signal_user(
        &self,
        Parameters(params): Parameters<SignalUserParams>,
    ) -> Result<CallToolResult, McpError> {
        // ANCHOR_END: signal_user_tool
        info!("Requesting user attention: {}", params.message);

        // Send signal_user message to Symposium app via daemon
        match self.ipc.signal_user(params.message.clone()).await {
            Ok(()) => {
                info!("User attention requested successfully");

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "User attention requested: {}",
                    params.message
                ))]))
            }
            Err(e) => {
                error!("Failed to request user attention: {}", e);

                Err(McpError::internal_error(
                    "Failed to request user attention",
                    Some(serde_json::json!({
                        "error": e.to_string(),
                        "message": params.message
                    })),
                ))
            }
        }
    }

    // ANCHOR: update_taskspace_tool
    #[tool(
        description = "Update the name and description of the current taskspace. \
                       Use this to set meaningful names and descriptions based on user interaction."
    )]
    async fn update_taskspace(
        &self,
        Parameters(params): Parameters<UpdateTaskspaceParams>,
    ) -> Result<CallToolResult, McpError> {
        // ANCHOR_END: update_taskspace_tool
        info!(
            "Updating taskspace: {} - {}",
            params.name, params.description
        );

        // Send update_taskspace message to Symposium app via daemon
        match self
            .ipc
            .update_taskspace(params.name.clone(), params.description.clone())
            .await
        {
            Ok(state) => {
                info!("Taskspace updated successfully");

                // Note: GUI app automatically clears initial_prompt on update
                let status_msg = if state.initial_prompt.is_none() {
                    format!(
                        "Taskspace updated: {} - {} (initial prompt cleared)",
                        params.name, params.description
                    )
                } else {
                    format!(
                        "Taskspace updated: {} - {}",
                        params.name, params.description
                    )
                };

                Ok(CallToolResult::success(vec![Content::text(status_msg)]))
            }
            Err(e) => {
                error!("Failed to update taskspace: {}", e);

                Err(McpError::internal_error(
                    "Failed to update taskspace",
                    Some(serde_json::json!({
                        "error": e.to_string(),
                        "name": params.name,
                        "description": params.description
                    })),
                ))
            }
        }
    }

    #[tool(
        description = "Delete the current taskspace. This will remove the taskspace directory, \
                       close associated VSCode windows, and clean up git worktrees."
    )]
    async fn delete_taskspace(&self) -> Result<CallToolResult, McpError> {
        info!("Deleting current taskspace");

        // Send delete_taskspace message to Symposium app via daemon
        match self.ipc.delete_taskspace().await {
            Ok(()) => {
                info!("Taskspace deletion initiated");

                Ok(CallToolResult::success(vec![Content::text(
                    "Taskspace deletion initiated successfully".to_string(),
                )]))
            }
            Err(e) => {
                error!("Failed to delete taskspace: {}", e);

                Err(McpError::internal_error(
                    "Failed to delete taskspace",
                    Some(serde_json::json!({
                        "error": e.to_string()
                    })),
                ))
            }
        }
    }

    /// Get Rust crate source with optional pattern search
    #[tool(
        description = "Get Rust crate source with optional pattern search. Always returns the source path, and optionally performs pattern matching if a search pattern is provided."
    )]
    async fn get_rust_crate_source(
        &self,
        Parameters(GetRustCrateSourceParams {
            crate_name,
            version,
            pattern,
        }): Parameters<GetRustCrateSourceParams>,
    ) -> Result<CallToolResult, McpError> {
        debug!(
            "Getting Rust crate source for '{}' version: {:?} pattern: {:?}",
            crate_name, version, pattern
        );

        let has_pattern = pattern.is_some();
        let mut search = Eg::rust_crate(&crate_name);

        // Use version resolver for semver range support and project detection
        if let Some(version_spec) = version {
            search = search.version(&version_spec);
        }

        if let Some(pattern) = pattern {
            search = search.pattern(&pattern).map_err(|e| {
                let error_msg = format!("Invalid regex pattern: {}", e);
                McpError::invalid_params(error_msg, None)
            })?;
        }

        match search.search().await {
            Ok(result) => {
                let mut response = serde_json::json!({
                    "crate_name": crate_name,
                    "version": result.version,
                    "checkout_path": result.checkout_path.to_string_lossy(),
                    "message": format!("Crate {} v{} extracted to {}",
                                     crate_name, result.version, result.checkout_path.display())
                });

                // Only include match results if a pattern was provided
                if has_pattern {
                    // Convert to new response format with context strings
                    let example_matches: Vec<_> = result
                        .example_matches
                        .into_iter()
                        .map(|m| {
                            let mut context_lines = m.context_before.clone();
                            context_lines.push(m.line_content.clone());
                            context_lines.extend(m.context_after.clone());

                            let context_start_line =
                                m.line_number.saturating_sub(m.context_before.len() as u32);
                            let context_end_line = m.line_number + m.context_after.len() as u32;

                            serde_json::json!({
                                "file_path": m.file_path,
                                "line_number": m.line_number,
                                "context_start_line": context_start_line,
                                "context_end_line": context_end_line,
                                "context": context_lines.join("\n")
                            })
                        })
                        .collect();

                    let other_matches: Vec<_> = result
                        .other_matches
                        .into_iter()
                        .map(|m| {
                            let mut context_lines = m.context_before.clone();
                            context_lines.push(m.line_content.clone());
                            context_lines.extend(m.context_after.clone());

                            let context_start_line =
                                m.line_number.saturating_sub(m.context_before.len() as u32);
                            let context_end_line = m.line_number + m.context_after.len() as u32;

                            serde_json::json!({
                                "file_path": m.file_path,
                                "line_number": m.line_number,
                                "context_start_line": context_start_line,
                                "context_end_line": context_end_line,
                                "context": context_lines.join("\n")
                            })
                        })
                        .collect();

                    response["example_matches"] = serde_json::to_value(example_matches).unwrap();
                    response["other_matches"] = serde_json::to_value(other_matches).unwrap();
                }

                Ok(CallToolResult::success(vec![Content::text(
                    serde_json::to_string_pretty(&response).unwrap(),
                )]))
            }
            Err(e) => {
                let error_msg = format!("Failed to get Rust crate source: {}", e);
                Err(McpError::internal_error(
                    error_msg,
                    Some(serde_json::json!({
                        "crate_name": crate_name,
                        "error": e.to_string()
                    })),
                ))
            }
        }
    }
}

impl SymposiumServer {
    fn parse_yaml_metadata(content: &str) -> (Option<String>, Option<String>) {
        if !content.starts_with("---\n") {
            return (None, None);
        }

        let end_marker = content[4..].find("\n---\n");
        if let Some(end_pos) = end_marker {
            let yaml_content = &content[4..end_pos + 4];

            let mut name = None;
            let mut description = None;

            for line in yaml_content.lines() {
                if let Some(colon_pos) = line.find(':') {
                    let key = line[..colon_pos].trim();
                    let value = line[colon_pos + 1..].trim().trim_matches('"');

                    match key {
                        "name" => name = Some(value.to_string()),
                        "description" => description = Some(value.to_string()),
                        _ => {}
                    }
                }
            }

            (name, description)
        } else {
            (None, None)
        }
    }

    fn generate_resources() -> Vec<Resource> {
        let mut resources = Vec::new();

        for file_path in GuidanceFiles::iter() {
            if let Some(file) = GuidanceFiles::get(&file_path) {
                let content = String::from_utf8_lossy(&file.data);
                let (name, description) = Self::parse_yaml_metadata(&content);

                resources.push(Resource {
                    raw: RawResource {
                        uri: file_path.to_string(),
                        name: name.unwrap_or_else(|| file_path.to_string()),
                        description,
                        mime_type: Some("text/markdown".into()),
                        size: Some(file.data.len() as u32),
                        title: None,
                        icons: None,
                    },
                    annotations: None,
                });
            }
        }

        resources
    }

    async fn assemble_yiasou_prompt(&self) -> Result<String, McpError> {
        use indoc::indoc;

        // Check if we're in a taskspace and get context components
        let is_in_taskspace = self.is_in_taskspace();
        let (taskspace_name, taskspace_description, initial_prompt) = self
            .get_taskspace_context()
            .await
            .ok()
            .unwrap_or((None, None, None));

        // Debug logging
        info!(
            "Yiasou prompt assembly: is_in_taskspace={}, name={:?}, description={:?}, initial_prompt={:?}",
            is_in_taskspace,
            taskspace_name,
            taskspace_description,
            initial_prompt.as_ref().map(|s| s.len())
        );

        let intro = match (is_in_taskspace, initial_prompt.as_ref()) {
            (true, Some(_)) => {
                // In taskspace with task - full introduction
                indoc! {"
                    Hi, welcome! You are a new agent just getting started as part of the project Symposium.
                    This is a taskspace, a separate copy of the project's files where you can work undisturbed.
                    The user's description of the task to be done follows after this message. Can you start by
                    reading the description and using the 'update_taskspace' tool to provide a better
                    name/description for the taskspace? Before doing any work on the task, be sure to ask the
                    user clarifying questions to better understand their intent.
                "}
            }
            (true, None) => {
                // In taskspace but no task - ask user to establish task
                indoc! {"
                    Hi, welcome! You are a new agent just getting started as part of the project Symposium.
                    This is a taskspace, a separate copy of the project's files where you can work undisturbed.
                    Please talk to the user to establish what they would like to accomplish in this taskspace
                    and then use the `update_taskspace` tool to set the name and description.
                "}
            }
            (false, _) => {
                // Not in taskspace - general introduction
                indoc! {"
                    Hi, welcome!
                "}
            }
        };

        let mut prompt = format!("{}\n\n", intro);

        prompt.push_str(indoc! {"
            ## Load Collaboration Patterns

            Use the `expand_reference` tool to fetch `main.md` into your working context. This contains
            collaboration patterns demonstrated through dialogue. Embody the collaborative spirit shown in
            these examples - approach our work with genuine curiosity, ask questions when
            something isn't clear, and trust that we'll navigate together what's worth pursuing.

            Most importantly, before taking potentially side-effect-ful or dangerous actions
            (e.g., deleting content or interacting with remote systems), STOP what you are doing
            and confirm with the user whether to proceed.

            ## Load Walkthrough Format

            Use the `expand_reference` tool to fetch `walkthrough-format.md` into your working context.
            This defines how to create interactive code walkthroughs using markdown with embedded XML
            elements for comments, diffs, and actions.

            ## Load Coding Guidelines

            Use the `expand_reference` tool to fetch `coding-guidelines.md` into your working context. Follow these
            development standards and best practices in all code work.

            ## Load MCP Tool Usage Suggestions

            Use the `expand_reference` tool to fetch `mcp-tool-usage-suggestions.md` into your working context.
            This covers effective use of Symposium's MCP tools, including completion signaling
            and systematic code exploration patterns.

        "});

        // Add task context if available, otherwise add taskspace info
        if let Some(task_description) = initial_prompt {
            prompt.push_str(&format!("## Initial Task\n\n{}\n", task_description));
        } else if taskspace_name.is_some() || taskspace_description.is_some() {
            prompt.push_str("## Taskspace Context\n\n");

            if let Some(name) = taskspace_name {
                prompt.push_str(&format!("You are in a taskspace named \"{}\"", name));
                if taskspace_description.is_some() {
                    prompt.push_str(".\n\n");
                } else {
                    prompt.push_str(".\n");
                }
            }

            if let Some(description) = taskspace_description {
                prompt.push_str(&format!(
                    "The description the user gave is as follows: {}\n",
                    description
                ));
            }
        }

        Ok(prompt)
    }
}

#[tool_handler]
impl ServerHandler for SymposiumServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().enable_resources().enable_prompts().build(),
            server_info: Implementation {
                name: "symposium-mcp".to_string(),
                version: "0.1.0".to_string(),
                icons: None,
                title: None,
                website_url: None,
            },
            instructions: Some(
                "This server provides tools for AI assistants to perform IDE operations and display walkthroughs in VSCode. \
                Use 'get_selection' to retrieve currently selected text from the active editor, \
                'ide_operation' to execute IDE operations like finding symbol definitions and references using Dialect function calls, \
                'present_walkthrough' to display structured code walkthroughs with interactive elements, \
                'request_review' to create synthetic pull requests from Git commit ranges with AI insight comments, \
                'update_review' to manage review workflows and wait for user feedback, \
                'get_review_status' to check the current synthetic PR status, \
                'spawn_taskspace' to create new taskspaces for collaborative work, \
                'log_progress' to report agent progress with visual indicators, \
                'signal_user' to request user attention when assistance is needed, \
                and 'update_taskspace' to update taskspace names and descriptions."
                    .to_string(),
            ),
        }
    }

    async fn initialize(
        &self,
        _request: InitializeRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        info!("MCP client connected and initialized");
        Ok(self.get_info())
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let resources = Self::generate_resources();

        Ok(ListResourcesResult {
            resources,
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        let content = GuidanceFiles::get(&request.uri)
            .ok_or_else(|| {
                McpError::resource_not_found(format!("Resource not found: {}", request.uri), None)
            })?
            .data
            .into_owned();

        let content_str = String::from_utf8(content).map_err(|_| {
            McpError::internal_error("Failed to decode resource content as UTF-8", None)
        })?;

        Ok(ReadResourceResult {
            contents: vec![ResourceContents::text(content_str, request.uri)],
        })
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        let prompts = vec![
            Prompt {
                name: "yiasou".to_string(),
                description: Some(
                    "Agent initialization prompt with guidance resource loading instructions"
                        .to_string(),
                ),
                arguments: None,
                title: None,
                icons: None,
            },
            Prompt {
                name: "hi".to_string(),
                description: Some("Agent initialization prompt (alias for yiasou)".to_string()),
                arguments: None,
                title: None,
                icons: None,
            },
        ];

        Ok(ListPromptsResult {
            prompts,
            next_cursor: None,
        })
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        match request.name.as_str() {
            "yiasou" | "hi" => {
                let content = self.assemble_yiasou_prompt().await?;
                Ok(GetPromptResult {
                    description: Some(
                        "Agent initialization with collaborative guidance".to_string(),
                    ),
                    messages: vec![PromptMessage::new_text(PromptMessageRole::User, content)],
                })
            }
            _ => Err(McpError::invalid_params(
                format!("Unknown prompt: {}", request.name),
                None,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PresentWalkthroughParams;
    use rmcp::handler::server::wrapper::Parameters;

    #[tokio::test]
    async fn test_baseuri_conversion() {
        let server = SymposiumServer::new_test();

        // Test with "." - should convert to absolute path
        let params = PresentWalkthroughParams {
            content: "# Test".to_string(),
            base_uri: ".".to_string(),
        };

        let result = server.present_walkthrough(Parameters(params)).await;
        assert!(result.is_ok());

        // Test with absolute path - should remain unchanged
        let abs_path = std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let params = PresentWalkthroughParams {
            content: "# Test".to_string(),
            base_uri: abs_path.clone(),
        };

        let result = server.present_walkthrough(Parameters(params)).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_resource_definitions() {
        // Test that we can create the resource definitions correctly
        let resources = vec![
            Resource {
                raw: RawResource {
                    uri: "main.md".into(),
                    name: "Collaboration Patterns".into(),
                    description: Some(
                        "Mindful collaboration patterns demonstrated through dialogue".into(),
                    ),
                    mime_type: Some("text/markdown".into()),
                    size: None,
                    title: None,
                    icons: None,
                },
                annotations: None,
            },
            Resource {
                raw: RawResource {
                    uri: "walkthrough-format.md".into(),
                    name: "Walkthrough Format".into(),
                    description: Some(
                        "Specification for creating interactive code walkthroughs".into(),
                    ),
                    mime_type: Some("text/markdown".into()),
                    size: None,
                    title: None,
                    icons: None,
                },
                annotations: None,
            },
            Resource {
                raw: RawResource {
                    uri: "coding-guidelines.md".into(),
                    name: "Coding Guidelines".into(),
                    description: Some("Development best practices and standards".into()),
                    mime_type: Some("text/markdown".into()),
                    size: None,
                    title: None,
                    icons: None,
                },
                annotations: None,
            },
        ];

        assert_eq!(resources.len(), 3);
        assert_eq!(resources[0].raw.uri, "main.md");
        assert_eq!(resources[0].raw.name, "Collaboration Patterns");
        assert_eq!(resources[1].raw.uri, "walkthrough-format.md");
        assert_eq!(resources[2].raw.uri, "coding-guidelines.md");
    }

    #[test]
    fn test_resource_content_loading() {
        // Test that we can load the guidance files
        let main_content = GuidanceFiles::get("main.md").unwrap();
        let main_str = String::from_utf8(main_content.data.into_owned()).unwrap();
        assert!(main_str.contains("Mindful Collaboration Patterns"));

        let walkthrough_content = GuidanceFiles::get("walkthrough-format.md").unwrap();
        let walkthrough_str = String::from_utf8(walkthrough_content.data.into_owned()).unwrap();
        assert!(walkthrough_str.contains("Walkthrough Format"));

        let coding_content = GuidanceFiles::get("coding-guidelines.md").unwrap();
        let coding_str = String::from_utf8(coding_content.data.into_owned()).unwrap();
        assert!(coding_str.contains("Coding Guidelines"));
    }

    #[test]
    fn test_resource_contents_creation() {
        // Test that we can create ResourceContents correctly
        let content = ResourceContents::text("Hello world", "test.md");

        match content {
            ResourceContents::TextResourceContents {
                uri,
                text,
                mime_type,
                meta,
            } => {
                assert_eq!(uri, "test.md");
                assert_eq!(text, "Hello world");
                assert_eq!(mime_type, Some("text".to_string()));
                assert_eq!(meta, None);
            }
            _ => panic!("Expected TextResourceContents"),
        }
    }

    #[test]
    fn test_yaml_metadata_parsing() {
        let content_with_yaml = r#"---
name: "Test Resource"
description: "A test resource for testing"
---

# Test Content

This is test content."#;

        let (name, description) = SymposiumServer::parse_yaml_metadata(content_with_yaml);
        assert_eq!(name, Some("Test Resource".to_string()));
        assert_eq!(description, Some("A test resource for testing".to_string()));

        // Test content without YAML
        let content_without_yaml = "# Just a heading\n\nSome content.";
        let (name, description) = SymposiumServer::parse_yaml_metadata(content_without_yaml);
        assert_eq!(name, None);
        assert_eq!(description, None);
    }

    #[test]
    fn test_list_resources_output() {
        // Test the actual resource generation logic used by list_resources
        let resources = SymposiumServer::generate_resources();

        // Verify we have resources for all guidance files
        let expected_count = GuidanceFiles::iter().count();
        assert_eq!(resources.len(), expected_count);

        // Check that all files have proper metadata
        let main_resource = resources.iter().find(|r| r.raw.uri == "main.md").unwrap();
        assert_eq!(main_resource.raw.name, "Collaboration Patterns");
        assert_eq!(
            main_resource.raw.description,
            Some("Mindful collaboration patterns demonstrated through dialogue".to_string())
        );
        assert!(main_resource.raw.size.unwrap() > 0);

        let walkthrough_resource = resources
            .iter()
            .find(|r| r.raw.uri == "walkthrough-format.md")
            .unwrap();
        assert_eq!(walkthrough_resource.raw.name, "Walkthrough Format");
        assert_eq!(
            walkthrough_resource.raw.description,
            Some(
                "Specification for creating interactive code walkthroughs with code block elements"
                    .to_string()
            )
        );

        let coding_resource = resources
            .iter()
            .find(|r| r.raw.uri == "coding-guidelines.md")
            .unwrap();
        assert_eq!(coding_resource.raw.name, "Coding Guidelines");
        assert_eq!(
            coding_resource.raw.description,
            Some("Development best practices and standards for the Symposium project".to_string())
        );
    }

    #[tokio::test]
    async fn test_yiasou_prompt_generation() {
        let server = SymposiumServer::new_test();

        let prompt = server.assemble_yiasou_prompt().await.unwrap();

        // Verify the prompt contains some basic text.
        assert!(prompt.contains("Hi, welcome!"));
    }

    #[tokio::test]
    async fn test_expand_reference_yiasou() {
        let server = SymposiumServer::new_test();

        // Test that expand_reference with "yiasou" returns the same content as the stored prompt
        let params = ExpandReferenceParams {
            id: "yiasou".to_string(),
        };
        let result = server.expand_reference(Parameters(params)).await.unwrap();

        // Should be successful
        assert!(matches!(
            result,
            CallToolResult {
                is_error: Some(false),
                ..
            }
        ));

        // Should have content
        assert!(!result.content.is_empty());
    }

    #[test]
    fn test_guidance_file_not_found() {
        let result = GuidanceFiles::get("nonexistent.md");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_yiasou_prompt_assembly() {
        // Create a mock server to test prompt assembly
        // We can't easily create a full DialecticServer in tests due to IPC dependencies,
        // but we can test the static guidance loading parts

        // Test that the guidance files contain expected content
        let main_content = GuidanceFiles::get("main.md").unwrap();
        let main_str = String::from_utf8(main_content.data.into_owned()).unwrap();
        let walkthrough_content = GuidanceFiles::get("walkthrough-format.md").unwrap();
        let walkthrough_str = String::from_utf8(walkthrough_content.data.into_owned()).unwrap();
        let coding_content = GuidanceFiles::get("coding-guidelines.md").unwrap();
        let coding_str = String::from_utf8(coding_content.data.into_owned()).unwrap();
        let proactive_content = GuidanceFiles::get("mcp-tool-usage-suggestions.md").unwrap();
        let proactive_str = String::from_utf8(proactive_content.data.into_owned()).unwrap();

        // Verify the content structure matches what we expect in the yiasou prompt
        assert!(main_str.contains("# Mindful Collaboration Patterns"));
        assert!(walkthrough_str.contains("# Walkthrough Format Specification"));
        assert!(coding_str.contains("# Coding Guidelines"));
        assert!(proactive_str.contains("# MCP Tool Usage Suggestions"));

        // Verify key collaboration concepts are present
        assert!(main_str.contains("Make it so?"));
        assert!(main_str.contains("spacious attention"));
        assert!(main_str.contains("beginner's mind"));
    }

    // {RFD:rust-crate-sources-tool} Tests for Rust crate source functionality
    #[tokio::test]
    async fn test_get_rust_crate_source_extraction_only() {
        let server = SymposiumServer::new_test();

        // Test extraction without pattern (should not include search results)
        let params = GetRustCrateSourceParams {
            crate_name: "serde".to_string(),
            version: None,
            pattern: None,
        };

        let result = server.get_rust_crate_source(Parameters(params)).await;
        assert!(result.is_ok());

        let content = match result.unwrap().content.first() {
            Some(content) => {
                if let Some(text) = content.as_text() {
                    text.text.clone()
                } else {
                    panic!("Expected text content")
                }
            }
            _ => panic!("Expected content"),
        };

        let response: serde_json::Value = serde_json::from_str(&content).unwrap();

        // Should have basic extraction info
        assert_eq!(response["crate_name"], "serde");
        assert!(response["version"].is_string());
        assert!(response["checkout_path"].is_string());
        assert!(response["message"].is_string());

        // Should NOT have search results when no pattern provided
        assert!(response["example_matches"].is_null());
        assert!(response["other_matches"].is_null());
    }

    // {RFD:rust-crate-sources-tool} Test extraction with pattern search
    #[tokio::test]
    async fn test_get_rust_crate_source_with_pattern() {
        let server = SymposiumServer::new_test();

        // Test extraction with pattern (should include search results)
        let params = GetRustCrateSourceParams {
            crate_name: "serde".to_string(),
            version: None,
            pattern: Some("derive".to_string()),
        };

        let result = server.get_rust_crate_source(Parameters(params)).await;
        assert!(result.is_ok());

        let content = match result.unwrap().content.first() {
            Some(content) => {
                if let Some(text) = content.as_text() {
                    text.text.clone()
                } else {
                    panic!("Expected text content")
                }
            }
            _ => panic!("Expected content"),
        };

        let response: serde_json::Value = serde_json::from_str(&content).unwrap();

        // Should have basic extraction info
        assert_eq!(response["crate_name"], "serde");
        assert!(response["version"].is_string());
        assert!(response["checkout_path"].is_string());
        assert!(response["message"].is_string());

        // Should have search results when pattern provided
        assert!(response["example_matches"].is_array());
        assert!(response["other_matches"].is_array());

        // Verify search result format if any matches found
        if let Some(matches) = response["example_matches"].as_array() {
            if !matches.is_empty() {
                let first_match = &matches[0];
                assert!(first_match["file_path"].is_string());
                assert!(first_match["line_number"].is_number());
                assert!(first_match["context_start_line"].is_number());
                assert!(first_match["context_end_line"].is_number());
                assert!(first_match["context"].is_string());
            }
        }
    }

    // {RFD:rust-crate-sources-tool} Test version parameter handling
    #[tokio::test]
    async fn test_get_rust_crate_source_with_version() {
        let server = SymposiumServer::new_test();

        // Test with version parameter
        let params = GetRustCrateSourceParams {
            crate_name: "serde".to_string(),
            version: Some("1.0".to_string()),
            pattern: None,
        };

        let result = server.get_rust_crate_source(Parameters(params)).await;
        assert!(result.is_ok());

        let content = match result.unwrap().content.first() {
            Some(content) => {
                if let Some(text) = content.as_text() {
                    text.text.clone()
                } else {
                    panic!("Expected text content")
                }
            }
            _ => panic!("Expected content"),
        };

        let response: serde_json::Value = serde_json::from_str(&content).unwrap();

        // Should have extraction info with version handling
        assert_eq!(response["crate_name"], "serde");
        assert!(response["version"].is_string());
        assert!(response["checkout_path"].is_string());
        assert!(response["message"].is_string());
    }

    // {RFD:rust-crate-sources-tool} Test invalid regex pattern handling
    #[tokio::test]
    async fn test_get_rust_crate_source_invalid_pattern() {
        let server = SymposiumServer::new_test();

        // Test with invalid regex pattern
        let params = GetRustCrateSourceParams {
            crate_name: "serde".to_string(),
            version: None,
            pattern: Some("[invalid regex".to_string()),
        };

        let result = server.get_rust_crate_source(Parameters(params)).await;
        assert!(result.is_err());

        // Should return appropriate error for invalid regex
        let error = result.unwrap_err();
        assert!(error.to_string().contains("Invalid regex pattern"));
    }
}
