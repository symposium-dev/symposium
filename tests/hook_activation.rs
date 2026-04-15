use symposium::hook::HookEvent;
use symposium::hook_schema::HookAgent;

#[tokio::test]
async fn pre_tool_use_builtin_empty() {
    let ctx = symposium_testlib::with_fixture(&["plugins0"]);
    let output = ctx
        .invoke_hook(
            HookAgent::Claude,
            HookEvent::PreToolUse,
            &serde_json::json!({
                "hook_event_name": "PreToolUse",
                "tool_name": "Bash",
            }),
        )
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(v.get("hookSpecificOutput").is_none());
}

#[tokio::test]
async fn post_tool_use_records_bash_activation() {
    let ctx = symposium_testlib::with_fixture(&["plugins0"]);
    let cwd = ctx.sym.config_dir().to_string_lossy().to_string();
    let output = ctx
        .invoke_hook(
            HookAgent::Claude,
            HookEvent::PostToolUse,
            &serde_json::json!({
                "hook_event_name": "PostToolUse",
                "tool_name": "Bash",
                "tool_input": {"command": "symposium crate tokio"},
                "tool_response": {"stdout": "...", "exit_code": 0},
                "session_id": "s1",
                "cwd": cwd,
            }),
        )
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(v.get("hookSpecificOutput").is_none());
}

#[tokio::test]
async fn post_tool_use_records_mcp_activation() {
    let ctx = symposium_testlib::with_fixture(&["plugins0"]);
    let cwd = ctx.sym.config_dir().to_string_lossy().to_string();
    let output = ctx
        .invoke_hook(
            HookAgent::Claude,
            HookEvent::PostToolUse,
            &serde_json::json!({
                "hook_event_name": "PostToolUse",
                "tool_name": "mcp__symposium__rust",
                "tool_input": {"args": ["crate", "serde"]},
                "tool_response": {"output": "..."},
                "session_id": "s1",
                "cwd": cwd,
            }),
        )
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(v.get("hookSpecificOutput").is_none());
}

#[tokio::test]
async fn post_tool_use_no_session_returns_empty() {
    let ctx = symposium_testlib::with_fixture(&["plugins0"]);
    let output = ctx
        .invoke_hook(
            HookAgent::Claude,
            HookEvent::PostToolUse,
            &serde_json::json!({
                "hook_event_name": "PostToolUse",
                "tool_name": "Bash",
                "tool_input": {"command": "symposium crate tokio"},
                "tool_response": {"exit_code": 0},
                "cwd": "/tmp",
            }),
        )
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(v.get("hookSpecificOutput").is_none());
}

#[tokio::test]
async fn session_start_returns_plugin_context() {
    let ctx = symposium_testlib::with_fixture(&["plugins0"]);
    let output = ctx
        .invoke_hook(
            HookAgent::Claude,
            HookEvent::SessionStart,
            &serde_json::json!({
                "hook_event_name": "SessionStart",
                "session_id": "s1",
            }),
        )
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let context = v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("session start should return additional context");

    expect_test::expect![[
        r#"**Critical:** Before authoring Rust code, run `symposium start` for instructions."#
    ]]
    .assert_eq(context);
}
