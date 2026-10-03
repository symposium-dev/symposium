//! Tests for per-plugin format selection in hook dispatch.
//!
//! Verifies that:
//! - A native hook (matching the host agent) takes priority over a symposium hook.
//! - A symposium hook fires when no native hook matches.
//! - A hook for a different agent does not fire when there's no symposium fallback.

use serde_json::json;
use symposium::hook_schema::{HookAgent, HookEvent};
use symposium_testlib::{HookStep, TestMode, with_fixture};

/// When running on Claude and the plugin has both `format = "claude"` and
/// `format = "symposium"` hooks, the claude hook fires (native priority).
#[tokio::test(flavor = "multi_thread")]
async fn native_hook_takes_priority_over_symposium() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-hooks-format"],
        async |mut ctx| {
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
                result.has_context_containing("claude-hook-fired"),
                "expected claude native hook to fire on Claude, got: {:#?}",
                result.outputs_for(HookEvent::PreToolUse),
            );
            assert!(
                !result.has_context_containing("symposium-hook-fired"),
                "symposium hook should NOT fire when native hook matches",
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// Pi native hooks receive flat fields and retain decisions and input changes.
#[tokio::test(flavor = "multi_thread")]
async fn pi_native_hook_takes_priority_over_symposium() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-hooks-format"],
        async |ctx| {
            let output = ctx
                .invoke_hook(
                    HookAgent::Pi,
                    HookEvent::PreToolUse,
                    &json!({
                        "cwd": ctx.tempdir.display().to_string(),
                        "tool_name": "bash",
                        "tool_input": {"command": "ls"},
                    }),
                )
                .await?;
            let output: serde_json::Value = serde_json::from_slice(&output)?;
            assert_eq!(output["decision"], "deny");
            assert_eq!(output["updatedInput"]["command"], "pwd");
            assert_eq!(output["additionalContext"], "pi-hook-fired");
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// When running on Copilot and the plugin has both `format = "claude"` and
/// `format = "symposium"` hooks, the symposium hook fires (no native match).
/// Copilot names its shell tool `bash`, not Claude's `Bash`.
#[tokio::test(flavor = "multi_thread")]
async fn symposium_hook_fires_when_no_native_match() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-hooks-format"],
        async |mut ctx| {
            let result = ctx
                .prompt_or_hook(
                    "ignored",
                    &[HookStep::PreToolUse {
                        tool_name: "bash".to_string(),
                        tool_input: json!({"command": "ls"}),
                    }],
                    HookAgent::Copilot,
                )
                .await?;

            assert!(
                result.has_context_containing("symposium-hook-fired"),
                "expected symposium hook to fire on Copilot, got: {:#?}",
                result.outputs_for(HookEvent::PreToolUse),
            );
            assert!(
                !result.has_context_containing("claude-hook-fired"),
                "claude hook should NOT fire on Copilot",
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// When running on Claude and the plugin only has `format = "claude"` (no
/// symposium fallback), the hook fires.
#[tokio::test(flavor = "multi_thread")]
async fn claude_only_hook_fires_on_claude() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-hooks-format"],
        async |mut ctx| {
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
                result.has_context_containing("claude-only-fired"),
                "expected claude-only hook to fire on Claude, got: {:#?}",
                result.outputs_for(HookEvent::PreToolUse),
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// When running on Copilot and the plugin only has `format = "claude"` (no
/// symposium fallback), nothing fires — the hook is silently skipped.
#[tokio::test(flavor = "multi_thread")]
async fn claude_only_hook_does_not_fire_on_other_agents() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-hooks-format"],
        async |mut ctx| {
            let result = ctx
                .prompt_or_hook(
                    "ignored",
                    &[HookStep::PreToolUse {
                        tool_name: "Read".to_string(),
                        tool_input: json!({"file_path": "/tmp/x"}),
                    }],
                    HookAgent::Copilot,
                )
                .await?;

            assert!(
                !result.has_context_containing("claude-only-fired"),
                "claude-only hook should NOT fire on Copilot, got: {:#?}",
                result.outputs_for(HookEvent::PreToolUse),
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}
