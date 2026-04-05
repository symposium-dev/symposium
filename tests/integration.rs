mod testlib;

use symposium::hook::{
    HookPayload, HookSubPayload, PostToolUsePayload, PreToolUsePayload, UserPromptSubmitPayload,
};

#[tokio::test]
async fn dispatch_help() {
    let ctx = testlib::with_fixture(&["plugins0"]);
    // Clap handles "help" as a built-in, returning a parse error with help text.
    let result = ctx.invoke(&["help"]).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("start"), "help should mention 'start': {err}");
    assert!(err.contains("crate"), "help should mention 'crate': {err}");
}

#[tokio::test]
async fn dispatch_unknown_command() {
    let ctx = testlib::with_fixture(&["plugins0"]);
    let result = ctx.invoke(&["nonsense"]).await;
    assert!(result.is_err());
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

// --- PostToolUse activation recording tests ---

#[tokio::test]
async fn hook_post_tool_use_records_bash_activation() {
    let ctx = testlib::with_fixture(&["plugins0"]);
    let cwd = ctx.sym.config_dir().to_string_lossy().to_string();
    let payload = HookPayload {
        sub_payload: HookSubPayload::PostToolUse(PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({"command": "symposium crate tokio"}),
            tool_response: serde_json::json!({"stdout": "...", "exit_code": 0}),
            session_id: Some("s1".to_string()),
            cwd: Some(cwd),
        }),
        rest: serde_json::Map::new(),
    };
    // PostToolUse records the activation but returns empty output
    let output = ctx.invoke_hook(&payload).await;
    assert!(output.hook_specific_output.is_none());
}

#[tokio::test]
async fn hook_post_tool_use_records_mcp_activation() {
    let ctx = testlib::with_fixture(&["plugins0"]);
    let cwd = ctx.sym.config_dir().to_string_lossy().to_string();
    let payload = HookPayload {
        sub_payload: HookSubPayload::PostToolUse(PostToolUsePayload {
            tool_name: "mcp__symposium__rust".to_string(),
            tool_input: serde_json::json!({"args": ["crate", "serde"]}),
            tool_response: serde_json::json!({"output": "..."}),
            session_id: Some("s1".to_string()),
            cwd: Some(cwd),
        }),
        rest: serde_json::Map::new(),
    };
    let output = ctx.invoke_hook(&payload).await;
    assert!(output.hook_specific_output.is_none());
}

// --- UserPromptSubmit nudge tests ---

#[tokio::test]
async fn hook_user_prompt_submit_nudges_about_available_skill() {
    // plugins0 has a standalone serde skill (activation: always), so mentioning
    // serde in a prompt should trigger a nudge even without workspace deps.
    let ctx = testlib::with_fixture(&["plugins0"]);
    let cwd = ctx.sym.config_dir().to_string_lossy().to_string();
    let payload = HookPayload {
        sub_payload: HookSubPayload::UserPromptSubmit(UserPromptSubmitPayload {
            prompt: "I need to use `serde`".to_string(),
            session_id: Some("s1".to_string()),
            cwd: Some(cwd),
        }),
        rest: serde_json::Map::new(),
    };
    let output = ctx.invoke_hook(&payload).await;
    let ctx_text = output
        .hook_specific_output
        .as_ref()
        .and_then(|h| h.additional_context.as_deref())
        .unwrap_or("");
    assert!(
        ctx_text.contains("serde"),
        "nudge should mention serde: {ctx_text}"
    );
}

#[tokio::test]
async fn hook_post_tool_use_no_session_returns_empty() {
    let ctx = testlib::with_fixture(&["plugins0"]);
    let payload = HookPayload {
        sub_payload: HookSubPayload::PostToolUse(PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({"command": "symposium crate tokio"}),
            tool_response: serde_json::json!({"exit_code": 0}),
            session_id: None, // no session
            cwd: Some("/tmp".to_string()),
        }),
        rest: serde_json::Map::new(),
    };
    let output = ctx.invoke_hook(&payload).await;
    assert!(output.hook_specific_output.is_none());
}

#[tokio::test]
async fn hook_activation_then_no_nudge() {
    // After activating a crate via post-tool-use, a subsequent prompt mention
    // should NOT nudge about that crate.
    let ctx = testlib::with_fixture(&["plugins0"]);
    let cwd = ctx.sym.config_dir().to_string_lossy().to_string();

    // First: record activation via PostToolUse
    let activate = HookPayload {
        sub_payload: HookSubPayload::PostToolUse(PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({"command": "symposium crate serde"}),
            tool_response: serde_json::json!({"exit_code": 0}),
            session_id: Some("s1".to_string()),
            cwd: Some(cwd.clone()),
        }),
        rest: serde_json::Map::new(),
    };
    ctx.invoke_hook(&activate).await;

    // Second: mention serde in a prompt — should not nudge since already activated
    let prompt = HookPayload {
        sub_payload: HookSubPayload::UserPromptSubmit(UserPromptSubmitPayload {
            prompt: "I need to use `serde` for serialization".to_string(),
            session_id: Some("s1".to_string()),
            cwd: Some(cwd),
        }),
        rest: serde_json::Map::new(),
    };
    let output = ctx.invoke_hook(&prompt).await;
    // No nudge because serde was already activated
    assert!(output.hook_specific_output.is_none());
}
