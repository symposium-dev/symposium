//! Pi initialization, extension registration, and automatic skill sync.

use symposium::agents::{Agent, McpScope};
use symposium::hook_schema::{HookAgent, HookEvent};
use symposium_testlib::{HookStep, TestMode, with_fixture};

#[test]
fn pi_is_available_in_init_and_uses_shared_skills() {
    assert_eq!(Agent::from_config_name("pi").unwrap(), Agent::Pi);
    assert!(Agent::all().contains(&Agent::Pi));
    assert_eq!(Agent::Pi.config_name(), "pi");
    assert_eq!(Agent::Pi.display_name(), "Pi");
    let root = std::path::Path::new("/project");
    assert_eq!(
        Agent::Pi.project_skill_dir(root, "test"),
        root.join(".agents/skills/test")
    );
    assert_eq!(
        Agent::Pi.global_skill_dir(root, "test"),
        Some(root.join(".agents/skills/test"))
    );
    assert_eq!(serde_json::to_string(&HookAgent::Pi).unwrap(), "\"pi\"");
    assert_eq!(HookAgent::Pi.as_str(), "pi");
    let format: symposium_sdk::manifest::HookFormat = serde_json::from_str("\"pi\"").unwrap();
    assert_eq!(format.as_agent(), Some(HookAgent::Pi));
    assert_eq!(serde_json::to_string(&format).unwrap(), "\"pi\"");
}

#[tokio::test]
async fn pi_stop_returns_empty_output_in_pipeline_and_cli() {
    use std::io::Write;

    with_fixture(
        TestMode::SimulationOnly,
        &["workspace-empty0"],
        async |mut ctx| {
            ctx.sym.config.auto_sync = false;
            ctx.sym.config.auto_update = symposium::config::AutoUpdate::Off;
            ctx.sym.save_config()?;
            let root = ctx.workspace_root.as_ref().unwrap();
            let payload = serde_json::json!({
                "cwd": root,
                "session_id": "test-session",
            });

            let output = ctx
                .invoke_hook(HookAgent::Pi, HookEvent::Stop, &payload)
                .await?;
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&output)?,
                serde_json::json!({})
            );

            let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_cargo-agents"))
                .args(["hook", "pi", "stop"])
                .env("SYMPOSIUM_HOME", ctx.sym.config_dir())
                .current_dir(root)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(&serde_json::to_vec(&payload)?)?;
            let output = child.wait_with_output()?;
            assert!(
                output.status.success(),
                "Pi stop failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&output.stdout)?,
                serde_json::json!({})
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn pi_session_start_installs_skills_automatically() {
    with_fixture(
        TestMode::SimulationOnly,
        &["workspace-plugin0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "pi"]).await?;
            ctx.sym.config.auto_update = symposium::config::AutoUpdate::Off;
            let root = ctx.workspace_root.as_ref().unwrap().clone();
            let global = Agent::Pi.mcp_config_path(McpScope::User, &root, ctx.sym.home_dir());
            let extension = global.parent().unwrap().join("extensions/symposium.ts");
            assert!(extension.exists(), "init installs the Pi extension");
            assert!(!root.join(".agents/skills/ws-hello").exists());
            ctx.prompt_or_hook("hello", &[HookStep::session_start()], HookAgent::Pi)
                .await?;
            let skill = root.join(".agents/skills/ws-hello");
            assert!(
                skill.join("SKILL.md").exists(),
                "session-start auto-sync installs skills"
            );
            assert!(skill.join(".symposium").exists());
            assert_eq!(std::fs::read_to_string(skill.join(".gitignore"))?, "*\n");
            let installed = std::fs::read_to_string(&extension)?;
            assert!(installed.contains("resources_discover"));
            assert!(installed.contains("managedSkillPaths(event.cwd)"));
            ctx.symposium(&["init", "--remove-agent", "pi"]).await?;
            assert!(
                !extension.exists(),
                "removal unregisters the generated extension"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn removing_the_final_project_agent_reaps_its_extension_and_skills() {
    with_fixture(
        TestMode::SimulationOnly,
        &["workspace-plugin0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "pi", "--hook-scope", "project"])
                .await?;
            ctx.symposium(&["sync"]).await?;
            let root = ctx.workspace_root.as_ref().unwrap().clone();
            assert!(root.join(".pi/extensions/symposium.ts").exists());
            assert!(root.join(".agents/skills/ws-hello/SKILL.md").exists());
            let personal = root.join(".agents/skills/personal");
            std::fs::create_dir_all(&personal)?;
            std::fs::write(personal.join("SKILL.md"), "user skill")?;
            ctx.symposium(&["init", "--remove-agent", "pi"]).await?;
            ctx.symposium(&["sync"]).await?;
            assert!(!root.join(".pi/extensions/symposium.ts").exists());
            assert!(!root.join(".agents/skills/ws-hello").exists());
            assert!(personal.join("SKILL.md").exists());
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn pi_scope_changes_leave_one_generated_extension() {
    with_fixture(
        TestMode::SimulationOnly,
        &["workspace-plugin0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "pi"]).await?;
            let root = ctx.workspace_root.as_ref().unwrap().clone();
            let global = Agent::Pi.mcp_config_path(McpScope::User, &root, ctx.sym.home_dir());
            let global = global.parent().unwrap().join("extensions/symposium.ts");
            let project = root.join(".pi/extensions/symposium.ts");
            assert!(global.exists());
            ctx.symposium(&["init", "--hook-scope", "project"]).await?;
            ctx.symposium(&["sync"]).await?;
            assert!(project.exists());
            assert!(!global.exists());
            ctx.symposium(&["init", "--hook-scope", "global"]).await?;
            ctx.symposium(&["sync"]).await?;
            assert!(global.exists());
            assert!(!project.exists());
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn pi_project_scope_and_shared_skill_cleanup() {
    with_fixture(
        TestMode::SimulationOnly,
        &["workspace-plugin0"],
        async |mut ctx| {
            ctx.symposium(&[
                "init",
                "--add-agent",
                "pi",
                "--add-agent",
                "codex",
                "--hook-scope",
                "project",
            ])
            .await?;
            let root = ctx.workspace_root.as_ref().unwrap().clone();
            let extension = root.join(".pi/extensions/symposium.ts");
            assert!(!extension.exists(), "project registration waits for sync");
            ctx.symposium(&["sync"]).await?;
            assert!(extension.exists());
            let skill = root.join(".agents/skills/ws-hello");
            assert!(skill.join("SKILL.md").exists());
            let user_extension = root.join(".pi/extensions/user.ts");
            std::fs::write(&user_extension, "export default function () {}\n")?;
            ctx.symposium(&["init", "--remove-agent", "pi"]).await?;
            ctx.symposium(&["sync"]).await?;
            assert!(!extension.exists());
            assert!(
                user_extension.exists(),
                "other Pi extensions are not removed"
            );
            assert!(
                skill.join("SKILL.md").exists(),
                "Codex still uses the shared skill"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}
