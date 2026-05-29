//! End-to-end tests for plugin hook dispatch — load a plugin from a fixture,
//! fire a hook event, and verify the resulting output.

use serde_json::json;
use symposium::hook_schema::{HookAgent, HookEvent};
use symposium_testlib::{HookStep, TestMode, with_fixture};

/// Inline `command = { source = "shell", … }` is promoted to a synthetic
/// installation named after the hook and runs end-to-end.
#[tokio::test(flavor = "multi_thread")]
async fn inline_shell_hook_emits_context() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-hooks0"],
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
                result.has_context_containing("inline-shell-output"),
                "expected `inline-shell-output` in hook output, got: {:#?}",
                result.outputs_for(HookEvent::PreToolUse),
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// `command = "named-shell"` resolves the named installation at dispatch time.
#[tokio::test(flavor = "multi_thread")]
async fn named_installation_resolves_at_dispatch() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-hooks0"],
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
                result.has_context_containing("named-shell-output"),
                "expected `named-shell-output` in hook output, got: {:#?}",
                result.outputs_for(HookEvent::PreToolUse),
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// A hook with `requirements` runs to completion — the requirement is a
/// no-op shell installation, so this exercises the requirements code path
/// without needing real install side-effects.
#[tokio::test(flavor = "multi_thread")]
async fn hook_with_requirements_runs() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-hooks0"],
        async |mut ctx| {
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
        },
    )
    .await
    .unwrap();
}

/// Hook-level `args` reach the shell as positional parameters (`$1`, …).
#[tokio::test(flavor = "multi_thread")]
async fn shell_hook_receives_positional_args() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-hooks0"],
        async |mut ctx| {
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
        },
    )
    .await
    .unwrap();
}

/// `install_commands` declared on an installation run after the kind-specific
/// install step. Here, the install command writes a JSON file the hook's
/// shell command then reads — proving the post-install step happened first.
#[tokio::test(flavor = "multi_thread")]
async fn install_commands_run_before_command() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-hooks0"],
        async |mut ctx| {
            let result = ctx
                .prompt_or_hook(
                    "ignored",
                    &[HookStep::PreToolUse {
                        tool_name: "WebFetch".to_string(),
                        tool_input: json!({"url": "https://example.com"}),
                    }],
                    HookAgent::Claude,
                )
                .await?;

            assert!(
                result.has_context_containing("install-cmd-ran"),
                "expected `install-cmd-ran` (proof install_commands ran), got: {:#?}",
                result.outputs_for(HookEvent::PreToolUse),
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// `install_commands` declared on an *inline* command (promoted to a
/// synthetic installation) also run before the resolved command.
#[tokio::test(flavor = "multi_thread")]
async fn inline_install_commands_run_before_command() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-hooks0"],
        async |mut ctx| {
            let result = ctx
                .prompt_or_hook(
                    "ignored",
                    &[HookStep::PreToolUse {
                        tool_name: "Write".to_string(),
                        tool_input: json!({"file_path": "/tmp/x", "content": "y"}),
                    }],
                    HookAgent::Claude,
                )
                .await?;

            assert!(
                result.has_context_containing("inline-install-cmd-ran"),
                "expected `inline-install-cmd-ran` in hook output, got: {:#?}",
                result.outputs_for(HookEvent::PreToolUse),
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// Hook-level `script` overrides a bare installation: the installation only
/// contributes `install_commands`, the hook supplies the runnable.
#[tokio::test(flavor = "multi_thread")]
async fn hook_supplies_script_against_bare_installation() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-hooks0"],
        async |mut ctx| {
            let result = ctx
                .prompt_or_hook(
                    "ignored",
                    &[HookStep::PreToolUse {
                        tool_name: "Task".to_string(),
                        tool_input: json!({"description": "x"}),
                    }],
                    HookAgent::Claude,
                )
                .await?;

            assert!(
                result.has_context_containing("hook-supplied-script-output"),
                "expected `hook-supplied-script-output` in hook output, got: {:#?}",
                result.outputs_for(HookEvent::PreToolUse),
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// The `matcher` field filters hooks by tool name. Firing a tool no hook
/// matches produces no `additionalContext` in the merged output.
#[tokio::test(flavor = "multi_thread")]
async fn matcher_filters_out_non_matching_hooks() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-hooks0"],
        async |mut ctx| {
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
        },
    )
    .await
    .unwrap();
}

/// OpenCode as host agent with a symposium-format hook: the inline shell hook
/// produces `additionalContext` that flows through the symposium → OpenCode path.
#[tokio::test(flavor = "multi_thread")]
async fn opencode_host_receives_symposium_hook_context() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-hooks0"],
        async |mut ctx| {
            let result = ctx
                .prompt_or_hook(
                    "ignored",
                    &[HookStep::PreToolUse {
                        tool_name: "Bash".to_string(),
                        tool_input: json!({"command": "ls"}),
                    }],
                    HookAgent::OpenCode,
                )
                .await?;

            assert!(
                result.has_context_containing("inline-shell-output"),
                "expected `inline-shell-output` in OpenCode hook output, got: {:#?}",
                result.outputs_for(HookEvent::PreToolUse),
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// OpenCode as host agent: matcher filtering still works — a tool name no hook
/// matches produces no additionalContext.
#[tokio::test(flavor = "multi_thread")]
async fn opencode_host_matcher_filters() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-hooks0"],
        async |mut ctx| {
            let result = ctx
                .prompt_or_hook(
                    "ignored",
                    &[HookStep::PreToolUse {
                        tool_name: "Grep".to_string(),
                        tool_input: json!({"pattern": "foo"}),
                    }],
                    HookAgent::OpenCode,
                )
                .await?;

            assert!(
                !result.has_context_containing("output"),
                "no hook should fire for `Grep` via OpenCode, got: {:#?}",
                result.outputs_for(HookEvent::PreToolUse),
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// OpenCode receives PostToolUse hook output.
#[tokio::test(flavor = "multi_thread")]
async fn opencode_host_post_tool_use() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-hooks0"],
        async |mut ctx| {
            let result = ctx
                .prompt_or_hook(
                    "ignored",
                    &[HookStep::PostToolUse {
                        tool_name: "Bash".to_string(),
                        tool_input: json!({"command": "ls"}),
                        tool_response: json!("file1.rs\nfile2.rs"),
                    }],
                    HookAgent::OpenCode,
                )
                .await?;

            // PostToolUse hooks are not configured for Bash in this fixture,
            // so there should be no context. This verifies the pipeline doesn't
            // error out for OpenCode PostToolUse events.
            assert!(
                !result.has_context_containing("output"),
                "no PostToolUse hooks configured for Bash, got: {:#?}",
                result.outputs_for(HookEvent::PostToolUse),
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}
