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
