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
                    "reloadSkills": true,
                }
            })
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("installing noisy tool"));
        Ok(())
    })
    .await
    .unwrap();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn hook_runs_when_the_logs_dir_is_unwritable() {
    use std::os::unix::fs::PermissionsExt;

    with_fixture(
        TestMode::SimulationOnly,
        &["plugins0", "workspace0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            let workspace = ctx.workspace_root.clone().unwrap();
            let logs = ctx.sym.config_dir().join("logs");
            std::fs::create_dir_all(&logs)?;
            std::fs::set_permissions(&logs, std::fs::Permissions::from_mode(0o500))?;
            let permissions_enforced = std::fs::write(logs.join("probe"), "").is_err();
            let output = run_session_start(&ctx, HookAgent::Claude, &workspace);
            std::fs::set_permissions(&logs, std::fs::Permissions::from_mode(0o700))?;
            if !permissions_enforced {
                return Ok(());
            }

            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(output.status.success(), "stderr: {stderr}");
            assert!(!stderr.contains("panicked"), "stderr: {stderr}");
            serde_json::from_slice::<Value>(&output.stdout).unwrap_or_else(|e| {
                panic!(
                    "stdout is not JSON ({e}): {}",
                    String::from_utf8_lossy(&output.stdout)
                )
            });
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn rejected_skill_warning_stays_off_hook_stdout() {
    with_fixture(
        TestMode::SimulationOnly,
        &["workspace-plugin0"],
        async |mut ctx| {
            let workspace = ctx.workspace_root.clone().unwrap();
            let bad = workspace.join("skills/bad");
            std::fs::create_dir_all(&bad)?;
            std::fs::write(
                bad.join("SKILL.md"),
                "---\nname: \"a/b\"\ndescription: An unsafe name.\n---\n",
            )?;
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;

            let output = run_session_start(&ctx, HookAgent::Claude, &workspace);
            assert!(output.status.success());
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(!stdout.contains("plain directory name"), "stdout: {stdout}");
            serde_json::from_slice::<Value>(&output.stdout)
                .unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {stdout}"));
            assert!(workspace.join(".claude/skills/ws-hello").is_dir());
            Ok(())
        },
    )
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
