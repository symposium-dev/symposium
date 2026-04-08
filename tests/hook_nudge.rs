use expect_test::expect;
use cargo_agents::hook::{PostToolUsePayload, UserPromptSubmitPayload};

#[tokio::test]
async fn nudges_about_available_skill() {
    // plugins0 has a standalone serde skill; workspace0 has serde as a dep.
    // The nudge fires because serde is both in the workspace and has a matching skill.
    let ctx = cargo_agents_testlib::with_fixture(&["plugins0", "workspace0"]);
    let cwd = ctx
        .workspace_root
        .as_ref()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let output = ctx
        .invoke_hook(UserPromptSubmitPayload {
            prompt: "I need to use `serde`".to_string(),
            session_id: Some("s1".to_string()),
            cwd: Some(cwd),
        })
        .await;
    let ctx_text = output
        .hook_specific_output
        .as_ref()
        .and_then(|h| h.additional_context.as_deref())
        .unwrap_or("");
    assert!(
        ctx_text.contains("serde"),
        "nudge should mention serde: {ctx_text}"
    );
    expect![[r#"
        The `serde` crate has specialized guidance available.
        To load it, run: `cargo agents crate serde`
    "#]]
    .assert_eq(&format!("{ctx_text}\n"));
}

#[tokio::test]
async fn activation_suppresses_nudge() {
    // After activating a crate via post-tool-use, a subsequent prompt mention
    // should NOT nudge about that crate.
    let ctx = cargo_agents_testlib::with_fixture(&["plugins0", "workspace0"]);
    let cwd = ctx
        .workspace_root
        .as_ref()
        .unwrap()
        .to_string_lossy()
        .to_string();

    // First: record activation via PostToolUse
    ctx.invoke_hook(PostToolUsePayload {
        tool_name: "Bash".to_string(),
        tool_input: serde_json::json!({"command": "cargo agents crate serde"}),
        tool_response: serde_json::json!({"exit_code": 0}),
        session_id: Some("s1".to_string()),
        cwd: Some(cwd.clone()),
    })
    .await;

    // Second: mention serde in a prompt — should not nudge since already activated
    let output = ctx
        .invoke_hook(UserPromptSubmitPayload {
            prompt: "I need to use `serde` for serialization".to_string(),
            session_id: Some("s1".to_string()),
            cwd: Some(cwd),
        })
        .await;
    assert!(output.hook_specific_output.is_none());
}
