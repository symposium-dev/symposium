//! A failed config read must never be interpreted as uninstalling every agent.

use std::fs;

use serde_json::json;
use symposium::agents::{Agent, McpScope};
use symposium::config::{AutoUpdate, Symposium};
use symposium::hook_schema::{HookAgent, HookEvent};
use symposium::output::Output;
use symposium::workspace_state::WorkspaceState;
use symposium_testlib::{TestMode, with_fixture};

#[tokio::test]
async fn failed_config_load_preserves_installations_and_recovers_after_repair() {
    let corruptions: [(&str, Option<&[u8]>); 4] = [
        ("malformed TOML", Some(b"[[agent")),
        ("invalid field type", Some(b"agent = 42")),
        ("invalid UTF-8", Some(b"\xff")),
        // A directory gives a portable read error, even when tests run as root.
        ("unreadable file", None),
    ];
    for (agent, hook_agent) in [
        (Agent::Claude, HookAgent::Claude),
        (Agent::Pi, HookAgent::Pi),
    ] {
        for scope in ["project", "global"] {
            for (damage, contents) in corruptions {
                with_fixture(
                    TestMode::SimulationOnly,
                    &["mcp-filtering0", "workspace-plugin0"],
                    async |mut ctx| {
                        ctx.symposium(&[
                            "init", "--add-agent", agent.config_name(), "--hook-scope", scope,
                        ])
                        .await?;
                        ctx.symposium(&["sync"]).await?;
                        let root = ctx.workspace_root.as_ref().unwrap().clone();
                        let config_dir = ctx.sym.config_dir().to_path_buf();
                        let config_path = config_dir.join("config.toml");
                        let valid_config = fs::read(&config_path)?;
                        let hook_root = if scope == "global" { &config_dir } else { &root };
                        let hook_path = match (agent, scope) {
                            (Agent::Claude, _) => hook_root.join(".claude/settings.json"),
                            (Agent::Pi, "global") => hook_root.join(".pi/agent/extensions/symposium.ts"),
                            (Agent::Pi, _) => hook_root.join(".pi/extensions/symposium.ts"),
                            _ => unreachable!(),
                        };
                        let mcp_scope = if scope == "global" { McpScope::User } else { McpScope::Project };
                        let mcp_path = agent.mcp_config_path(mcp_scope, &root, &ctx.sym);
                        let skill = agent.project_skill_dir(&root, "ws-hello");
                        let personal = agent.project_skill_dir(&root, "personal");
                        fs::create_dir_all(&personal)?;
                        fs::write(personal.join("SKILL.md"), "user-managed skill")?;
                        let paths = [
                            hook_path.clone(), mcp_path.clone(), skill.join("SKILL.md"),
                            skill.join(".symposium"), personal.join("SKILL.md"),
                        ];
                        let snapshots = paths.iter().map(fs::read).collect::<Result<Vec<_>, _>>()?;

                        fs::remove_file(&config_path)?;
                        if let Some(contents) = contents {
                            fs::write(&config_path, contents)?;
                        } else {
                            fs::create_dir(&config_path)?;
                        }
                        ctx.sym = Symposium::from_dir(&config_dir);
                        // No update checks during the in-process hook test.
                        ctx.sym.config.auto_update = AutoUpdate::Off;
                        for _ in 0..2 {
                            let error = ctx.symposium(&["sync"]).await.expect_err(damage);
                            assert!(format!("{error:#}").contains("config.toml"));
                        }
                        assert!(symposium::sync::register_hooks(&ctx.sym, &Output::quiet()).await.is_err());
                        assert!(ctx.symposium(&["init", "--remove-agent", agent.config_name()]).await.is_err());
                        assert!(ctx.sym.save_config().is_err());

                        // Failed auto-sync must not block hooks or mark sync as fresh.
                        assert!(!WorkspaceState::load(&ctx.sym, &root).sync_is_fresh(&root));
                        let payload = json!({
                            "cwd": root, "session_id": "config-recovery", "hook_event_name": "SessionStart",
                        });
                        let response = ctx.invoke_hook(hook_agent, HookEvent::SessionStart, &payload).await?;
                        assert_eq!(response.exit_code, 0);
                        assert!(!WorkspaceState::load(&ctx.sym, &root).sync_is_fresh(&root));
                        for (path, before) in paths.iter().zip(&snapshots) {
                            assert_eq!(&fs::read(path)?, before, "{damage}: {}", path.display());
                        }
                        if let Some(contents) = contents {
                            assert_eq!(fs::read(&config_path)?, contents);
                        } else {
                            assert!(config_path.is_dir());
                            fs::remove_dir(&config_path)?;
                        }

                        // A repaired config resumes sync on the next invocation.
                        fs::write(&config_path, valid_config)?;
                        ctx.sym = Symposium::from_dir(&config_dir);
                        ctx.sym.config.auto_update = AutoUpdate::Off;
                        ctx.invoke_hook(hook_agent, HookEvent::SessionStart, &payload).await?;
                        assert!(WorkspaceState::load(&ctx.sym, &root).sync_is_fresh(&root));
                        for (path, before) in paths.iter().zip(&snapshots) {
                            assert_eq!(&fs::read(path)?, before);
                        }

                        // Valid removal still cleans managed resources, not personal skills.
                        ctx.symposium(&["init", "--remove-agent", agent.config_name()]).await?;
                        ctx.symposium(&["sync"]).await?;
                        assert!(!skill.exists());
                        if mcp_path.exists() {
                            let mcp: serde_json::Value = serde_json::from_slice(&fs::read(&mcp_path)?)?;
                            assert!(mcp["mcpServers"].get("always-server").is_none());
                        }
                        assert_eq!(fs::read_to_string(personal.join("SKILL.md"))?, "user-managed skill");
                        if agent == Agent::Pi {
                            assert!(!hook_path.exists());
                        } else {
                            assert!(!fs::read_to_string(&hook_path)?.contains("cargo-agents"));
                        }
                        Ok(())
                    },
                )
                .await
                .unwrap();
            }
        }
    }
}
