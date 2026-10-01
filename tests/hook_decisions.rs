//! A symposium-format plugin hook can deny, rewrite, or block a tool call,
//! and each agent must receive that decision in the form it honors: a
//! decision in its JSON for most agents, exit 2 for Kiro, which reads none.
//!
//! The `decisions` plugin picks its behavior from the tool name, so every
//! case runs against the same fixture.

use serde_json::{Value, json};
use symposium::hook::HookResponse;
use symposium::hook_schema::{HookAgent, agent_event};
use symposium_testlib::{HookStep, TestContext, TestMode, with_fixture};

const ALL_AGENTS: [HookAgent; 5] = [
    HookAgent::Claude,
    HookAgent::Codex,
    HookAgent::Copilot,
    HookAgent::Kiro,
    HookAgent::Antigravity,
];

const DENY_REASON: &str = "denied by the decisions plugin";
const BLOCK_REASON: &str = "blocked by the decisions plugin";

/// Run `step` through `agent`'s wire format, as the agent would invoke the hook.
async fn respond(ctx: &TestContext, agent: HookAgent, step: HookStep) -> HookResponse {
    let event = step.event();
    let cwd = ctx.tempdir.display().to_string();
    let handler = agent_event(agent, event).expect("agent handles the event");
    let payload: Value = serde_json::from_str(
        &handler
            .translate_input(&step.to_input_event(&cwd))
            .to_string()
            .unwrap(),
    )
    .unwrap();
    ctx.invoke_hook(agent, event, &payload).await.unwrap()
}

fn tool_call(tool_name: &str) -> HookStep {
    HookStep::PreToolUse {
        tool_name: tool_name.to_string(),
        tool_input: json!({"command": "rm -rf target"}),
    }
}

fn stdout_json(response: &HookResponse) -> Value {
    serde_json::from_slice(&response.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout is not JSON ({e}): {:?}",
            String::from_utf8_lossy(&response.stdout)
        )
    })
}

/// How `agent` must receive a denial carrying `reason`.
fn assert_denied(agent: HookAgent, response: &HookResponse, reason: &str) {
    if agent == HookAgent::Kiro {
        assert_eq!(response.exit_code, 2, "Kiro denies only through exit 2");
        assert!(
            response.stdout.is_empty(),
            "denied Kiro call writes no context"
        );
        assert_eq!(String::from_utf8_lossy(&response.stderr), reason);
        return;
    }
    let expected = match agent {
        HookAgent::Claude | HookAgent::Codex => json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": reason,
            }
        }),
        HookAgent::Copilot => json!({
            "permissionDecision": "deny",
            "permissionDecisionReason": reason,
        }),
        HookAgent::Antigravity => json!({"decision": "deny", "reason": reason}),
        other => unreachable!("{other:?} has no PreToolUse hook"),
    };
    assert_eq!(
        response.exit_code, 0,
        "{agent:?} reads the denial from stdout"
    );
    assert_eq!(stdout_json(response), expected, "{agent:?} denial");
}

/// `zz-late-rewrite` rewrites the same call after `decisions` denies it; the
/// denial stays final, as agents rank it (Claude: deny over ask over allow).
#[tokio::test(flavor = "multi_thread")]
async fn plugin_deny_reaches_every_agent() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-hook-decisions0"],
        async |ctx| {
            for agent in ALL_AGENTS {
                let response = respond(&ctx, agent, tool_call("DenyProbe")).await;
                assert_denied(agent, &response, DENY_REASON);
            }
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// Exit 2 is the plugin contract's other way to block. On a tool call it
/// becomes the same denial, so it reaches Antigravity (which ignores exit
/// codes) and keeps its reason on Copilot (which drops it on a non-zero exit).
#[tokio::test(flavor = "multi_thread")]
async fn plugin_exit_2_denies_the_tool_call_on_every_agent() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-hook-decisions0"],
        async |ctx| {
            for agent in ALL_AGENTS {
                let response = respond(&ctx, agent, tool_call("BlockProbe")).await;
                assert_denied(agent, &response, BLOCK_REASON);
            }
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// A rewrite reaches every agent that can apply it without approving the
/// call on the user's behalf: Claude gets `ask`, so the user sees the
/// rewritten call. Codex (allow-only) and Kiro (no rewrite) leave the call
/// untouched rather than run something else.
#[tokio::test(flavor = "multi_thread")]
async fn plugin_rewrite_reaches_agents_that_can_apply_it() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-hook-decisions0"],
        async |ctx| {
            let rewritten = json!({"command": "echo rewritten"});
            for agent in ALL_AGENTS {
                let response = respond(&ctx, agent, tool_call("RewriteProbe")).await;
                assert_eq!(response.exit_code, 0, "{agent:?} rewrite exit code");
                if agent == HookAgent::Kiro {
                    assert!(response.stdout.is_empty(), "Kiro cannot rewrite a call");
                    continue;
                }
                let expected = match agent {
                    HookAgent::Claude => json!({
                        "hookSpecificOutput": {
                            "hookEventName": "PreToolUse",
                            "permissionDecision": "ask",
                            "updatedInput": rewritten,
                        }
                    }),
                    HookAgent::Codex => json!({}),
                    HookAgent::Copilot => json!({"modifiedArgs": rewritten}),
                    HookAgent::Antigravity => json!({"decision": "allow", "overwrite": rewritten}),
                    other => unreachable!("{other:?} has no PreToolUse hook"),
                };
                assert_eq!(stdout_json(&response), expected, "{agent:?} rewrite");
            }
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// Outside a tool call, exit 2 passes through with its reason, and each agent
/// applies its own blocking semantics (Claude erases the prompt, for one).
#[tokio::test(flavor = "multi_thread")]
async fn plugin_exit_2_on_a_prompt_passes_through() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-hook-decisions0"],
        async |ctx| {
            for agent in ALL_AGENTS {
                let response = respond(&ctx, agent, HookStep::user_prompt("hello")).await;
                assert_eq!(response.exit_code, 2, "{agent:?} prompt exit code");
                assert!(response.stdout.is_empty(), "{agent:?} prompt stdout");
                assert_eq!(String::from_utf8_lossy(&response.stderr), BLOCK_REASON);
            }
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// The exit code and stderr are what the agent actually sees, so check them on
/// the real `cargo agents hook` process, not just the in-process pipeline.
#[tokio::test(flavor = "multi_thread")]
async fn hook_command_exits_2_with_the_reason_on_stderr() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-hook-decisions0"],
        async |ctx| {
            let cwd = ctx.tempdir.display().to_string();
            let cases = [
                (
                    HookAgent::Kiro,
                    "pre-tool-use",
                    tool_call("DenyProbe"),
                    DENY_REASON,
                ),
                (
                    HookAgent::Claude,
                    "user-prompt-submit",
                    HookStep::user_prompt("hello"),
                    BLOCK_REASON,
                ),
            ];
            for (agent, event_arg, step, reason) in cases {
                let event = step.event();
                let payload = agent_event(agent, event)
                    .unwrap()
                    .translate_input(&step.to_input_event(&cwd))
                    .to_string()?;
                let output = run_hook_command(&ctx, agent, event_arg, &payload);
                assert_eq!(
                    output.status.code(),
                    Some(2),
                    "{agent:?} {event:?} exit status"
                );
                assert_eq!(String::from_utf8_lossy(&output.stderr), reason);
            }
            Ok(())
        },
    )
    .await
    .unwrap();
}

fn run_hook_command(
    ctx: &TestContext,
    agent: HookAgent,
    event_arg: &str,
    payload: &str,
) -> std::process::Output {
    use std::io::Write;

    let binary = std::env::var("CARGO_BIN_EXE_cargo-agents").expect("must run via cargo test");
    let mut child = std::process::Command::new(binary)
        .args(["hook", agent.as_str(), event_arg])
        .env("SYMPOSIUM_HOME", ctx.sym.config_dir())
        .current_dir(&ctx.tempdir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn cargo-agents");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}
