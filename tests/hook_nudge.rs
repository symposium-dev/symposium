use expect_test::expect;
use symposium::hook::HookEvent;
use symposium::hook_schema::HookAgent;

#[tokio::test]
async fn nudges_about_available_skill() {
    let ctx = symposium_testlib::with_fixture(&["plugins0", "workspace0"]);
    let cwd = ctx
        .workspace_root
        .as_ref()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let output = ctx
        .invoke_hook(
            HookAgent::Claude,
            HookEvent::UserPromptSubmit,
            &serde_json::json!({
                "hook_event_name": "UserPromptSubmit",
                "prompt": "I need to use `serde`",
                "session_id": "s1",
                "cwd": cwd,
            }),
        )
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let ctx_text = v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap_or("");
    assert!(
        ctx_text.contains("serde"),
        "nudge should mention serde: {ctx_text}"
    );
    expect![[r#"
        The `serde` crate has specialized guidance available.
        To load it, run: `symposium crate serde`
    "#]]
    .assert_eq(&format!("{ctx_text}\n"));
}

#[tokio::test]
async fn activation_suppresses_nudge() {
    let ctx = symposium_testlib::with_fixture(&["plugins0", "workspace0"]);
    let cwd = ctx
        .workspace_root
        .as_ref()
        .unwrap()
        .to_string_lossy()
        .to_string();

    // First: record activation via PostToolUse
    ctx.invoke_hook(
        HookAgent::Claude,
        HookEvent::PostToolUse,
        &serde_json::json!({
            "hook_event_name": "PostToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": "symposium crate serde"},
            "tool_response": {"exit_code": 0},
            "session_id": "s1",
            "cwd": cwd,
        }),
    )
    .await
    .unwrap();

    // Second: mention serde in a prompt — should not nudge since already activated
    let output = ctx
        .invoke_hook(
            HookAgent::Claude,
            HookEvent::UserPromptSubmit,
            &serde_json::json!({
                "hook_event_name": "UserPromptSubmit",
                "prompt": "I need to use `serde` for serialization",
                "session_id": "s1",
                "cwd": cwd,
            }),
        )
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(v.get("hookSpecificOutput").is_none());
}
