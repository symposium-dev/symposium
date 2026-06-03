//! Dual-mode agent integration tests.
//!
//! See `md/design/running-tests.md` for how to run these.

use symposium::hook_schema::HookAgent;
use symposium_testlib::{HookStep, TestMode, with_fixture};

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
/// agent to discover them via `cargo agents --help`.
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

            expect_test::expect![[r#"This project has crate-aware tools available via `cargo agents`. Run `cargo agents --help` to list them before working with the Rust code. Only use tools under the 'Commands for agents' section unless the user explicitly asks you to run one from 'Commands for humans'."#]]
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
