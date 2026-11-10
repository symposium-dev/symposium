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
    regenerate_lockfile(root);
}

fn bump_version(root: &Path, name: &str, version: &str) {
    let manifest = root.join(name).join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest)
        .unwrap()
        .replace("version = \"0.1.0\"", &format!("version = \"{version}\""));
    std::fs::write(&manifest, text).unwrap();
    regenerate_lockfile(root);
}

fn regenerate_lockfile(root: &Path) {
    let lock = root.join("Cargo.lock");
    let previous = std::fs::metadata(&lock).and_then(|m| m.modified()).ok();
    let status = std::process::Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(root)
        .status()
        .unwrap();
    assert!(status.success(), "cargo generate-lockfile failed");
    // The sync gate and the dependency cache key on `Cargo.lock`'s mtime in
    // whole seconds; step past whatever was recorded in the current one.
    let after = previous.map_or(SystemTime::now(), |p| p.max(SystemTime::now()));
    std::fs::File::options()
        .write(true)
        .open(&lock)
        .unwrap()
        .set_modified(after + Duration::from_secs(5))
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
async fn a_plugin_a_mid_session_sync_installs_comes_with_how_to_load_it() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-reload0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            start_session(&ctx, HookAgent::Claude).await;

            add_dependency(ctx.workspace_root.as_ref().unwrap(), "crate-b");
            let stdout = respond(&ctx, HookAgent::Claude, tool_call_finished()).await;
            assert!(
                stdout.contains("crate-b-guide") && stdout.contains("/reload-plugins"),
                "{stdout}"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
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
async fn session_start_tells_the_agent_how_to_load_the_plugins_its_sync_installed() {
    for (agent, load_now) in [
        (HookAgent::Claude, "/reload-plugins"),
        (HookAgent::Codex, "new session"),
        (HookAgent::Copilot, "/restart"),
        (HookAgent::Antigravity, "/skills reload"),
    ] {
        with_fixture(
            TestMode::SimulationOnly,
            &["agent-plugin-scopes0"],
            async |mut ctx| {
                ctx.symposium(&["init", "--add-agent", agent.as_str()])
                    .await?;

                let installed = respond(&ctx, agent, HookStep::session_start()).await;
                serde_json::from_str::<Value>(&installed)?;
                assert!(
                    installed.contains("global-guide") && installed.contains(load_now),
                    "{installed}"
                );

                let unchanged = respond(&ctx, agent, HookStep::session_start()).await;
                assert!(!unchanged.contains(load_now), "{unchanged}");

                let skill = ctx.sym.config_dir().join("plugins/global-guide/SKILL.md");
                std::fs::write(
                    &skill,
                    "---\nname: global-guide\ndescription: edited\n---\n",
                )?;
                let edited = respond(&ctx, agent, HookStep::session_start()).await;
                assert!(
                    edited.contains("global-guide") && edited.contains(load_now),
                    "{edited}"
                );
                Ok(())
            },
        )
        .await
        .unwrap();
    }
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
            assert!(
                root.join(".claude/skills/crate-b/skills/b-guidance/SKILL.md")
                    .exists()
            );
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

#[tokio::test(flavor = "multi_thread")]
async fn a_noticed_dependency_is_not_named_again_after_an_unrelated_change() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-notice0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            start_session(&ctx, HookAgent::Claude).await;
            let root = ctx.workspace_root.clone().unwrap();

            add_dependency(&root, "crate-b");
            let stdout = respond(&ctx, HookAgent::Claude, tool_call_finished()).await;
            assert_notice(HookAgent::Claude, &stdout);

            add_dependency(&root, "crate-c");
            let stdout = respond(&ctx, HookAgent::Claude, tool_call_finished()).await;
            assert!(!stdout.contains(NOTICE), "noticed crate-b again: {stdout}");
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_later_dependency_is_named_alone() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-notice0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            start_session(&ctx, HookAgent::Claude).await;
            let root = ctx.workspace_root.clone().unwrap();

            add_dependency(&root, "crate-b");
            let stdout = respond(&ctx, HookAgent::Claude, tool_call_finished()).await;
            assert_notice(HookAgent::Claude, &stdout);

            add_dependency(&root, "crate-d");
            let stdout = respond(&ctx, HookAgent::Claude, tool_call_finished()).await;
            assert!(stdout.contains(NOTICE), "crate-d not noticed: {stdout}");
            assert!(stdout.contains("crate-d"), "{stdout}");
            assert!(!stdout.contains("crate-b"), "named crate-b again: {stdout}");
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_version_of_a_noticed_dependency_is_named_again() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-notice0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            start_session(&ctx, HookAgent::Claude).await;
            let root = ctx.workspace_root.clone().unwrap();

            add_dependency(&root, "crate-b");
            add_dependency(&root, "crate-d");
            let stdout = respond(&ctx, HookAgent::Claude, tool_call_finished()).await;
            assert_notice(HookAgent::Claude, &stdout);
            assert!(stdout.contains("crate-d"), "{stdout}");

            bump_version(&root, "crate-b", "0.2.0");
            let stdout = respond(&ctx, HookAgent::Claude, tool_call_finished()).await;
            assert_notice(HookAgent::Claude, &stdout);
            assert!(!stdout.contains("crate-d"), "named crate-d again: {stdout}");
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_notice_lost_to_a_blocked_prompt_is_given_on_the_next_change() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-notice0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.sym.config.defaults.user_plugins = true;
            start_session(&ctx, HookAgent::Claude).await;
            let root = ctx.workspace_root.clone().unwrap();

            add_dependency(&root, "crate-b");
            let stdout = respond(&ctx, HookAgent::Claude, HookStep::user_prompt("next")).await;
            assert!(!stdout.contains(NOTICE), "blocked prompt: {stdout}");

            add_dependency(&root, "crate-d");
            let stdout = respond(&ctx, HookAgent::Claude, tool_call_finished()).await;
            assert_notice(HookAgent::Claude, &stdout);
            assert!(stdout.contains("crate-d"), "{stdout}");
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_notice_the_agent_cannot_read_is_given_on_the_next_change() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-notice0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            start_session(&ctx, HookAgent::Claude).await;
            let root = ctx.workspace_root.clone().unwrap();

            add_dependency(&root, "crate-b");
            let stdout = respond(&ctx, HookAgent::Antigravity, tool_call_finished()).await;
            assert!(!stdout.contains(NOTICE), "{stdout}");

            add_dependency(&root, "crate-d");
            let stdout = respond(&ctx, HookAgent::Claude, tool_call_finished()).await;
            assert_notice(HookAgent::Claude, &stdout);
            assert!(stdout.contains("crate-d"), "{stdout}");
            Ok(())
        },
    )
    .await
    .unwrap();
}
