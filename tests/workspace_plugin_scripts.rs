//! Workspace-root and member plugins resolve identical script names separately.

use std::path::Path;

use serde_json::json;
use symposium::hook_schema::HookAgent;
use symposium_testlib::{HookStep, TestMode, with_fixture};

fn write_plugin(directory: &Path, name: &str, tool: &str, marker: &str) -> anyhow::Result<()> {
    std::fs::create_dir_all(directory.join("scripts"))?;
    std::fs::write(
        directory.join("Symposium.toml"),
        format!(
            r#"name = "{name}"

[[hooks]]
name = "own-script"
event = "PreToolUse"
matcher = "{tool}"
format = "claude"
command = {{ script = "scripts/probe.sh" }}
"#,
        ),
    )?;
    let output = json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "additionalContext": marker,
        },
    });
    std::fs::write(
        directory.join("scripts/probe.sh"),
        format!("#!/bin/sh\nprintf '%s\\n' '{output}'\n"),
    )?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn workspace_root_and_member_run_their_own_relative_scripts() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-own-script0"],
        async |mut ctx| {
            let workspace = ctx.tempdir.join("workspace");
            let member = workspace.join("member");
            std::fs::create_dir_all(member.join("src"))?;
            std::fs::write(
                workspace.join("Cargo.toml"),
                "[workspace]\nmembers = [\"member\"]\nresolver = \"2\"\n",
            )?;
            std::fs::write(
                member.join("Cargo.toml"),
                "[package]\nname = \"script-member\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            )?;
            std::fs::write(member.join("src/lib.rs"), "")?;
            write_plugin(&workspace, "root-plugin", "RootProbe", "root-script-fired")?;
            write_plugin(
                &member,
                "member-plugin",
                "MemberProbe",
                "member-script-fired",
            )?;
            ctx.workspace_root = Some(workspace.clone());

            // The simulation sets the event's cwd but leaves this process in
            // the repository. Neither script can work through a cwd fallback.
            let process_cwd = std::fs::canonicalize(std::env::current_dir()?)?;
            assert_ne!(process_cwd, std::fs::canonicalize(&workspace)?);
            assert_ne!(process_cwd, std::fs::canonicalize(&member)?);

            let result = ctx
                .prompt_or_hook(
                    "ignored",
                    &[
                        HookStep::PreToolUse {
                            tool_name: "RootProbe".into(),
                            tool_input: json!({}),
                        },
                        HookStep::PreToolUse {
                            tool_name: "MemberProbe".into(),
                            tool_input: json!({}),
                        },
                    ],
                    HookAgent::Claude,
                )
                .await?;

            assert_eq!(result.hooks.len(), 2);
            for (hook, marker) in result
                .hooks
                .iter()
                .zip(["root-script-fired", "member-script-fired"])
            {
                assert_eq!(
                    hook.output
                        .pointer("/hookSpecificOutput/additionalContext")
                        .and_then(serde_json::Value::as_str),
                    Some(marker),
                    "each hook must run the script beside its own manifest: {hook:#?}",
                );
            }
            Ok(())
        },
    )
    .await
    .unwrap();
}
