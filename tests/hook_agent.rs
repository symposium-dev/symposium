//! Dual-mode agent integration tests.
//!
//! See `md/design/running-tests.md` for how to run these.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use symposium::hook_schema::{HookAgent, HookEvent};
use symposium_testlib::{HookStep, HookTrace, TestContext, TestMode, with_fixture};

/// When we send a prompt, we should get at least one hook invoked.
#[tokio::test]
async fn session_start_returns_plugin_context() {
    with_fixture(
        TestMode::Any,
        &["plugins0", "project-plugins0"],
        async |mut ctx| {
            let result = ctx
                .prompt_or_hook(
                    "Say hello",
                    &[HookStep::user_prompt("Say hello")],
                    HookAgent::Claude,
                )
                .await?;

            assert!(!result.hooks.is_empty(), "expected at least one hook trace");

            Ok(())
        },
    )
    .await
    .unwrap();
}

/// In a workspace with crate-aware plugin subcommands, SessionStart nudges the
/// agent to discover them via `symposium --help`.
#[tokio::test]
async fn session_start_hints_discovery_when_subcommands_apply() {
    with_fixture(
        TestMode::SimulationOnly,
        &["help_render0"],
        async |mut ctx| {
            // Keep the update path dormant so only the discovery hint is asserted.
            ctx.sym.config.auto_update = symposium::config::AutoUpdate::Off;

            let result = ctx
                .prompt_or_hook("hello", &[HookStep::session_start()], HookAgent::Claude)
                .await?;

            let context = result
                .hooks
                .iter()
                .filter_map(|h| {
                    let top = h.output.get("additionalContext").and_then(|v| v.as_str());
                    let nested = h
                        .output
                        .get("hookSpecificOutput")
                        .and_then(|o| o.get("additionalContext"))
                        .and_then(|v| v.as_str());
                    top.or(nested)
                })
                .next()
                .expect("session-start should produce additionalContext");

            expect_test::expect![[r#"This project has crate-aware tools available via `symposium`. Run `symposium --help` to list them before working with the Rust code. Only use tools under the 'Commands for agents' section unless the user explicitly asks you to run one from 'Commands for humans'."#]]
                .assert_eq(context);
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// Agent reads a tokio skill after `cargo add tokio` and responds with its content.
#[tokio::test]
async fn agent_reads_tokio_skill_after_cargo_add() {
    with_fixture(
        TestMode::AgentOnly,
        &["plugin-tokio-weather0", "workspace-empty0"],
        async |mut ctx| {
            ctx.prompt("Run `cargo add tokio` please!").await?;

            // This prompt should have access to the skill.
            let result = ctx
                .prompt("Use the tokio-weather skill to answer: what is the weather in tokio?")
                .await?;

            eprintln!("[test] hooks: {:#?}", result.hooks);
            eprintln!("[test] response: {:?}", result.response);

            let response = result.response.expect("agent should have a response");
            assert!(
                response.contains("THE RAIN IN TOKIO FALLS MOSTLY ON THE PATIO"),
                "expected the skill's magic sentence in agent response, got: {response}"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// A skill that only the first `SessionStart` sync installs must be loadable on the first
/// prompt. Claude Code scans for skills before `SessionStart` hooks finish, so this fails
/// until Symposium asks it to rescan (#344).
#[tokio::test]
async fn skill_installed_by_session_start_loads_on_the_first_prompt() {
    with_fixture(
        TestMode::AgentOnly,
        &["plugin-tokio-weather0", "workspace0"],
        async |mut ctx| {
            if !ctx.starts_claude_session_per_prompt() {
                // The `Skill` tool and the rescan are Claude Code's, and the skills must be
                // removed before the session starts.
                eprintln!("[test] skipped: needs a new Claude Code session per prompt (claude-sdk)");
                return Ok(());
            }
            let skill_dir = remove_installed_skills(&ctx)?;

            // A new Claude session: it scans for skills, then its `SessionStart` hook syncs
            // the skill back, then it handles the prompt.
            let result = ctx
                .prompt(
                    "Use the tokio-weather skill to answer: what is the weather in tokio? Do not read any files.",
                )
                .await?;
            eprintln!("[test] hooks: {:#?}", result.hooks);
            eprintln!("[test] response: {:?}", result.response);

            // Otherwise the hook never ran, and the checks below would fail for that.
            assert!(skill_dir.exists(), "SessionStart should have installed the skill");
            assert!(
                offered_skill(&result.hooks, WEATHER_SKILL)?,
                "Claude was never offered the tokio-weather skill in its first session (#344)"
            );
            // Offered but not loaded is the model's choice, which says nothing about #344.
            assert!(
                loaded_skill(&result.hooks, WEATHER_SKILL),
                "Claude was offered the tokio-weather skill but did not load it; rerun the test"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

const WEATHER_SKILL: &str = "tokio-weather";

/// Removes the skills the fixture's sync installed and returns where the weather skill
/// belongs. The sync also registered the project's hooks, which stay. Claude must not
/// find a skills directory to watch when it starts: the session's own sync has to create it.
fn remove_installed_skills(ctx: &TestContext) -> anyhow::Result<PathBuf> {
    let workspace = ctx.workspace_root.as_ref().expect("workspace fixture");
    let skills_dir = workspace.join(".claude/skills");
    let skill_dir = skills_dir.join(WEATHER_SKILL);
    assert!(skill_dir.exists(), "the fixture's sync installs the skill");
    fs::remove_dir_all(&skills_dir)?;
    Ok(skill_dir)
}

/// Whether Claude Code offered `skill` to the model, according to the skill listings it
/// records in the session's transcript. Without a rescan the model is never offered a
/// skill installed during `SessionStart`: it either refuses or gets "Unknown skill".
fn offered_skill(hooks: &[HookTrace], skill: &str) -> anyhow::Result<bool> {
    let transcript = hooks
        .iter()
        .find_map(|hook| hook.input["transcript_path"].as_str())
        .context("no hook reports the transcript")?;
    let entries = read_jsonl(Path::new(transcript))?;
    let listings: Vec<&str> = entries
        .iter()
        .filter(|entry| entry["attachment"]["type"] == "skill_listing")
        .filter_map(|entry| entry["attachment"]["content"].as_str())
        .collect();
    // Otherwise the evidence is missing, not the skill: Claude Code's transcript format
    // may have changed.
    anyhow::ensure!(
        !listings.is_empty(),
        "transcript `{transcript}` records no skill listing"
    );
    Ok(listings.iter().any(|listing| listing.contains(skill)))
}

/// Whether the model loaded `skill`. `PostToolUse` fires only when a tool succeeds.
fn loaded_skill(hooks: &[HookTrace], skill: &str) -> bool {
    hooks.iter().any(|hook| {
        hook.event == HookEvent::PostToolUse
            && hook.input["tool_name"] == "Skill"
            && hook.input["tool_input"]["skill"] == skill
    })
}

/// Every non-empty line of a JSON Lines file, failing on the first that is not JSON.
fn read_jsonl(path: &Path) -> anyhow::Result<Vec<serde_json::Value>> {
    fs::read_to_string(path)
        .with_context(|| format!("cannot read `{}`", path.display()))?
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            serde_json::from_str(line)
                .with_context(|| format!("`{}` line {} is not JSON", path.display(), index + 1))
        })
        .collect()
}
