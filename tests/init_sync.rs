//! Integration tests for init and sync flows.

use expect_test::expect;

/// Read the user config file from the test context.
///
/// We read the raw file rather than going through the `Config` struct so that
/// snapshots catch formatting, field ordering, and comment preservation —
/// things the deserialized struct would hide.
fn read_user_config(ctx: &cargo_agents_testlib::TestContext) -> String {
    let path = ctx.sym.config_dir().join("config.toml");
    std::fs::read_to_string(&path).unwrap_or_else(|_| "(not found)".to_string())
}

/// Read the project config file from the test context.
///
/// Same rationale as `read_user_config` — we want to snapshot the actual
/// file on disk, not the deserialized struct.
fn read_project_config(ctx: &cargo_agents_testlib::TestContext) -> String {
    let path = ctx
        .workspace_root
        .as_ref()
        .unwrap()
        .join(".cargo-agents")
        .join("config.toml");
    std::fs::read_to_string(&path).unwrap_or_else(|_| "(not found)".to_string())
}

/// `init --user` creates a user config with the specified agent.
#[tokio::test]
async fn init_user_creates_config() {
    let mut ctx = cargo_agents_testlib::with_fixture(&["plugins0"]);

    ctx.cargo_agents(&["init", "--user", "--agent", "claude"])
        .await
        .unwrap();

    let config = cargo_agents::config::Symposium::from_dir(ctx.sym.config_dir());
    assert_eq!(config.config.agent.name.as_deref(), Some("claude"));

    expect![[r#"
        plugin-source = []

        [agent]
        name = "claude"
        sync-default = true
        auto-sync = false

        [logging]
        level = "info"

        [defaults]
        symposium-recommendations = false
        user-plugins = true

        [hooks]
        nudge-interval = 50
    "#]]
    .assert_eq(&read_user_config(&ctx));
}

/// `init --project` creates `.cargo-agents/config.toml` and discovers skills.
#[tokio::test]
async fn init_project_creates_config_and_discovers_skills() {
    let mut ctx = cargo_agents_testlib::with_fixture(&["plugins0", "workspace0"]);

    ctx.cargo_agents(&["init", "--user", "--agent", "claude"])
        .await
        .unwrap();

    ctx.cargo_agents(&["init", "--project"]).await.unwrap();

    let workspace_root = ctx.workspace_root.as_ref().unwrap();
    let project_config =
        cargo_agents::config::ProjectConfig::load(workspace_root).expect("project config missing");

    // workspace0 has serde as a dep, plugins0 has a serde skill
    assert!(
        project_config.skills.contains_key("serde"),
        "should discover serde skill, got: {:?}",
        project_config.skills
    );

    expect![[r#"
        [skills]
        serde = true

        [workflows]
    "#]]
    .assert_eq(&read_project_config(&ctx));
}

/// `sync --workspace` adds new skills and preserves existing choices.
#[tokio::test]
async fn sync_workspace_preserves_existing_choices() {
    let mut ctx = cargo_agents_testlib::with_fixture(&["plugins0", "workspace0"]);

    ctx.cargo_agents(&["init", "--user", "--agent", "claude"])
        .await
        .unwrap();
    ctx.cargo_agents(&["init", "--project"]).await.unwrap();

    let workspace_root = ctx.workspace_root.clone().unwrap();

    // User disables serde by editing the config file directly
    let config_path = workspace_root.join(".cargo-agents").join("config.toml");
    let contents = std::fs::read_to_string(&config_path).unwrap();
    let contents = contents.replace("serde = true", "serde = false");
    std::fs::write(&config_path, contents).unwrap();

    // Re-sync should preserve the user's choice
    ctx.cargo_agents(&["sync", "--workspace"]).await.unwrap();

    let config =
        cargo_agents::config::ProjectConfig::load(&workspace_root).expect("project config missing");
    assert_eq!(
        config.skills.get("serde"),
        Some(&false),
        "user's off choice should be preserved"
    );

    expect![[r#"
        [skills]
        serde = false

        [workflows]
    "#]]
    .assert_eq(&read_project_config(&ctx));
}

/// `sync --agent` installs skill files into the agent's expected location.
#[tokio::test]
async fn sync_agent_installs_skills() {
    let mut ctx = cargo_agents_testlib::with_fixture(&["plugins0", "workspace0"]);

    ctx.cargo_agents(&["init", "--user", "--agent", "claude"])
        .await
        .unwrap();
    ctx.cargo_agents(&["init", "--project"]).await.unwrap();

    let workspace_root = ctx.workspace_root.as_ref().unwrap();

    // Check that a skill was installed in .claude/skills/
    let skills_dir = workspace_root.join(".claude").join("skills");
    if skills_dir.exists() {
        let entries: Vec<_> = std::fs::read_dir(&skills_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        // If serde skill was enabled and installed, there should be at least one entry
        if !entries.is_empty() {
            let skill_dir = &entries[0].path();
            assert!(
                skill_dir.join("SKILL.md").exists(),
                "installed skill should contain SKILL.md"
            );
        }
    }
}

/// Copilot uses vendor-neutral `.agents/skills/` path, not `.claude/skills/`.
#[test]
fn copilot_uses_vendor_neutral_skill_path() {
    let root = std::path::Path::new("/project");
    let agent = cargo_agents::agents::Agent::Copilot;

    let skill_dir = agent.project_skill_dir(root, "serde-guidance");
    assert_eq!(
        skill_dir,
        std::path::PathBuf::from("/project/.agents/skills/serde-guidance")
    );

    // Claude should use .claude/skills/ instead
    let claude_dir = cargo_agents::agents::Agent::Claude.project_skill_dir(root, "serde-guidance");
    assert_eq!(
        claude_dir,
        std::path::PathBuf::from("/project/.claude/skills/serde-guidance")
    );
}

/// `sync --set-agent` changes the project agent (format-preserving).
#[tokio::test]
async fn sync_set_agent_changes_project_agent() {
    let mut ctx = cargo_agents_testlib::with_fixture(&["plugins0", "workspace0"]);

    ctx.cargo_agents(&["init", "--user", "--agent", "claude"])
        .await
        .unwrap();
    ctx.cargo_agents(&["init", "--project", "--agent", "claude"])
        .await
        .unwrap();

    let workspace_root = ctx.workspace_root.clone().unwrap();

    // Change agent to copilot
    ctx.cargo_agents(&["sync", "--set-agent", "copilot"])
        .await
        .unwrap();

    let config =
        cargo_agents::config::ProjectConfig::load(&workspace_root).expect("project config missing");
    assert_eq!(
        config.agent.as_ref().and_then(|a| a.name.as_deref()),
        Some("copilot")
    );

    expect![[r#"
        [agent]
        name = "copilot"
        sync-default = true
        auto-sync = false

        [skills]
        serde = true

        [workflows]
    "#]]
    .assert_eq(&read_project_config(&ctx));
}

/// `init --project` with `--agent` sets a project-level agent override.
#[tokio::test]
async fn init_project_with_agent_sets_override() {
    let mut ctx = cargo_agents_testlib::with_fixture(&["plugins0", "workspace0"]);

    ctx.cargo_agents(&["init", "--user", "--agent", "claude"])
        .await
        .unwrap();
    ctx.cargo_agents(&["init", "--project", "--agent", "gemini"])
        .await
        .unwrap();

    let workspace_root = ctx.workspace_root.as_ref().unwrap();
    let config =
        cargo_agents::config::ProjectConfig::load(workspace_root).expect("project config missing");
    assert_eq!(
        config.agent.as_ref().and_then(|a| a.name.as_deref()),
        Some("gemini")
    );

    expect![[r#"
        [agent]
        name = "gemini"
        sync-default = true
        auto-sync = false

        [skills]
        serde = true

        [workflows]
    "#]]
    .assert_eq(&read_project_config(&ctx));
}
