//! Integration tests for init and sync flows.

/// Read the user config file from the test context.
fn read_user_config(ctx: &symposium_testlib::TestContext) -> String {
    let path = ctx.sym.config_dir().join("config.toml");
    std::fs::read_to_string(&path).unwrap_or_else(|_| "(not found)".to_string())
}

/// `init` creates a user config with the specified agent.
#[tokio::test]
async fn init_creates_config() {
    let mut ctx = symposium_testlib::with_fixture(&["plugins0"]);

    ctx.symposium(&["init", "--add-agent", "claude"])
        .await
        .unwrap();

    let config = symposium::config::Symposium::from_dir(ctx.sym.config_dir());
    assert_eq!(config.config.agents.len(), 1);

    let content = read_user_config(&ctx);
    assert!(content.contains(r#"name = "claude""#));
    assert!(!content.contains("sync-default"));
}

/// `sync` installs skill files into the agent's expected location.
#[tokio::test]
async fn sync_installs_skills() {
    let mut ctx = symposium_testlib::with_fixture(&["plugins0", "workspace0"]);

    ctx.symposium(&["init", "--add-agent", "claude"])
        .await
        .unwrap();

    ctx.symposium(&["sync"]).await.unwrap();

    let workspace_root = ctx.workspace_root.as_ref().unwrap();

    // Check that a skill was installed in .claude/skills/
    let skill_file = workspace_root
        .join(".claude")
        .join("skills")
        .join("serde-guidance")
        .join("SKILL.md");
    assert!(
        skill_file.exists(),
        "sync should install serde-guidance skill"
    );

    // Check manifest was written
    let manifest_path = workspace_root
        .join(".claude")
        .join("skills")
        .join(".symposium.toml");
    assert!(manifest_path.exists(), "manifest should be written");
    let manifest = std::fs::read_to_string(&manifest_path).unwrap();
    assert!(
        manifest.contains("serde-guidance"),
        "manifest should track installed skill"
    );
}

/// `sync` removes stale skills tracked in the manifest.
#[tokio::test]
async fn sync_removes_stale_skills() {
    let mut ctx = symposium_testlib::with_fixture(&["plugins0", "workspace0"]);

    ctx.symposium(&["init", "--add-agent", "claude"])
        .await
        .unwrap();
    ctx.symposium(&["sync"]).await.unwrap();

    let workspace_root = ctx.workspace_root.as_ref().unwrap();

    // Manually add a fake skill to the manifest
    let manifest_path = workspace_root
        .join(".claude")
        .join("skills")
        .join(".symposium.toml");
    let manifest = std::fs::read_to_string(&manifest_path).unwrap();
    let manifest = manifest.replace(
        r#"installed = ["#,
        r#"installed = [
    "fake-old-skill","#,
    );
    std::fs::write(&manifest_path, &manifest).unwrap();

    // Create the fake skill dir
    let fake_dir = workspace_root
        .join(".claude")
        .join("skills")
        .join("fake-old-skill");
    std::fs::create_dir_all(&fake_dir).unwrap();
    std::fs::write(fake_dir.join("SKILL.md"), "old").unwrap();

    // Re-sync should remove the fake skill
    ctx.symposium(&["sync"]).await.unwrap();

    assert!(
        !fake_dir.exists(),
        "stale skill should be removed after sync"
    );
}

/// `sync` does not touch skills not in the manifest (user-managed).
#[tokio::test]
async fn sync_preserves_user_managed_skills() {
    let mut ctx = symposium_testlib::with_fixture(&["plugins0", "workspace0"]);

    ctx.symposium(&["init", "--add-agent", "claude"])
        .await
        .unwrap();
    ctx.symposium(&["sync"]).await.unwrap();

    let workspace_root = ctx.workspace_root.as_ref().unwrap();

    // Manually add a skill NOT in the manifest
    let user_skill_dir = workspace_root
        .join(".claude")
        .join("skills")
        .join("my-custom-skill");
    std::fs::create_dir_all(&user_skill_dir).unwrap();
    std::fs::write(user_skill_dir.join("SKILL.md"), "custom").unwrap();

    // Re-sync should leave it alone
    ctx.symposium(&["sync"]).await.unwrap();

    assert!(
        user_skill_dir.join("SKILL.md").exists(),
        "user-managed skill should not be removed"
    );
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
    let mut ctx = symposium_testlib::with_fixture(&["plugins0"]);

    ctx.symposium(&["init", "--add-agent", "claude", "--add-agent", "gemini"])
        .await
        .unwrap();

    let claude_settings = ctx.sym.home_dir().join(".claude").join("settings.json");
    let gemini_settings = ctx.sym.home_dir().join(".gemini").join("settings.json");
    assert!(claude_settings.exists(), "claude settings should exist");
    assert!(gemini_settings.exists(), "gemini settings should exist");
    assert!(
        std::fs::read_to_string(&gemini_settings)
            .unwrap()
            .contains("symposium hook"),
        "gemini should have symposium hooks"
    );

    ctx.symposium(&["init", "--remove-agent", "gemini"])
        .await
        .unwrap();

    let contents = std::fs::read_to_string(&claude_settings).unwrap();
    assert!(
        contents.contains("symposium hook"),
        "claude hooks should remain"
    );

    let contents = std::fs::read_to_string(&gemini_settings).unwrap();
    assert!(
        !contents.contains("symposium hook"),
        "gemini hooks should be removed"
    );
}

/// `--add-agent` is additive to existing agents.
#[tokio::test]
async fn add_agent_is_additive() {
    let mut ctx = symposium_testlib::with_fixture(&["plugins0"]);

    ctx.symposium(&["init", "--add-agent", "claude"])
        .await
        .unwrap();

    ctx.symposium(&["init", "--add-agent", "gemini"])
        .await
        .unwrap();

    let config = symposium::config::Symposium::from_dir(ctx.sym.config_dir());
    let agent_names: Vec<_> = config
        .config
        .agents
        .iter()
        .map(|a| a.name.as_str())
        .collect();
    assert_eq!(agent_names, vec!["claude", "gemini"]);

    let claude_settings = ctx.sym.home_dir().join(".claude").join("settings.json");
    let gemini_settings = ctx.sym.home_dir().join(".gemini").join("settings.json");
    assert!(
        std::fs::read_to_string(&claude_settings)
            .unwrap()
            .contains("symposium hook"),
    );
    assert!(
        std::fs::read_to_string(&gemini_settings)
            .unwrap()
            .contains("symposium hook"),
    );
}
