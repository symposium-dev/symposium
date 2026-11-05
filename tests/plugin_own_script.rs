//! A plugin's own hook scripts, named relative to its manifest.

use serde_json::json;
use symposium::hook_schema::HookAgent;
use symposium_testlib::{HookStep, TestMode, with_fixture};

#[tokio::test(flavor = "multi_thread")]
async fn scripts_named_relative_to_the_manifest_run() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-own-script0"],
        async |mut ctx| {
            let result = ctx
                .prompt_or_hook(
                    "ignored",
                    &[
                        HookStep::PreToolUse {
                            tool_name: "Read".to_string(),
                            tool_input: json!({"file_path": "/tmp/x"}),
                        },
                        HookStep::PreToolUse {
                            tool_name: "Bash".to_string(),
                            tool_input: json!({"command": "true"}),
                        },
                    ],
                    HookAgent::Claude,
                )
                .await?;

            for marker in ["own-script-fired", "beside-script-fired"] {
                assert!(
                    result.has_context_containing(marker),
                    "expected the plugin's own script to report {marker}, got: {result:#?}",
                );
            }
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn scripts_run_under_a_relative_symposium_home_from_another_workspace() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-own-script0"],
        async |ctx| {
            use std::io::Write;

            let config_dir = ctx.sym.config_dir();
            let started_in = config_dir.parent().unwrap();
            let relative_home = config_dir.file_name().unwrap();
            let workspace = ctx.tempdir.join("elsewhere");
            std::fs::create_dir_all(&workspace)?;
            let payload = serde_json::to_vec(&json!({
                "session_id": "s",
                "cwd": workspace,
                "hook_event_name": "PreToolUse",
                "tool_name": "Read",
                "tool_input": {"file_path": "/tmp/x"},
            }))?;

            let binary = std::env::var("CARGO_BIN_EXE_cargo-agents")?;
            let mut child = std::process::Command::new(binary)
                .args(["hook", "claude", "pre-tool-use"])
                .env("SYMPOSIUM_HOME", relative_home)
                .current_dir(started_in)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?;
            child.stdin.take().unwrap().write_all(&payload)?;
            let output = child.wait_with_output()?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                stdout.contains("own-script-fired"),
                "stdout: {stdout}\nstderr: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}
