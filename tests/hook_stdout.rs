//! A hook's stdout is the agent's protocol: the agent parses all of it, and a
//! single line ahead of the JSON makes it discard the whole output. These
//! tests run the real `cargo agents hook` process, since what leaks is written
//! by the binary around the in-process pipeline.

use serde_json::{Value, json};
use symposium::hook_schema::{HookAgent, agent_event};
use symposium_testlib::{HookStep, TestContext, TestMode, with_fixture};

/// A `SessionStart` that syncs (so sync reports progress) still answers every
/// agent with nothing but its own output: here, the consent hint for `crate-a`.
#[tokio::test(flavor = "multi_thread")]
async fn sync_progress_stays_off_hook_stdout() {
    with_fixture(TestMode::SimulationOnly, &["auto-enable0"], async |ctx| {
        let workspace = ctx.workspace_root.clone().unwrap();
        for agent in [
            HookAgent::Claude,
            HookAgent::Codex,
            HookAgent::Copilot,
            HookAgent::Antigravity,
            HookAgent::Kiro,
            HookAgent::Pi,
        ] {
            let output = run_session_start(&ctx, agent, &workspace);
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(output.status.success(), "{agent:?} exit status");
            assert!(
                stdout.contains("crate-a"),
                "{agent:?} should carry the consent hint, got: {stdout}"
            );
            if agent == HookAgent::Kiro {
                // Kiro takes plain text, all of which becomes context.
                assert!(!stdout.contains("scanning"), "Kiro stdout: {stdout}");
            } else {
                serde_json::from_slice::<Value>(&output.stdout)
                    .unwrap_or_else(|e| panic!("{agent:?} stdout is not JSON ({e}): {stdout}"));
            }
        }
        Ok(())
    })
    .await
    .unwrap();
}

/// A plugin's install commands run on every dispatch of the hook that needs
/// them; what they print must not reach the agent's stdout.
#[tokio::test(flavor = "multi_thread")]
async fn install_command_output_stays_off_hook_stdout() {
    with_fixture(TestMode::SimulationOnly, &["install-noise0"], async |ctx| {
        let output = run_session_start(&ctx, HookAgent::Claude, &ctx.tempdir);
        assert!(output.status.success());
        let stdout: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
            panic!(
                "stdout is not JSON ({e}): {}",
                String::from_utf8_lossy(&output.stdout)
            )
        });
        assert_eq!(
            stdout,
            json!({
                "hookSpecificOutput": {
                    "hookEventName": "SessionStart",
                    "additionalContext": "noisy plugin context",
                }
            })
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("installing noisy tool"));
        Ok(())
    })
    .await
    .unwrap();
}

/// Run `cargo agents hook <agent> session-start` as the agent would, from `cwd`.
fn run_session_start(
    ctx: &TestContext,
    agent: HookAgent,
    cwd: &std::path::Path,
) -> std::process::Output {
    use std::io::Write;

    let step = HookStep::session_start();
    let payload = agent_event(agent, step.event())
        .unwrap()
        .translate_input(&step.to_input_event(&cwd.display().to_string()))
        .to_string()
        .unwrap();

    let binary = std::env::var("CARGO_BIN_EXE_cargo-agents").expect("must run via cargo test");
    let mut child = std::process::Command::new(binary)
        .args(["hook", agent.as_str(), "session-start"])
        .env("SYMPOSIUM_HOME", ctx.sym.config_dir())
        .current_dir(cwd)
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
