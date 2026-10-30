//! A plugin hook that runs past its time limit is killed and fails open: it
//! contributes nothing, and the hooks after it still run.
//!
//! `a-hang` never finishes; `z-deny` runs after it and denies `DenyProbe`.

use std::io::Write;
use std::process::{Child, Command, Stdio};

use serde_json::{Value, json};
use symposium::hook_schema::{HookAgent, agent_event};
use symposium_testlib::{HookStep, TestContext, TestMode, with_fixture};

const DENY_REASON: &str = "denied by z-deny";

fn spawn_pre_tool_use(ctx: &TestContext, agent: HookAgent, tool_name: &str) -> Child {
    let step = HookStep::PreToolUse {
        tool_name: tool_name.to_string(),
        tool_input: json!({"command": "ls"}),
    };
    let cwd = ctx.tempdir.display().to_string();
    let payload = agent_event(agent, step.event())
        .unwrap()
        .translate_input(&step.to_input_event(&cwd))
        .to_string()
        .unwrap();
    let binary = std::env::var("CARGO_BIN_EXE_cargo-agents").expect("must run via cargo test");
    let mut child = Command::new(binary)
        .args(["hook", agent.as_str(), "pre-tool-use"])
        .env("SYMPOSIUM_HOME", ctx.sym.config_dir())
        .current_dir(&ctx.tempdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn cargo-agents");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.as_bytes())
        .unwrap();
    child
}

fn finish(child: Child) -> Value {
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn hook_setup_respects_the_total_budget() {
    with_fixture(TestMode::SimulationOnly, &["hook-timeout0"], async |ctx| {
        let plugin = ctx.sym.config_dir().join("plugins/a-hang");
        let manifest = std::fs::read_to_string(plugin.join("Symposium.toml"))?;
        std::fs::write(
            plugin.join("Symposium.toml"),
            manifest.replace(" }", ", install_commands = [\"sleep 9\"] }"),
        )?;
        std::fs::write(plugin.join("scripts/hang.sh"), "exit 0\n")?;
        Command::new(std::env::var("CARGO_BIN_EXE_cargo-agents")?)
            .arg("--version")
            .output()?;
        let start = std::time::Instant::now();
        finish(spawn_pre_tool_use(
            &ctx,
            HookAgent::Antigravity,
            "DenyProbe",
        ));
        assert!(
            start.elapsed() < std::time::Duration::from_millis(8500),
            "hook setup escaped the eight-second budget: {:?}",
            start.elapsed()
        );
        Ok(())
    })
    .await
    .unwrap();
}

/// The calls run at once, so the test waits for the time limit only once.
#[tokio::test(flavor = "multi_thread")]
async fn timed_out_hook_fails_open() {
    with_fixture(TestMode::SimulationOnly, &["hook-timeout0"], async |ctx| {
        let claude_deny = spawn_pre_tool_use(&ctx, HookAgent::Claude, "DenyProbe");
        let antigravity_deny = spawn_pre_tool_use(&ctx, HookAgent::Antigravity, "DenyProbe");
        let antigravity_allow = spawn_pre_tool_use(&ctx, HookAgent::Antigravity, "ReadProbe");
        let copilot_allow = spawn_pre_tool_use(&ctx, HookAgent::Copilot, "ReadProbe");

        assert_eq!(
            finish(claude_deny),
            json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "deny",
                    "permissionDecisionReason": DENY_REASON,
                }
            })
        );
        assert_eq!(
            finish(antigravity_deny),
            json!({"decision": "deny", "reason": DENY_REASON})
        );
        assert_eq!(finish(antigravity_allow), json!({"decision": "allow"}));
        assert_eq!(finish(copilot_allow), json!({}));
        Ok(())
    })
    .await
    .unwrap();
}
