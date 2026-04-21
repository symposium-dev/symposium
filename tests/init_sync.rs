//! Integration tests for init and sync flows.

use symposium_testlib::{TestMode, with_fixture};

/// Read the user config file from the test context.
fn read_user_config(ctx: &symposium_testlib::TestContext) -> String {
    let path = ctx.sym.config_dir().join("config.toml");
    std::fs::read_to_string(&path).unwrap_or_else(|_| "(not found)".to_string())
}

/// `init` defaults to global hook scope — hooks go to home dir.
#[tokio::test]
async fn init_defaults_to_global_hooks() {
    with_fixture(TestMode::SimulationOnly, &[], async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "claude"]).await?;

        let content = read_user_config(&ctx);

        // `hook-scope` is not found because it is the default (global) and
        // hence not serialized.
        assert!(!content.contains("hook-scope"));

        let settings = ctx.sym.home_dir().join(".claude").join("settings.json");
        assert!(settings.exists(), "global hooks should be installed");
        Ok(())
    })
    .await
    .unwrap();
}

/// `init --hook-scope project` writes the setting but does NOT install hooks.
#[tokio::test]
async fn init_hook_scope_project() {
    with_fixture(TestMode::SimulationOnly, &[], async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "claude", "--hook-scope", "project"])
            .await?;

        let content = read_user_config(&ctx);

        // `hook-scope` is serialized to be the project.
        assert!(content.contains("hook-scope = \"project\""));

        let settings = ctx.sym.home_dir().join(".claude").join("settings.json");
        assert!(
            !settings.exists(),
            "project-scope should not install global hooks"
        );
        Ok(())
    })
    .await
    .unwrap();
}

/// `init` preserves existing hook-scope when no flag is given.
#[tokio::test]
async fn init_preserves_existing_hook_scope() {
    with_fixture(TestMode::SimulationOnly, &[], async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "claude", "--hook-scope", "project"])
            .await?;
        ctx.symposium(&["init", "--add-agent", "gemini"]).await?;

        let content = read_user_config(&ctx);
        assert!(
            content.contains("hook-scope = \"project\""),
            "should preserve project scope"
        );
        Ok(())
    })
    .await
    .unwrap();
}

/// `init` creates a user config with the specified agent.
#[tokio::test]
async fn init_creates_config() {
    with_fixture(TestMode::SimulationOnly, &["plugins0"], async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "claude"]).await?;

        let config = symposium::config::Symposium::from_dir(ctx.sym.config_dir());
        assert_eq!(config.config.agents.len(), 1);

        let content = read_user_config(&ctx);
        assert!(content.contains(r#"name = "claude""#));
        assert!(!content.contains("sync-default"));
        Ok(())
    })
    .await
    .unwrap();
}

/// `sync` installs skill files into the agent's expected location.
#[tokio::test]
async fn sync_installs_skills() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugins0", "workspace0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.as_ref().unwrap();

            let skill_file = workspace_root.join(".claude/skills/serde-guidance/SKILL.md");
            assert!(
                skill_file.exists(),
                "sync should install serde-guidance skill"
            );

            let manifest_path = workspace_root.join(".claude/skills/.symposium.toml");
            assert!(manifest_path.exists(), "manifest should be written");
            let manifest = std::fs::read_to_string(&manifest_path).unwrap();
            assert!(
                manifest.contains("serde-guidance"),
                "manifest should track installed skill"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// `sync` removes stale skills tracked in the manifest.
#[tokio::test]
async fn sync_removes_stale_skills() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugins0", "workspace0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.as_ref().unwrap();

            let manifest_path = workspace_root.join(".claude/skills/.symposium.toml");
            let manifest = std::fs::read_to_string(&manifest_path).unwrap();
            let manifest = manifest.replace(
                r#"installed = ["#,
                r#"installed = [
    "fake-old-skill","#,
            );
            std::fs::write(&manifest_path, &manifest).unwrap();

            let fake_dir = workspace_root.join(".claude/skills/fake-old-skill");
            std::fs::create_dir_all(&fake_dir).unwrap();
            std::fs::write(fake_dir.join("SKILL.md"), "old").unwrap();

            ctx.symposium(&["sync"]).await?;

            assert!(
                !fake_dir.exists(),
                "stale skill should be removed after sync"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// `sync` does not touch skills not in the manifest (user-managed).
#[tokio::test]
async fn sync_preserves_user_managed_skills() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugins0", "workspace0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.as_ref().unwrap();

            let user_skill_dir = workspace_root.join(".claude/skills/my-custom-skill");
            std::fs::create_dir_all(&user_skill_dir).unwrap();
            std::fs::write(user_skill_dir.join("SKILL.md"), "custom").unwrap();

            ctx.symposium(&["sync"]).await?;

            assert!(
                user_skill_dir.join("SKILL.md").exists(),
                "user-managed skill should not be removed"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// Copilot uses vendor-neutral `.agents/skills/` path, not `.claude/skills/`.
#[test]
fn copilot_uses_vendor_neutral_skill_path() {
    let root = std::path::Path::new("/project");
    let agent = symposium::agents::Agent::Copilot;

    let skill_dir = agent.project_skill_dir(root, "serde-guidance");
    assert_eq!(
        skill_dir,
        std::path::PathBuf::from("/project/.agents/skills/serde-guidance")
    );

    let claude_dir = symposium::agents::Agent::Claude.project_skill_dir(root, "serde-guidance");
    assert_eq!(
        claude_dir,
        std::path::PathBuf::from("/project/.claude/skills/serde-guidance")
    );
}

/// Removing an agent removes its hooks.
#[tokio::test]
async fn removing_agent_removes_hooks() {
    with_fixture(TestMode::SimulationOnly, &["plugins0"], async |mut ctx| {
        ctx.symposium(&[
            "init",
            "--hook-scope",
            "global",
            "--add-agent",
            "claude",
            "--add-agent",
            "gemini",
        ])
        .await?;

        let claude_settings = ctx.sym.home_dir().join(".claude/settings.json");
        let gemini_settings = ctx.sym.home_dir().join(".gemini/settings.json");
        assert!(claude_settings.exists(), "claude settings should exist");
        assert!(gemini_settings.exists(), "gemini settings should exist");
        assert!(
            std::fs::read_to_string(&gemini_settings)
                .unwrap()
                .contains("cargo-agents hook"),
            "gemini should have symposium hooks"
        );

        ctx.symposium(&["init", "--hook-scope", "global", "--remove-agent", "gemini"])
            .await?;

        let contents = std::fs::read_to_string(&claude_settings).unwrap();
        assert!(
            contents.contains("cargo-agents hook"),
            "claude hooks should remain"
        );

        let contents = std::fs::read_to_string(&gemini_settings).unwrap();
        assert!(
            !contents.contains("cargo-agents hook"),
            "gemini hooks should be removed"
        );
        Ok(())
    })
    .await
    .unwrap();
}

/// `--add-agent` is additive to existing agents.
#[tokio::test]
async fn add_agent_is_additive() {
    with_fixture(TestMode::SimulationOnly, &["plugins0"], async |mut ctx| {
        ctx.symposium(&["init", "--hook-scope", "global", "--add-agent", "claude"])
            .await?;
        ctx.symposium(&["init", "--hook-scope", "global", "--add-agent", "gemini"])
            .await?;

        let config = symposium::config::Symposium::from_dir(ctx.sym.config_dir());
        let agent_names: Vec<_> = config
            .config
            .agents
            .iter()
            .map(|a| a.name.as_str())
            .collect();
        assert_eq!(agent_names, vec!["claude", "gemini"]);

        let claude_settings = ctx.sym.home_dir().join(".claude/settings.json");
        let gemini_settings = ctx.sym.home_dir().join(".gemini/settings.json");
        assert!(
            std::fs::read_to_string(&claude_settings)
                .unwrap()
                .contains("cargo-agents hook")
        );
        assert!(
            std::fs::read_to_string(&gemini_settings)
                .unwrap()
                .contains("cargo-agents hook")
        );
        Ok(())
    })
    .await
    .unwrap();
}

/// `sync` filters MCP servers by their `crates` predicates.
#[tokio::test]
async fn sync_filters_mcp_servers_by_crates() {
    with_fixture(
        TestMode::SimulationOnly,
        &["mcp-filtering0", "workspace0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.as_ref().unwrap();
            let settings_path = workspace_root.join(".claude/settings.json");
            let settings = std::fs::read_to_string(&settings_path)?;

            // always-server (crates = ["*"]) → registered
            assert!(
                settings.contains("always-server"),
                "wildcard MCP server should be registered"
            );
            // serde-server (crates = ["serde"]) → registered (serde is in workspace0)
            assert!(
                settings.contains("serde-server"),
                "serde MCP server should be registered"
            );
            // inherited-server (no crates, inherits from plugin) → registered
            assert!(
                settings.contains("inherited-server"),
                "inherited MCP server should be registered"
            );
            // missing-crate-server (crates = ["reqwest"]) → NOT registered
            assert!(
                !settings.contains("missing-crate-server"),
                "reqwest MCP server should NOT be registered"
            );

            Ok(())
        },
    )
    .await
    .unwrap();
}

/// `sync` does not install skills targeting transitive dependencies.
/// workspace0 has tokio as a direct dep; mio is a transitive dep of tokio.
#[tokio::test]
async fn sync_excludes_transitive_deps() {
    with_fixture(
        TestMode::SimulationOnly,
        &["transitive-dep0", "workspace0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.as_ref().unwrap();

            // mio-guidance should NOT be installed (mio is transitive, not direct)
            let mio_skill = workspace_root.join(".claude/skills/mio-guidance/SKILL.md");
            assert!(
                !mio_skill.exists(),
                "skill targeting transitive dep (mio) should NOT be installed"
            );

            Ok(())
        },
    )
    .await
    .unwrap();
}

/// `sync` installs skills defined inside a plugin's `[[skills]]` group
/// with `source.path = "."`. The skill directory should resolve relative
/// to the plugin's parent directory, not the TOML file path itself.
#[tokio::test]
async fn sync_installs_plugin_skill_group() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-skill-group0", "workspace0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.as_ref().unwrap();

            let skill_file = workspace_root.join(".claude/skills/serde-guidance/SKILL.md");
            assert!(
                skill_file.exists(),
                "sync should install skill from plugin [[skills]] group with source.path"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// `sync` installs skills from a plugin with `crates = ["*"]`.
/// Wildcard predicates should match any workspace.
#[tokio::test]
async fn sync_installs_wildcard_plugin_skill() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-skill-group0", "workspace0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.as_ref().unwrap();

            let skill_file = workspace_root.join(".claude/skills/wildcard-guidance/SKILL.md");
            assert!(
                skill_file.exists(),
                "sync should install skill from wildcard plugin"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}
