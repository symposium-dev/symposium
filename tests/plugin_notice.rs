use std::path::Path;
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};
use symposium::hook_schema::{HookAgent, agent_event};
use symposium_testlib::{HookStep, TestContext, TestMode, with_fixture};

const NOTICE: &str = "agent plugins that are not enabled yet";

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

fn add_dependency(root: &Path, name: &str) {
    let manifest = root.join("Cargo.toml");
    let mut text = std::fs::read_to_string(&manifest).unwrap();
    text.push_str(&format!("{name} = {{ path = \"{name}\" }}\n"));
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
    for expected in [
        NOTICE,
        "crate-b",
        "cargo agents sync",
        "Do not enable them yourself",
    ] {
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

            add_dependency(ctx.workspace_root.as_ref().unwrap(), "crate-b");
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

            add_dependency(ctx.workspace_root.as_ref().unwrap(), "crate-b");
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

                add_dependency(ctx.workspace_root.as_ref().unwrap(), "crate-b");
                let stdout = respond(&ctx, agent, tool_call_finished()).await;
                assert_notice(agent, &stdout);
                Ok(())
            },
        )
        .await
        .unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_workspace_seen_for_the_first_time_gets_no_notice() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-notice0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            let stdout = respond(&ctx, HookAgent::Claude, HookStep::user_prompt("first")).await;
            assert!(!stdout.contains(NOTICE), "first prompt: {stdout}");

            add_dependency(ctx.workspace_root.as_ref().unwrap(), "crate-b");
            let stdout = respond(&ctx, HookAgent::Claude, HookStep::user_prompt("next")).await;
            assert_notice(HookAgent::Claude, &stdout);
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dependency_auto_enable_covers_is_installed_without_a_notice() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-notice0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.sym
                .config
                .plugins
                .auto_enable
                .push("crate-b".to_string());
            start_session(&ctx, HookAgent::Claude).await;

            let root = ctx.workspace_root.clone().unwrap();
            add_dependency(&root, "crate-b");
            let stdout = respond(&ctx, HookAgent::Claude, tool_call_finished()).await;
            assert!(!stdout.contains("cargo agents sync"), "{stdout}");
            assert!(root.join(".claude/skills/b-guidance/SKILL.md").exists());
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dependency_without_plugins_gets_no_notice() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-notice0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            start_session(&ctx, HookAgent::Claude).await;

            add_dependency(ctx.workspace_root.as_ref().unwrap(), "crate-c");
            let stdout = respond(&ctx, HookAgent::Claude, tool_call_finished()).await;
            assert!(!stdout.contains("cargo agents sync"), "{stdout}");
            Ok(())
        },
    )
    .await
    .unwrap();
}
