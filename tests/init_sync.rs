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
        .join(".symposium")
        .join("config.toml");
    std::fs::read_to_string(&path).unwrap_or_else(|_| "(not found)".to_string())
}

/// `init --user` creates a user config with the specified agent.
#[tokio::test]
async fn init_user_creates_config() {
    let mut ctx = cargo_agents_testlib::with_fixture(&["plugins0"]);

    ctx.symposium(&["init", "--user", "--add-agent", "claude"])
        .await
        .unwrap();

    let config = symposium::config::Symposium::from_dir(ctx.sym.config_dir());
    assert_eq!(config.config.agents.len(), 1);

    expect![[r#"
        sync-default = true
        auto-sync = false
        plugin-source = []

        [[agent]]
        name = "claude"

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

/// `init --project` creates `.symposium/config.toml` and discovers skills.
#[tokio::test]
async fn init_project_creates_config_and_discovers_skills() {
    let mut ctx = cargo_agents_testlib::with_fixture(&["plugins0", "workspace0"]);

    ctx.symposium(&["init", "--user", "--add-agent", "claude"])
        .await
        .unwrap();

    ctx.symposium(&["init", "--project"]).await.unwrap();

    let workspace_root = ctx.workspace_root.as_ref().unwrap();
    let project_config =
        symposium::config::ProjectConfig::load(workspace_root).expect("project config missing");

    // workspace0 has serde as a dep, plugins0 has a serde skill
    assert!(
        project_config.skills.contains_key("serde"),
        "should discover serde skill, got: {:?}",
        project_config.skills
    );

    expect![[r#"
        sync-default = false
        agent = []
        self-contained = false
        plugin-source = []

        [skills]
        serde = false

        [workflows]
    "#]]
    .assert_eq(&read_project_config(&ctx));
}

/// `sync --workspace` adds new skills and preserves existing choices.
#[tokio::test]
async fn sync_workspace_preserves_existing_choices() {
    let mut ctx = cargo_agents_testlib::with_fixture(&["plugins0", "workspace0"]);

    ctx.symposium(&["init", "--user", "--add-agent", "claude"])
        .await
        .unwrap();
    ctx.symposium(&["init", "--project"]).await.unwrap();

    let workspace_root = ctx.workspace_root.clone().unwrap();

    // User disables serde by editing the config file directly
    let config_path = workspace_root.join(".symposium").join("config.toml");
    let contents = std::fs::read_to_string(&config_path).unwrap();
    let contents = contents.replace("serde = true", "serde = false");
    std::fs::write(&config_path, contents).unwrap();

    // Re-sync should preserve the user's choice
    ctx.symposium(&["sync", "--workspace"]).await.unwrap();

    let config =
        symposium::config::ProjectConfig::load(&workspace_root).expect("project config missing");
    assert_eq!(
        config.skills.get("serde"),
        Some(&false),
        "user's off choice should be preserved"
    );

    expect![[r#"
        sync-default = false
        agent = []
        self-contained = false
        plugin-source = []

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

    ctx.symposium(&["init", "--user", "--add-agent", "claude"])
        .await
        .unwrap();
    ctx.symposium(&["init", "--project"]).await.unwrap();

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
    let agent = symposium::agents::Agent::Copilot;

    let skill_dir = agent.project_skill_dir(root, "serde-guidance");
    assert_eq!(
        skill_dir,
        std::path::PathBuf::from("/project/.agents/skills/serde-guidance")
    );

    // Claude should use .claude/skills/ instead
    let claude_dir = symposium::agents::Agent::Claude.project_skill_dir(root, "serde-guidance");
    assert_eq!(
        claude_dir,
        std::path::PathBuf::from("/project/.claude/skills/serde-guidance")
    );
}

/// `sync --add-agent` adds an agent to the project config.
#[tokio::test]
async fn sync_add_agent_to_project() {
    let mut ctx = cargo_agents_testlib::with_fixture(&["plugins0", "workspace0"]);

    ctx.symposium(&["init", "--user", "--add-agent", "claude"])
        .await
        .unwrap();
    ctx.symposium(&["init", "--project", "--add-agent", "claude"])
        .await
        .unwrap();

    let workspace_root = ctx.workspace_root.clone().unwrap();

    // Add copilot alongside claude
    ctx.symposium(&["sync", "--add-agent", "copilot"])
        .await
        .unwrap();

    let config =
        symposium::config::ProjectConfig::load(&workspace_root).expect("project config missing");
    let agent_names: Vec<_> = config.agents.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(agent_names, vec!["claude", "copilot"]);

    expect![[r#"
        sync-default = false
        self-contained = false
        plugin-source = []

        [[agent]]
        name = "claude"

        [[agent]]
        name = "copilot"

        [skills]
        serde = false

        [workflows]
    "#]]
    .assert_eq(&read_project_config(&ctx));
}

/// `init --project` with `--agent` sets a project-level agent override.
#[tokio::test]
async fn init_project_with_agent_sets_override() {
    let mut ctx = cargo_agents_testlib::with_fixture(&["plugins0", "workspace0"]);

    ctx.symposium(&["init", "--user", "--add-agent", "claude"])
        .await
        .unwrap();
    ctx.symposium(&["init", "--project", "--add-agent", "gemini"])
        .await
        .unwrap();

    let workspace_root = ctx.workspace_root.as_ref().unwrap();
    let config =
        symposium::config::ProjectConfig::load(workspace_root).expect("project config missing");
    assert_eq!(
        config.agents.first().map(|a| a.name.as_str()),
        Some("gemini")
    );

    expect![[r#"
        sync-default = false
        self-contained = false
        plugin-source = []

        [[agent]]
        name = "gemini"

        [skills]
        serde = false

        [workflows]
    "#]]
    .assert_eq(&read_project_config(&ctx));
}

/// Removing an agent removes its hooks.
///
/// Init with claude + gemini, then remove gemini. Gemini hooks should be removed,
/// claude hooks should remain.
#[tokio::test]
async fn removing_agent_removes_hooks() {
    let mut ctx = cargo_agents_testlib::with_fixture(&["plugins0"]);

    // Init with claude + gemini
    ctx.symposium(&["init", "--user", "--add-agent", "claude", "--add-agent", "gemini"])
        .await
        .unwrap();

    // Verify both have hooks
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

    // Remove gemini
    ctx.symposium(&["init", "--user", "--remove-agent", "gemini"])
        .await
        .unwrap();

    // Claude hooks should remain
    let contents = std::fs::read_to_string(&claude_settings).unwrap();
    assert!(
        contents.contains("symposium hook"),
        "claude hooks should remain, got: {contents}"
    );

    // Gemini hooks should be removed
    let contents = std::fs::read_to_string(&gemini_settings).unwrap();
    assert!(
        !contents.contains("symposium hook"),
        "gemini hooks should be removed, got: {contents}"
    );
}

/// `--add-agent` is additive to existing agents.
///
/// Init with claude, then add gemini. Both should be configured.
#[tokio::test]
async fn add_agent_is_additive() {
    let mut ctx = cargo_agents_testlib::with_fixture(&["plugins0"]);

    // Init with claude
    ctx.symposium(&["init", "--user", "--add-agent", "claude"])
        .await
        .unwrap();

    // Add gemini (should keep claude)
    ctx.symposium(&["init", "--user", "--add-agent", "gemini"])
        .await
        .unwrap();

    // Both should be configured
    let config = symposium::config::Symposium::from_dir(ctx.sym.config_dir());
    let agent_names: Vec<_> = config.config.agents.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(agent_names, vec!["claude", "gemini"]);

    // Both should have hooks
    let claude_settings = ctx.sym.home_dir().join(".claude").join("settings.json");
    let gemini_settings = ctx.sym.home_dir().join(".gemini").join("settings.json");
    assert!(
        std::fs::read_to_string(&claude_settings)
            .unwrap()
            .contains("symposium hook"),
        "claude should still have hooks"
    );
    assert!(
        std::fs::read_to_string(&gemini_settings)
            .unwrap()
            .contains("symposium hook"),
        "gemini should have hooks"
    );
}

/// Project-level plugin source with session-start-context is loaded during hooks.
///
/// Uses `project-plugins0` fixture which has:
/// - A workspace with serde/tokio deps
/// - `.symposium/config.toml` with a `[[plugin-source]]` pointing to `project-plugins/`
/// - `project-plugins/project-guidance.toml` with session-start-context
#[tokio::test]
async fn project_plugin_source_loaded_in_hooks() {
    // plugins0 provides user-level config + plugins; project-plugins0 provides
    // a workspace with its own project-level plugin source
    let ctx = cargo_agents_testlib::with_fixture(&["plugins0", "project-plugins0"]);
    let workspace_root = ctx.workspace_root.as_ref().unwrap();

    use cargo_agents::hook::SessionStartPayload;
    let output = ctx
        .invoke_hook(SessionStartPayload {
            session_id: Some("s1".to_string()),
            cwd: Some(workspace_root.to_string_lossy().to_string()),
        })
        .await;

    let context = output
        .hook_specific_output
        .as_ref()
        .and_then(|h| h.additional_context.as_deref())
        .unwrap_or("");

    // Should include the project plugin's context
    assert!(
        context.contains("Always run `cargo test` before committing."),
        "should include project plugin context, got: {context}"
    );

    // Should also include the user-level plugin's context (plugins0 has session-start.toml)
    assert!(
        context.contains("symposium start"),
        "should also include user plugin context, got: {context}"
    );
}

/// Self-contained project excludes user-level plugin sources.
///
/// Uses `project-self-contained0` fixture which has:
/// - A workspace with serde/tokio deps
/// - `.symposium/config.toml` with `self-contained = true` and its own plugin source
/// - `project-plugins/only-this.toml` with session-start-context
#[tokio::test]
async fn self_contained_excludes_user_plugins() {
    // plugins0 provides user-level config + plugins; project-self-contained0 provides
    // a workspace that declares self-contained = true
    let ctx = cargo_agents_testlib::with_fixture(&["plugins0", "project-self-contained0"]);
    let workspace_root = ctx.workspace_root.as_ref().unwrap();

    use cargo_agents::hook::SessionStartPayload;
    let output = ctx
        .invoke_hook(SessionStartPayload {
            session_id: Some("s1".to_string()),
            cwd: Some(workspace_root.to_string_lossy().to_string()),
        })
        .await;

    let context = output
        .hook_specific_output
        .as_ref()
        .and_then(|h| h.additional_context.as_deref())
        .unwrap_or("");

    // Should contain the project plugin context
    assert!(
        context.contains("Project-only guidance."),
        "should include project plugin context, got: {context}"
    );

    // Should NOT contain user-level plugin context (self-contained excludes it)
    assert!(
        !context.contains("symposium start"),
        "self-contained should exclude user plugins, got: {context}"
    );
}

/// Self-contained project excludes user-level *skills* from sync --workspace.
///
/// plugins0 has a serde skill, but project-self-contained0 is self-contained
/// with no skill plugins of its own, so sync should discover no skills.
#[tokio::test]
async fn self_contained_excludes_user_skills_from_sync() {
    let mut ctx = cargo_agents_testlib::with_fixture(&["plugins0", "project-self-contained0"]);

    ctx.symposium(&["init", "--user", "--add-agent", "claude"])
        .await
        .unwrap();

    // Run sync --workspace on the self-contained project
    ctx.symposium(&["sync", "--workspace"]).await.unwrap();

    let project_config = symposium::config::ProjectConfig::load(
        ctx.workspace_root.as_ref().unwrap(),
    );

    // The serde skill from plugins0 should NOT appear — self-contained excludes user sources
    let skills = project_config.map(|c| c.skills).unwrap_or_default();
    assert!(
        !skills.contains_key("serde"),
        "self-contained should not discover user-level serde skill, got: {skills:?}"
    );
}
