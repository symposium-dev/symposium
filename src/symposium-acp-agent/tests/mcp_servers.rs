use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tempfile::TempDir;

use sacp::link::ClientToAgent;
use sacp::on_receive_notification;
use sacp::schema::{
    ContentBlock, ContentChunk, InitializeRequest, NewSessionRequest, PromptRequest,
    ProtocolVersion, SessionNotification, SessionUpdate, StopReason, TextContent,
};

use symposium_acp_agent::ConfigAgent;
use symposium_acp_agent::recommendations::Recommendations;
use symposium_acp_agent::recommendations::When;
use symposium_acp_agent::user_config::ModConfig;
use symposium_acp_agent::user_config::{ConfigPaths, GlobalAgentConfig, WorkspaceModsConfig};
use symposium_recommendations::{ComponentSource, LocalDistribution};
use symposium_recommendations::{ModKind, Recommendation};

#[derive(Debug, Default, Clone)]
struct CollectedNotifications {
    messages: Vec<String>,
}

impl CollectedNotifications {
    fn text(&self) -> String {
        self.messages.join("")
    }

    fn clear(&mut self) {
        self.messages.clear();
    }
}

/// Create a config that uses elizacp as the backend agent.
/// Uses the external elizacp binary (must be installed via `cargo install elizacp`).
fn elizacp_agent() -> ComponentSource {
    ComponentSource::Local(LocalDistribution {
        name: None,
        command: "elizacp".to_string(),
        args: vec!["--deterministic".to_string(), "acp".to_string()],
        env: BTreeMap::new(),
    })
}

/// Initialize tracing for tests. Call at the start of tests that need logging.
/// Set RUST_LOG=trace (or debug, info, etc.) to see output.
#[allow(dead_code)]
fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::DEBUG.into()),
        )
        .with_test_writer()
        .with_ansi(false)
        .try_init();
}

#[tokio::test]
async fn test_mcp_server_injected_and_used() -> Result<(), sacp::Error> {
    init_tracing();

    let config_temp_dir = TempDir::new().unwrap();
    let config_paths = ConfigPaths::with_root(config_temp_dir.path());

    let workspace_path = PathBuf::from("/fake/workspace");
    let mcp_server_bin = PathBuf::from(env!("CARGO_BIN_EXE_mcp-test-server"));
    tracing::debug!(?mcp_server_bin);

    let default_agent = elizacp_agent();
    GlobalAgentConfig::new(default_agent)
        .save(&config_paths)
        .await
        .unwrap();

    let source = ComponentSource::Local(LocalDistribution {
        name: Some("mcp-test-server".to_string()),
        command: mcp_server_bin.to_string_lossy().to_string(),
        args: Vec::new(),
        env: BTreeMap::new(),
    });
    let mut mods_config = WorkspaceModsConfig::new(vec![]);
    mods_config.mods.push(crate::ModConfig {
        kind: ModKind::MCP,
        source: source.clone(),
        enabled: true,
        when: When::default(),
    });

    mods_config
        .save(&config_paths, &workspace_path)
        .await
        .unwrap();

    let notifications = Arc::new(Mutex::new(CollectedNotifications::default()));
    let notifications_clone = notifications.clone();

    let agent =
        ConfigAgent::with_config_paths(config_paths).with_recommendations(Recommendations {
            mods: vec![Recommendation {
                kind: ModKind::MCP,
                source,
                when: None,
            }],
        });

    ClientToAgent::builder()
        .name("test_client")
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
            tracing::debug!("initialize");

            cx.send_request(InitializeRequest::new(ProtocolVersion::LATEST))
                .block_task()
                .await?;

            let session_response = cx
                .send_request(NewSessionRequest::new(&workspace_path))
                .block_task()
                .await?;
            let session_id = session_response.session_id;

            // Give the async notification time to arrive
            tokio::time::sleep(Duration::from_millis(50)).await;

            assert_eq!("", notifications.lock().unwrap().text());

            tracing::debug!("pre-prompt");

            notifications.lock().unwrap().clear();
            let prompt_response = cx
                .send_request(PromptRequest::new(
                    session_id.clone(),
                    vec![ContentBlock::Text(TextContent::new(
                        "list tools from mcp-test-server",
                    ))],
                ))
                .block_task()
                .await?;
            assert_eq!(prompt_response.stop_reason, StopReason::EndTurn);
            tokio::time::sleep(Duration::from_millis(200)).await;

            let text = notifications.lock().unwrap().text();
            assert!(
                text.contains("echo"),
                "Expected echo tool to be listed, got: {text}"
            );

            notifications.lock().unwrap().clear();
            cx.send_request(PromptRequest::new(
                session_id,
                vec![ContentBlock::Text(TextContent::new(
                    r#"use tool mcp-test-server::echo with {"text": "hello"}"#,
                ))],
            ))
            .block_task()
            .await?;
            tokio::time::sleep(Duration::from_millis(200)).await;

            let text = notifications.lock().unwrap().text();
            assert!(
                text.contains("hello"),
                "Expected tool result to include echoed text, got: {text}"
            );

            Ok(())
        })
        .await
}
