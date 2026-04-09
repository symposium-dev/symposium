use cargo_agents::hook::{PostToolUsePayload, PreToolUsePayload, SessionStartPayload};
use expect_test::expect;

#[tokio::test]
async fn pre_tool_use_builtin_empty() {
    let ctx = cargo_agents_testlib::with_fixture(&["plugins0"]);
    let output = ctx
        .invoke_hook(PreToolUsePayload {
            tool_name: "Bash".to_string(),
        })
        .await;
    assert!(output.hook_specific_output.is_none());
}

#[tokio::test]
async fn post_tool_use_records_bash_activation() {
    let ctx = cargo_agents_testlib::with_fixture(&["plugins0"]);
    let cwd = ctx.sym.config_dir().to_string_lossy().to_string();
    let output = ctx
        .invoke_hook(PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({"command": "symposium crate tokio"}),
            tool_response: serde_json::json!({"stdout": "...", "exit_code": 0}),
            session_id: Some("s1".to_string()),
            cwd: Some(cwd),
        })
        .await;
    assert!(output.hook_specific_output.is_none());
}

#[tokio::test]
async fn post_tool_use_records_mcp_activation() {
    let ctx = cargo_agents_testlib::with_fixture(&["plugins0"]);
    let cwd = ctx.sym.config_dir().to_string_lossy().to_string();
    let output = ctx
        .invoke_hook(PostToolUsePayload {
            tool_name: "mcp__symposium__rust".to_string(),
            tool_input: serde_json::json!({"args": ["crate", "serde"]}),
            tool_response: serde_json::json!({"output": "..."}),
            session_id: Some("s1".to_string()),
            cwd: Some(cwd),
        })
        .await;
    assert!(output.hook_specific_output.is_none());
}

#[tokio::test]
async fn post_tool_use_no_session_returns_empty() {
    let ctx = cargo_agents_testlib::with_fixture(&["plugins0"]);
    let output = ctx
        .invoke_hook(PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({"command": "symposium crate tokio"}),
            tool_response: serde_json::json!({"exit_code": 0}),
            session_id: None,
            cwd: Some("/tmp".to_string()),
        })
        .await;
    assert!(output.hook_specific_output.is_none());
}

#[tokio::test]
async fn session_start_returns_plugin_context() {
    // plugins0 has a session-start.toml with session-start-context
    let ctx = cargo_agents_testlib::with_fixture(&["plugins0"]);
    let output = ctx
        .invoke_hook(SessionStartPayload {
            session_id: Some("s1".to_string()),
            cwd: None,
        })
        .await;

    let context = output
        .hook_specific_output
        .as_ref()
        .and_then(|h| h.additional_context.as_deref())
        .expect("session start should return additional context");

    expect![[r#"**Critical:** Before authoring Rust code, run `symposium start` for instructions."#]]
        .assert_eq(context);
}
