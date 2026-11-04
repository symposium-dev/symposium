use serde_json::{Value, json};
use symposium::hook_schema::{HookAgent, agent_event};
use symposium_testlib::{HookStep, TestContext, TestMode, with_fixture};

async fn respond(ctx: &TestContext, cwd: &str, step: HookStep) -> Value {
    let event = step.event();
    let handler =
        agent_event(HookAgent::Antigravity, event).expect("Antigravity handles the event");
    let payload: Value = serde_json::from_str(
        &handler
            .translate_input(&step.to_input_event(cwd))
            .to_string()
            .unwrap(),
    )
    .unwrap();
    let response = ctx
        .invoke_hook(HookAgent::Antigravity, event, &payload)
        .await
        .unwrap();
    assert_eq!(response.exit_code, 0);
    serde_json::from_slice(&response.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout is not JSON ({e}): {:?}",
            String::from_utf8_lossy(&response.stdout)
        )
    })
}

fn tool_call(tool_name: &str) -> HookStep {
    HookStep::PreToolUse {
        tool_name: tool_name.to_string(),
        tool_input: json!({"CommandLine": "rm -rf target"}),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn native_ask_survives_a_context_only_hook() {
    with_fixture(
        TestMode::SimulationOnly,
        &["antigravity-native0"],
        async |ctx| {
            let cwd = ctx.tempdir.display().to_string();
            let stdout = respond(&ctx, &cwd, tool_call("AskProbe")).await;
            assert_eq!(
                stdout,
                json!({"decision": "ask", "reason": "confirm deletion"})
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn native_object_without_a_decision_denies() {
    with_fixture(
        TestMode::SimulationOnly,
        &["antigravity-native0"],
        async |ctx| {
            let cwd = ctx.tempdir.display().to_string();
            let stdout = respond(&ctx, &cwd, tool_call("EmptyProbe")).await;
            assert_eq!(stdout["decision"], "deny", "{stdout}");
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn native_user_message_survives_joined_context() {
    with_fixture(
        TestMode::SimulationOnly,
        &["antigravity-native0"],
        async |ctx| {
            let cwd = ctx.tempdir.display().to_string();
            let stdout = respond(&ctx, &cwd, HookStep::user_prompt("hello")).await;
            assert_eq!(
                stdout,
                json!({"injectSteps": [
                    {"userMessage": "please also run the tests"},
                    {"ephemeralMessage": "portable context"},
                ]})
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn hook_predicates_hold_in_the_payload_workspace() {
    with_fixture(
        TestMode::SimulationOnly,
        &["antigravity-workspace0"],
        async |ctx| {
            let cwd = ctx.tempdir.display().to_string();
            let stdout = respond(&ctx, &cwd, tool_call("run_command")).await;
            assert_eq!(
                stdout,
                json!({"decision": "deny", "reason": "denied in the workspace"})
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn plugin_hooks_run_in_the_payload_workspace() {
    with_fixture(
        TestMode::SimulationOnly,
        &["antigravity-workspace0"],
        async |ctx| {
            let cwd = ctx.tempdir.display().to_string();
            let stdout = respond(&ctx, &cwd, HookStep::user_prompt("hello")).await;
            assert_eq!(
                stdout,
                json!({"injectSteps": [{"ephemeralMessage": "hook ran in the workspace"}]})
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}
