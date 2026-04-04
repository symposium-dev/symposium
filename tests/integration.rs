mod testlib;

use symposium::hook::{
    HookPayload, HookSubPayload, PostToolUsePayload, PreToolUsePayload, UserPromptSubmitPayload,
};

#[tokio::test]
async fn dispatch_help() {
    let ctx = testlib::with_fixture(&["plugins0"]);
    let output = ctx.invoke(&["help"]).await.unwrap();
    assert!(output.contains("Symposium"));
    assert!(output.contains("start"));
    assert!(output.contains("crate"));
}

#[tokio::test]
async fn dispatch_unknown_command() {
    let ctx = testlib::with_fixture(&["plugins0"]);
    let result = ctx.invoke(&["nonsense"]).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Unknown command"));
}

#[tokio::test]
async fn dispatch_start_includes_tutorial() {
    let ctx = testlib::with_fixture(&["plugins0"]);
    let output = ctx.invoke(&["start"]).await.unwrap();
    // The start output includes the tutorial content
    assert!(!output.is_empty());
}

#[tokio::test]
async fn dispatch_crate_list_with_plugins() {
    let ctx = testlib::with_fixture(&["plugins0"]);
    // Without a workspace, the crate list should still work (empty workspace)
    let output = ctx.invoke(&["crate", "--list"]).await.unwrap();
    // May say "No skills available" since no workspace deps match
    assert!(!output.is_empty());
}

#[tokio::test]
async fn hook_pre_tool_use_builtin_empty() {
    let ctx = testlib::with_fixture(&["plugins0"]);
    let payload = HookPayload {
        sub_payload: HookSubPayload::PreToolUse(PreToolUsePayload {
            tool_name: "Bash".to_string(),
        }),
        rest: serde_json::Map::new(),
    };
    let output = ctx.invoke_hook(&payload).await;
    assert!(output.hook_specific_output.is_none());
}

#[tokio::test]
async fn hook_post_tool_use_builtin_empty_for_now() {
    let ctx = testlib::with_fixture(&["plugins0"]);
    let payload = HookPayload {
        sub_payload: HookSubPayload::PostToolUse(PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({"command": "symposium crate tokio"}),
            tool_response: serde_json::json!({"stdout": "...", "exit_code": 0}),
            session_id: Some("s1".to_string()),
            cwd: Some("/tmp".to_string()),
        }),
        rest: serde_json::Map::new(),
    };
    let output = ctx.invoke_hook(&payload).await;
    assert!(output.hook_specific_output.is_none());
}

#[tokio::test]
async fn hook_user_prompt_submit_builtin_empty_for_now() {
    let ctx = testlib::with_fixture(&["plugins0"]);
    let payload = HookPayload {
        sub_payload: HookSubPayload::UserPromptSubmit(UserPromptSubmitPayload {
            prompt: "I need to use `tokio`".to_string(),
            session_id: Some("s1".to_string()),
            cwd: Some("/tmp".to_string()),
        }),
        rest: serde_json::Map::new(),
    };
    let output = ctx.invoke_hook(&payload).await;
    assert!(output.hook_specific_output.is_none());
}
