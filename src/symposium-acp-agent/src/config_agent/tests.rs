//! Tests for the ConfigAgent.

use super::*;
use expect_test::expect;
use sacp::link::ClientToAgent;
use sacp::on_receive_notification;
use sacp::schema::{ContentChunk, ProtocolVersion, TextContent};
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

/// Helper to write a test config file to a temp directory.
fn write_config(dir: &TempDir, config: &SymposiumUserConfig) -> PathBuf {
    let config_path = dir.path().join("config.jsonc");
    config.save_to(&config_path).unwrap();
    config_path
}

/// Create a config that uses elizacp as the backend.
fn elizacp_config() -> SymposiumUserConfig {
    SymposiumUserConfig {
        // elizacp needs 'acp' subcommand to run as ACP agent
        // --deterministic is a global flag that goes before the subcommand
        agent: "elizacp --deterministic acp".to_string(),
        proxies: vec![], // No proxies for simpler testing
    }
}

// ============================================================================
// Basic flow tests
// ============================================================================

/// Test that when no config exists, we get the initial setup flow with agent selection.
#[tokio::test]
async fn test_no_config_initial_setup() -> Result<(), sacp::Error> {
    use crate::registry::AgentListEntry;

    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.jsonc");
    // Don't create the file - we want to test the "no config" path

    let notifications = Arc::new(Mutex::new(CollectedNotifications::default()));
    let notifications_clone = notifications.clone();

    // Inject test agents so we don't hit the registry
    let test_agents = vec![
        AgentListEntry {
            id: "claude-code".to_string(),
            name: "Claude Code".to_string(),
            description: Some("AI coding assistant".to_string()),
            version: None,
        },
        AgentListEntry {
            id: "gemini".to_string(),
            name: "Gemini CLI".to_string(),
            description: Some("Google's AI".to_string()),
            version: None,
        },
    ];

    let agent = ConfigAgent::new()
        .with_config_path(&config_path)
        .with_injected_agents(test_agents);

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

            // Request a new session - should trigger initial setup (config mode with agent selection)
            let session_response = cx
                .send_request(NewSessionRequest::new("."))
                .block_task()
                .await?;
            let session_id = session_response.session_id;

            // Give the async notification time to arrive
            tokio::time::sleep(Duration::from_millis(50)).await;

            // Should have received the welcome message and agent selection menu
            let text = notifications.lock().unwrap().text();
            expect![[r#"
                Welcome to Symposium!

                No configuration found. Let's set up your AI agent.
                # Select Agent

                | # | Agent | Description |
                |---|-------|-------------|
                | 1 | Claude Code | AI coding assistant |
                | 2 | Gemini CLI | Google's AI |

                Enter a number to select, or `back` to return.
            "#]]
            .assert_eq(&text);

            // Select an agent (Claude Code = 1, using 1-based indexing)
            notifications.lock().unwrap().clear();
            let prompt_response = cx
                .send_request(PromptRequest::new(
                    session_id,
                    vec![ContentBlock::Text(TextContent::new("1"))],
                ))
                .block_task()
                .await?;
            assert_eq!(prompt_response.stop_reason, StopReason::EndTurn);

            // Give the async notification time to arrive
            tokio::time::sleep(Duration::from_millis(50)).await;

            // Should now show the main config menu with the selected agent
            let text = notifications.lock().unwrap().text();
            assert!(
                text.contains("Agent set to"),
                "Expected agent selection confirmation"
            );
            assert!(
                text.contains("# Configuration"),
                "Expected main menu after selection"
            );

            Ok(())
        })
        .await
}

/// Test new session flow with valid config.
/// This requires elizacp to be available, so we need to think about how to wire it up.
#[tokio::test]
async fn test_new_session_with_config() -> Result<(), sacp::Error> {
    init_tracing();

    let temp_dir = TempDir::new().unwrap();
    let config = elizacp_config();
    let config_path = write_config(&temp_dir, &config);

    let notifications = Arc::new(Mutex::new(CollectedNotifications::default()));
    let notifications_clone = notifications.clone();

    let agent = ConfigAgent::new().with_config_path(&config_path);

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
        .connect_to(agent)?
        .run_until(async |cx| {
            // Initialize
            cx.send_request(InitializeRequest::new(ProtocolVersion::LATEST))
                .block_task()
                .await?;

            // Request a new session - should delegate to conductor with elizacp
            let session_response = cx
                .send_request(NewSessionRequest::new("."))
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

    let temp_dir = TempDir::new().unwrap();
    let config = elizacp_config();
    let config_path = write_config(&temp_dir, &config);

    let notifications = Arc::new(Mutex::new(CollectedNotifications::default()));
    let notifications_clone = notifications.clone();

    // Use injected agents to bypass registry fetch
    let agent = ConfigAgent::new()
        .with_config_path(&config_path)
        .with_injected_agents(vec![]); // Empty list - no agents from registry

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
            cx.send_request(InitializeRequest::new(ProtocolVersion::LATEST))
                .block_task()
                .await?;

            // Create a session
            let session_response = cx
                .send_request(NewSessionRequest::new("."))
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
            expect![[r#"
                # Configuration

                * **Agent:** elizacp --deterministic acp
                * **Extensions:**
                    * (none configured)

                # Commands

                - `A` or `AGENT` - Select a different agent
                - `save` - Save for future sessions
                - `cancel` - Exit without saving
            "#]]
            .assert_eq(&text);

            Ok(())
        })
        .await
}
