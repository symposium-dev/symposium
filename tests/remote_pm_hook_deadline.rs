//! A silent package manager must leave time for healthy plugins to answer a hook.
//!
//! These use the deadline from each agent's generated hook registration, just
//! as the agent would. They intentionally use real time: a paused Tokio clock
//! can expire while the fixture's real subprocess is still becoming ready.
#![cfg(unix)]

use std::time::Duration;

use serde_json::{Value, json};
use symposium::agents::Agent;
use symposium::config::Symposium;
use symposium::hook::HookResponse;
use symposium::hook_schema::{HookAgent, HookEvent, agent_event};
use symposium::output::Output;
use symposium_testlib::{HookStep, TestContext, TestMode, with_fixture};

fn registered_deadline(ctx: &TestContext, agent: HookAgent) -> Duration {
    let (agent, path, pointer) = match agent {
        HookAgent::Codex => (
            Agent::Codex,
            ".codex/hooks.json",
            "/hooks/PreToolUse/0/hooks/0/timeout",
        ),
        HookAgent::Copilot => (
            Agent::Copilot,
            ".github/hooks/symposium.json",
            "/hooks/preToolUse/0/timeoutSec",
        ),
        _ => unreachable!(),
    };
    agent
        .register_project_hooks(&ctx.tempdir, &ctx.sym, &Output::quiet())
        .unwrap();
    let registration: Value =
        serde_json::from_slice(&std::fs::read(ctx.tempdir.join(path)).unwrap()).unwrap();
    Duration::from_secs(registration.pointer(pointer).unwrap().as_u64().unwrap())
}

fn assert_healthy_plugin_answered(agent: HookAgent, response: HookResponse) {
    assert_eq!(response.exit_code, 0);
    let output: Value = serde_json::from_slice(&response.stdout).unwrap();
    let decision = match agent {
        HookAgent::Codex => &output["hookSpecificOutput"],
        HookAgent::Copilot => &output,
        _ => unreachable!(),
    };
    assert_eq!(decision["permissionDecision"], "deny");
    assert_eq!(
        decision["permissionDecisionReason"],
        "denied by the decisions plugin"
    );
}

async fn healthy_plugin_answers_despite_a_silent_pm(
    agent: HookAgent,
    pm_count: usize,
    slow_handshake: bool,
) {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-hook-decisions0"],
        async |mut ctx| {
            let deadline = registered_deadline(&ctx, agent);
            let event = HookEvent::PreToolUse;
            let step = HookStep::PreToolUse {
                tool_name: "DenyProbe".to_string(),
                tool_input: json!({}),
            };
            let payload: Value = serde_json::from_str(
                &agent_event(agent, event)
                    .unwrap()
                    .translate_input(&step.to_input_event(&ctx.tempdir.to_string_lossy()))
                    .to_string()
                    .unwrap(),
            )?;

            // Control: the healthy local plugin's decision reaches this agent
            // within its registered deadline before adding an unrelated PM.
            let response = tokio::time::timeout(deadline, ctx.invoke_hook(agent, event, &payload))
                .await
                .expect("the control hook must finish within the agent's deadline")?;
            assert_healthy_plugin_answered(agent, response);

            let script = ctx.tempdir.join("silent-pm.sh");
            let requested = ctx.tempdir.join("pm-requested");
            // Consume requests without replying. `read` blocks in the shell
            // itself, so cancelling the test leaves no sleeping descendant.
            let body = if slow_handshake {
                // Each response takes less than the PM budget, but together
                // initialize and project_info exceed it. The latter must be
                // cancelled before its completion marker can be written.
                r#"while IFS= read -r line; do
  printf requested > "$1"
  id=$(printf '%s' "$line" | sed 's/.*"id":\([0-9]*\),"method".*/\1/')
  sleep 1.2
  case "$line" in
    *'"initialize"'*)
      printf '{"id":%s,"jsonrpc":"2.0","result":{"protocol_version":1,"name":"silent"}}\n' "$id" ;;
    *)
      printf completed > "$1-completed"
      printf '{"id":%s,"jsonrpc":"2.0","result":{"project":{}}}\n' "$id" ;;
  esac
done
"#
            } else {
                "while IFS= read -r line; do printf requested > \"$1\"; done\n"
            };
            std::fs::write(&script, body)?;
            let config_path = ctx.sym.config_dir().join("config.toml");
            let mut config = std::fs::read_to_string(&config_path)?;
            config.push_str(&format!(
                "\n[[package-manager]]\nname = \"silent\"\ncommand = \"/bin/sh\"\nargs = [{:?}, {:?}]\n",
                script.to_string_lossy(),
                requested.to_string_lossy(),
            ));
            for index in 1..pm_count {
                config.push_str(&format!(
                    "\n[[package-manager]]\nname = \"silent-{index}\"\ncommand = \"/bin/sh\"\nargs = [{:?}, {:?}]\n",
                    script.to_string_lossy(), requested.with_file_name(format!("pm-requested-{index}")).to_string_lossy(),
                ));
            }
            std::fs::write(&config_path, config)?;
            ctx.sym = Symposium::from_dir(ctx.sym.config_dir());

            // This outer timeout models the deadline the agent enforces on
            // the whole hook, including discovery of every package manager.
            let response =
                tokio::time::timeout(deadline, ctx.invoke_hook(agent, event, &payload)).await;
            assert!(requested.exists(), "the hook must actually ask the silent PM");
            let response = response.unwrap_or_else(|_| {
                panic!(
                    "{agent:?} reached its registered {deadline:?} hook deadline before the \
                     healthy plugin's decision was returned"
                )
            })?;
            assert_healthy_plugin_answered(agent, response);
            if slow_handshake {
                assert!(!ctx.tempdir.join("pm-requested-completed").exists(), "initialization and later requests must share the budget");
            }
            for index in 1..pm_count {
                assert!(!ctx.tempdir.join(format!("pm-requested-{index}")).exists(), "an exhausted shared budget must not spawn another PM");
            }
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_silent_pm_preserves_codex_hook_output_before_its_deadline() {
    healthy_plugin_answers_despite_a_silent_pm(HookAgent::Codex, 1, false).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_silent_pm_preserves_copilot_hook_output_before_its_deadline() {
    healthy_plugin_answers_despite_a_silent_pm(HookAgent::Copilot, 1, false).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn several_silent_pms_share_one_hook_budget() {
    healthy_plugin_answers_despite_a_silent_pm(HookAgent::Codex, 6, false).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn initialization_and_operations_share_one_hook_budget() {
    healthy_plugin_answers_despite_a_silent_pm(HookAgent::Copilot, 1, true).await;
}
