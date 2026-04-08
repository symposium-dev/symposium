use cargo_agents::hook::{PostToolUsePayload, PreToolUsePayload};

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
            tool_input: serde_json::json!({"command": "cargo agents crate tokio"}),
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
            tool_name: "mcp__cargo_agents__rust".to_string(),
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
            tool_input: serde_json::json!({"command": "cargo agents crate tokio"}),
            tool_response: serde_json::json!({"exit_code": 0}),
            session_id: None,
            cwd: Some("/tmp".to_string()),
        })
        .await;
    assert!(output.hook_specific_output.is_none());
}
