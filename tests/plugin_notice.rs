use std::path::Path;
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};
use symposium::hook_schema::{HookAgent, agent_event};
use symposium_testlib::{HookStep, TestContext, TestMode, with_fixture};

const NOTICE: &str = "there may be new agent plugins";

async fn respond(ctx: &TestContext, agent: HookAgent, step: HookStep) -> String {
    let event = step.event();
    let cwd = ctx.workspace_root.as_ref().unwrap().display().to_string();
    let handler = agent_event(agent, event).expect("agent handles the event");
    let payload: Value = serde_json::from_str(
        &handler
            .translate_input(&step.to_input_event(&cwd))
            .to_string()
            .unwrap(),
    )
    .unwrap();
    let stdout = ctx.invoke_hook(agent, event, &payload).await.unwrap();
    String::from_utf8(stdout.stdout).unwrap()
}

fn tool_call_finished() -> HookStep {
    HookStep::PostToolUse {
        tool_name: "Bash".to_string(),
        tool_input: json!({"command": "cargo add --path crate-b"}),
        tool_response: json!({"stdout": ""}),
    }
}

fn add_crate_b(root: &Path) {
    let manifest = root.join("Cargo.toml");
    let mut text = std::fs::read_to_string(&manifest).unwrap();
    text.push_str("crate-b = { path = \"crate-b\" }\n");
    std::fs::write(&manifest, text).unwrap();
    let status = std::process::Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(root)
        .status()
        .unwrap();
    assert!(status.success(), "cargo generate-lockfile failed");
    // The sync gate and the dependency cache key on `Cargo.lock`'s mtime in
    // whole seconds; step past whatever was recorded in the current one.
    std::fs::File::options()
        .write(true)
        .open(root.join("Cargo.lock"))
        .unwrap()
        .set_modified(SystemTime::now() + Duration::from_secs(5))
        .unwrap();
}

async fn start_session(ctx: &TestContext, agent: HookAgent) {
    let stdout = respond(ctx, agent, HookStep::session_start()).await;
    assert!(
        !stdout.contains(NOTICE),
        "{agent:?} session start: {stdout}"
    );
}

fn assert_notice(agent: HookAgent, stdout: &str) {
    for expected in [NOTICE, "cargo agents sync", "Do not enable them yourself"] {
        assert!(
            stdout.contains(expected),
            "{agent:?} should get the notice: {stdout}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dependency_added_by_a_tool_call_is_noticed_once() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-notice0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            start_session(&ctx, HookAgent::Claude).await;

            add_crate_b(ctx.workspace_root.as_ref().unwrap());
            let stdout = respond(&ctx, HookAgent::Claude, tool_call_finished()).await;
            assert_notice(HookAgent::Claude, &stdout);

            for step in [tool_call_finished(), HookStep::user_prompt("next")] {
                let stdout = respond(&ctx, HookAgent::Claude, step).await;
                assert!(!stdout.contains(NOTICE), "noticed twice: {stdout}");
            }
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dependency_the_user_added_is_noticed_with_their_next_prompt() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-notice0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            start_session(&ctx, HookAgent::Claude).await;

            add_crate_b(ctx.workspace_root.as_ref().unwrap());
            let stdout = respond(&ctx, HookAgent::Claude, HookStep::user_prompt("next")).await;
            assert_notice(HookAgent::Claude, &stdout);
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn agents_that_read_context_after_a_tool_call_get_the_notice() {
    for agent in [HookAgent::Codex, HookAgent::Copilot, HookAgent::Kiro] {
        with_fixture(
            TestMode::SimulationOnly,
            &["plugin-notice0"],
            async |mut ctx| {
                ctx.symposium(&["init", "--add-agent", "claude"]).await?;
                start_session(&ctx, agent).await;

                add_crate_b(ctx.workspace_root.as_ref().unwrap());
                let stdout = respond(&ctx, agent, tool_call_finished()).await;
                assert_notice(agent, &stdout);
                Ok(())
            },
        )
        .await
        .unwrap();
    }
}
