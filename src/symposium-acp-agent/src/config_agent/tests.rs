//! Tests for the ConfigAgent.

use super::*;
use crate::recommendations::{Recommendation, Recommendations};
use crate::registry::{ComponentSource, LocalDistribution};
use crate::user_config::{ConfigPaths, GlobalAgentConfig, WorkspaceModsConfig};
use sacp::link::ClientToAgent;
use sacp::on_receive_notification;
use sacp::schema::{ContentChunk, ProtocolVersion, TextContent};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::TempDir;

/// Initialize tracing for tests. Call at the start of tests that need logging.
/// Set RUST_LOG=trace (or debug, info, etc.) to see output.
#[allow(dead_code)]
fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_test_writer()
        .try_init();
}

/// Collected notifications from sessions.
#[derive(Debug, Default, Clone)]
struct CollectedNotifications {
    messages: Vec<String>,
    commands: Vec<Vec<AvailableCommand>>,
}

impl CollectedNotifications {
    /// Get all text messages concatenated.
    fn text(&self) -> String {
        self.messages.join("")
    }

    fn clear(&mut self) {
        self.messages.clear();
        self.commands.clear();
    }
}

/// Helper to write workspace config using the given ConfigPaths.
/// Now writes both agent (global) and mods (per-workspace).
fn write_workspace_config(
    config_paths: &ConfigPaths,
    workspace_path: &std::path::Path,
    agent: &ComponentSource,
    mods: &WorkspaceModsConfig,
) {
    GlobalAgentConfig::new(agent.clone())
        .save(config_paths)
        .unwrap();
    mods.save(config_paths, workspace_path).unwrap();
}

/// Create a config that uses elizacp as the backend agent.
/// Uses the external elizacp binary (must be installed via `cargo install elizacp`).
fn elizacp_agent() -> ComponentSource {
    ComponentSource::Local(LocalDistribution {
        command: "elizacp".to_string(),
        args: vec!["--deterministic".to_string(), "acp".to_string()],
        env: BTreeMap::new(),
    })
}

/// Create empty mods config for testing.
fn empty_mods() -> WorkspaceModsConfig {
    WorkspaceModsConfig::new(vec![])
}

/// Create test recommendations for testing initial setup flow.
fn test_recommendations() -> Recommendations {
    Recommendations {
        mods: vec![Recommendation {
            source: ComponentSource::Builtin("ferris".to_string()),
            when: None, // Always recommended
        }],
    }
}

/// Default test agent for initial setup testing.
fn test_default_agent() -> ComponentSource {
    ComponentSource::Builtin("eliza".to_string())
}

// ============================================================================
// Basic flow tests
// ============================================================================

/// Test that when no config exists, we get the initial setup flow using recommendations.
#[tokio::test]
#[ignore = "https://github.com/symposium-dev/symposium/issues/113"]
async fn test_no_config_initial_setup() -> Result<(), sacp::Error> {
    // Use a temp dir for ConfigPaths (isolates from real ~/.symposium)
    let config_temp_dir = TempDir::new().unwrap();
    let config_paths = ConfigPaths::with_root(config_temp_dir.path());

    // Use a fake workspace path (doesn't need to exist on disk for this test)
    let workspace_path = PathBuf::from("/fake/workspace");
    // Don't create the workspace config file - we want to test the "no config" path

    // Pre-populate the global agent config so we skip agent selection
    let default_agent = test_default_agent();
    let global_config = crate::user_config::GlobalAgentConfig::new(default_agent.clone());
    global_config.save(&config_paths).unwrap();

    let notifications = Arc::new(Mutex::new(CollectedNotifications::default()));
    let notifications_clone = notifications.clone();

    // Use test recommendations
    let recommendations = test_recommendations();

    let agent =
        ConfigAgent::with_config_paths(config_paths.clone()).with_recommendations(recommendations);

    ClientToAgent::builder()
        .on_receive_notification(
            async move |notif: SessionNotification, _cx| {
                if let SessionUpdate::AgentMessageChunk(ContentChunk { content, .. }) = notif.update
                {
                    if let ContentBlock::Text(TextContent { text, .. }) = content {
                        notifications_clone.lock().unwrap().messages.push(text);
                    }
                }
                Ok(())
            },
            on_receive_notification!(),
        )
        .connect_to(agent)?
        .run_until(async |cx| {
            // Initialize
            let init_response = cx
                .send_request(InitializeRequest::new(ProtocolVersion::LATEST))
                .block_task()
                .await?;
            assert_eq!(init_response.protocol_version, ProtocolVersion::LATEST);

            // Request a new session - should trigger initial setup
            let session_response = cx
                .send_request(NewSessionRequest::new(&workspace_path))
                .block_task()
                .await?;
            let session_id = session_response.session_id;

            // Give the async notification time to arrive
            tokio::time::sleep(Duration::from_millis(50)).await;

            // Should have received the welcome message and config menu
            let text = notifications.lock().unwrap().text();
            assert!(
                text.contains("Welcome to Symposium!"),
                "Expected welcome message, got: {}",
                text
            );
            assert!(
                text.contains("Using your default agent"),
                "Expected default agent message, got: {}",
                text
            );
            assert!(
                text.contains("Configuration created with recommended mods"),
                "Expected config creation message, got: {}",
                text
            );
            assert!(
                text.contains("# Configuration"),
                "Expected config menu, got: {}",
                text
            );
            assert!(
                text.contains("eliza"),
                "Expected eliza agent in config, got: {}",
                text
            );
            assert!(
                text.contains("ferris"),
                "Expected ferris mod, got: {}",
                text
            );

            // Save the configuration
            notifications.lock().unwrap().clear();
            let prompt_response = cx
                .send_request(PromptRequest::new(
                    session_id,
                    vec![ContentBlock::Text(TextContent::new("save"))],
                ))
                .block_task()
                .await?;
            assert_eq!(prompt_response.stop_reason, StopReason::EndTurn);

            // Give the async notification time to arrive
            tokio::time::sleep(Duration::from_millis(50)).await;

            // Should have saved message
            let text = notifications.lock().unwrap().text();
            assert!(
                text.contains("Configuration saved"),
                "Expected save confirmation"
            );

            // Give time for the config to be written
            tokio::time::sleep(Duration::from_millis(250)).await;

            // Verify config was written
            let loaded_agent = GlobalAgentConfig::load(&config_paths).unwrap();
            assert!(loaded_agent.is_some(), "Agent config should have been saved");
            let loaded_mods =
                WorkspaceModsConfig::load(&config_paths, &workspace_path).unwrap();
            assert!(
                loaded_mods.is_some(),
                "Mods config should have been saved"
            );

            Ok(())
        })
        .await
}

/// Test new session flow with valid config.
/// This requires elizacp to be available.
#[tokio::test]
async fn test_new_session_with_config() -> Result<(), sacp::Error> {
    init_tracing();

    // Use a temp dir for ConfigPaths (isolates from real ~/.symposium)
    let config_temp_dir = TempDir::new().unwrap();
    let config_paths = ConfigPaths::with_root(config_temp_dir.path());

    // Use a fake workspace path
    let workspace_path = PathBuf::from("/fake/workspace");
    let agent_source = elizacp_agent();
    let extensions = empty_mods();
    write_workspace_config(&config_paths, &workspace_path, &agent_source, &extensions);

    let notifications = Arc::new(Mutex::new(CollectedNotifications::default()));
    let notifications_clone = notifications.clone();

    // Use empty recommendations to avoid triggering the diff prompt
    let config_agent =
        ConfigAgent::with_config_paths(config_paths).with_recommendations(Recommendations::empty());

    ClientToAgent::builder()
        .on_receive_notification(
            async move |notif: SessionNotification, _cx| {
                match notif.update {
                    SessionUpdate::AgentMessageChunk(ContentChunk { content, .. }) => {
                        if let ContentBlock::Text(TextContent { text, .. }) = content {
                            notifications_clone.lock().unwrap().messages.push(text);
                        }
                    }
                    SessionUpdate::AvailableCommandsUpdate(update) => {
                        notifications_clone
                            .lock()
                            .unwrap()
                            .commands
                            .push(update.available_commands);
                    }
                    _ => {}
                }
                Ok(())
            },
            on_receive_notification!(),
        )
        .connect_to(config_agent)?
        .run_until(async |cx| {
            // Initialize
            cx.send_request(InitializeRequest::new(ProtocolVersion::LATEST))
                .block_task()
                .await?;

            // Request a new session - should delegate to conductor with elizacp
            let session_response = cx
                .send_request(NewSessionRequest::new(&workspace_path))
                .block_task()
                .await?;
            let session_id = session_response.session_id;

            // Send a prompt - elizacp should respond
            notifications.lock().unwrap().clear();
            cx.send_request(PromptRequest::new(
                session_id.clone(),
                vec![ContentBlock::Text(TextContent::new("Hello, how are you?"))],
            ))
            .block_task()
            .await?;

            // Give the async notification time to arrive
            tokio::time::sleep(Duration::from_millis(100)).await;

            // Elizacp should have responded
            let text = notifications.lock().unwrap().text();
            assert!(
                !text.is_empty(),
                "Expected elizacp to respond, got empty response"
            );

            // Check that /symposium:config command was sent
            let commands = notifications.lock().unwrap().commands.clone();
            let has_config_command = commands
                .iter()
                .any(|cmds| cmds.iter().any(|cmd| cmd.name == "symposium:config"));
            assert!(
                has_config_command,
                "Expected /symposium:config command to be available"
            );

            Ok(())
        })
        .await
}

/// Test that /symposium:config enters config mode.
#[tokio::test]
async fn test_config_mode_entry() -> Result<(), sacp::Error> {
    init_tracing();

    // Use a temp dir for ConfigPaths (isolates from real ~/.symposium)
    let config_temp_dir = TempDir::new().unwrap();
    let config_paths = ConfigPaths::with_root(config_temp_dir.path());

    // Use a fake workspace path
    let workspace_path = PathBuf::from("/fake/workspace");
    let agent_source = elizacp_agent();
    let extensions = empty_mods();
    write_workspace_config(&config_paths, &workspace_path, &agent_source, &extensions);

    let notifications = Arc::new(Mutex::new(CollectedNotifications::default()));
    let notifications_clone = notifications.clone();

    // Use empty recommendations to avoid triggering the diff prompt
    let config_agent =
        ConfigAgent::with_config_paths(config_paths).with_recommendations(Recommendations::empty());

    ClientToAgent::builder()
        .on_receive_notification(
            async move |notif: SessionNotification, _cx| {
                if let SessionUpdate::AgentMessageChunk(ContentChunk { content, .. }) = notif.update
                {
                    if let ContentBlock::Text(TextContent { text, .. }) = content {
                        notifications_clone.lock().unwrap().messages.push(text);
                    }
                }
                Ok(())
            },
            on_receive_notification!(),
        )
        .connect_to(config_agent)?
        .run_until(async |cx| {
            // Initialize
            cx.send_request(InitializeRequest::new(ProtocolVersion::LATEST))
                .block_task()
                .await?;

            // Create a session
            let session_response = cx
                .send_request(NewSessionRequest::new(&workspace_path))
                .block_task()
                .await?;
            let session_id = session_response.session_id;

            // Clear notifications and send the config command
            notifications.lock().unwrap().clear();
            cx.send_request(PromptRequest::new(
                session_id.clone(),
                vec![ContentBlock::Text(TextContent::new("/symposium:config"))],
            ))
            .block_task()
            .await?;

            // Give the async notification time to arrive
            tokio::time::sleep(Duration::from_millis(100)).await;

            // Should have received config mode welcome with menu
            let text = notifications.lock().unwrap().text();
            assert!(text.contains("# Configuration"), "Expected config menu");
            assert!(
                text.contains("Mods for workspace"),
                "Expected workspace path in mods header"
            );
            assert!(text.contains("eliza"), "Expected eliza agent");
            assert!(text.contains("(none configured)"), "Expected no mods");
            assert!(text.contains("SAVE"), "Expected save command");
            assert!(text.contains("CANCEL"), "Expected cancel command");

            Ok(())
        })
        .await
}
