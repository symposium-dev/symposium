//! End-to-end tests for plugin hook dispatch — load a plugin from a fixture,
//! fire a hook event, and verify the resulting output.

use serde_json::json;
use symposium::hook_schema::{HookAgent, HookEvent};
use symposium_testlib::{HookStep, TestMode, with_fixture};

/// Inline `command = { source = "shell", … }` is promoted to a synthetic
/// installation named after the hook and runs end-to-end.
#[tokio::test(flavor = "multi_thread")]
async fn inline_shell_hook_emits_context() {
    with_fixture(TestMode::SimulationOnly, &["plugin-hooks0"], async |mut ctx| {
        let result = ctx
            .prompt_or_hook(
                "ignored",
                &[HookStep::PreToolUse {
                    tool_name: "Bash".to_string(),
                    tool_input: json!({"command": "ls"}),
                }],
                HookAgent::Claude,
            )
            .await?;

        assert!(
            result.has_context_containing("inline-shell-output"),
            "expected `inline-shell-output` in hook output, got: {:#?}",
            result.outputs_for(HookEvent::PreToolUse),
        );
        Ok(())
    })
    .await
    .unwrap();
}

/// `command = "named-shell"` resolves the named installation at dispatch time.
#[tokio::test(flavor = "multi_thread")]
async fn named_installation_resolves_at_dispatch() {
    with_fixture(TestMode::SimulationOnly, &["plugin-hooks0"], async |mut ctx| {
        let result = ctx
            .prompt_or_hook(
                "ignored",
                &[HookStep::PreToolUse {
                    tool_name: "Read".to_string(),
                    tool_input: json!({"file_path": "/tmp/x"}),
                }],
                HookAgent::Claude,
            )
            .await?;

        assert!(
            result.has_context_containing("named-shell-output"),
            "expected `named-shell-output` in hook output, got: {:#?}",
            result.outputs_for(HookEvent::PreToolUse),
        );
        Ok(())
    })
    .await
    .unwrap();
}

/// A hook with `requirements` runs to completion — the requirement is a
/// no-op shell installation, so this exercises the requirements code path
/// without needing real install side-effects.
#[tokio::test(flavor = "multi_thread")]
async fn hook_with_requirements_runs() {
    with_fixture(TestMode::SimulationOnly, &["plugin-hooks0"], async |mut ctx| {
        let result = ctx
            .prompt_or_hook(
                "ignored",
                &[HookStep::PreToolUse {
                    tool_name: "Edit".to_string(),
                    tool_input: json!({"file_path": "/tmp/x"}),
                }],
                HookAgent::Claude,
            )
            .await?;

        assert!(
            result.has_context_containing("with-requirements-output"),
            "expected `with-requirements-output` in hook output, got: {:#?}",
            result.outputs_for(HookEvent::PreToolUse),
        );
        Ok(())
    })
    .await
    .unwrap();
}

/// Hook-level `args` reach the shell as positional parameters (`$1`, …).
#[tokio::test(flavor = "multi_thread")]
async fn shell_hook_receives_positional_args() {
    with_fixture(TestMode::SimulationOnly, &["plugin-hooks0"], async |mut ctx| {
        let result = ctx
            .prompt_or_hook(
                "ignored",
                &[HookStep::PreToolUse {
                    tool_name: "Glob".to_string(),
                    tool_input: json!({"pattern": "*.rs"}),
                }],
                HookAgent::Claude,
            )
            .await?;

        assert!(
            result.has_context_containing("shell-args:picked-up"),
            "expected hook to receive `picked-up` as $1, got: {:#?}",
            result.outputs_for(HookEvent::PreToolUse),
        );
        Ok(())
    })
    .await
    .unwrap();
}

/// The `matcher` field filters hooks by tool name. Firing a tool no hook
/// matches produces no `additionalContext` in the merged output.
#[tokio::test(flavor = "multi_thread")]
async fn matcher_filters_out_non_matching_hooks() {
    with_fixture(TestMode::SimulationOnly, &["plugin-hooks0"], async |mut ctx| {
        let result = ctx
            .prompt_or_hook(
                "ignored",
                &[HookStep::PreToolUse {
                    tool_name: "Grep".to_string(),
                    tool_input: json!({"pattern": "foo"}),
                }],
                HookAgent::Claude,
            )
            .await?;

        assert!(
            !result.has_context_containing("output"),
            "no hook should fire for `Grep`, got: {:#?}",
            result.outputs_for(HookEvent::PreToolUse),
        );
        Ok(())
    })
    .await
    .unwrap();
}
