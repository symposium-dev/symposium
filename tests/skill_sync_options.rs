//! Integration tests for skill syncing with different sync-default configurations.
//!
//! Covers:
//! - Initial sync with no applicable skills → add dep → re-sync → skill appears
//! - sync-default = true (project-level) → skills enabled by default
//! - sync-default = false (project-level) → skills disabled by default
//! - Toggling a skill from false→true populates the skill dir on next sync
//! - Toggling from true→false documents that cleanup is not yet implemented

use expect_test::expect;

fn read_project_config(ctx: &symposium_testlib::TestContext) -> String {
    let path = ctx
        .workspace_root
        .as_ref()
        .unwrap()
        .join(".symposium")
        .join("config.toml");
    std::fs::read_to_string(&path).unwrap_or_else(|_| "(not found)".to_string())
}

fn skill_dir_populated(ctx: &symposium_testlib::TestContext, skill_name: &str) -> bool {
    let root = ctx.workspace_root.as_ref().unwrap();
    root.join(".claude").join("skills").join(skill_name).join("SKILL.md").exists()
}

// -----------------------------------------------------------------------
// Scenario 1: Initial sync with no applicable skills, then add a dep
// -----------------------------------------------------------------------

/// Start with a workspace that has no deps matching any skill plugin.
/// Add serde to Cargo.toml, re-sync, and observe the skill appears
/// with the default value (false, since ProjectConfig defaults sync-default=false).
#[tokio::test]
async fn no_skills_then_add_dep_discovers_skill_default_off() {
    let mut ctx = symposium_testlib::with_fixture(&["plugins0", "workspace-noserde0"]);

    ctx.symposium(&["init", "--user", "--add-agent", "claude"])
        .await
        .unwrap();
    ctx.symposium(&["init", "--project"]).await.unwrap();

    let workspace_root = ctx.workspace_root.clone().unwrap();

    // Initial state: no serde skill discovered (workspace has no serde dep)
    let config = symposium::config::ProjectConfig::load(&workspace_root).unwrap();
    assert!(
        !config.skills.contains_key("serde"),
        "should not discover serde skill without serde dep, got: {:?}",
        config.skills
    );

    expect![[r#"
        sync-default = false
        agent = []
        self-contained = false
        plugin-source = []

        [skills]

        [workflows]
    "#]]
    .assert_eq(&read_project_config(&ctx));

    // Add serde dependency to Cargo.toml
    let cargo_path = workspace_root.join("Cargo.toml");
    let mut cargo = std::fs::read_to_string(&cargo_path).unwrap();
    cargo.push_str("serde = \"1.0\"\n");
    std::fs::write(&cargo_path, &cargo).unwrap();

    // Simulate what the hook does: run sync --workspace
    ctx.symposium(&["sync", "--workspace"]).await.unwrap();

    // Now serde skill should appear, defaulting to false (project sync-default = false)
    let config = symposium::config::ProjectConfig::load(&workspace_root).unwrap();
    assert_eq!(
        config.skills.get("serde"),
        Some(&false),
        "newly discovered skill should default to false"
    );

    // Sync agent — disabled skill should NOT be installed
    ctx.symposium(&["sync", "--agent"]).await.unwrap();
    assert!(
        !skill_dir_populated(&ctx, "serde-guidance"),
        "disabled skill should not be installed"
    );
}

// -----------------------------------------------------------------------
// Scenario 2: sync-default = true at project level
// -----------------------------------------------------------------------

/// When the project config has sync-default = true, newly discovered skills
/// should be enabled by default and installed on sync --agent.
#[tokio::test]
async fn sync_default_true_enables_and_installs_skill() {
    let mut ctx = symposium_testlib::with_fixture(&["plugins0", "workspace-noserde0"]);

    ctx.symposium(&["init", "--user", "--add-agent", "claude"])
        .await
        .unwrap();
    ctx.symposium(&["init", "--project"]).await.unwrap();

    let workspace_root = ctx.workspace_root.clone().unwrap();

    // Set sync-default = true in project config
    let config_path = workspace_root.join(".symposium").join("config.toml");
    let content = std::fs::read_to_string(&config_path).unwrap();
    let content = content.replace("sync-default = false", "sync-default = true");
    std::fs::write(&config_path, &content).unwrap();

    // Add serde dependency
    let cargo_path = workspace_root.join("Cargo.toml");
    let mut cargo = std::fs::read_to_string(&cargo_path).unwrap();
    cargo.push_str("serde = \"1.0\"\n");
    std::fs::write(&cargo_path, &cargo).unwrap();

    // Sync workspace — skill should appear as enabled
    ctx.symposium(&["sync", "--workspace"]).await.unwrap();

    let config = symposium::config::ProjectConfig::load(&workspace_root).unwrap();
    assert_eq!(
        config.skills.get("serde"),
        Some(&true),
        "newly discovered skill should default to true when sync-default = true"
    );

    // Sync agent — enabled skill should be installed
    ctx.symposium(&["sync", "--agent"]).await.unwrap();
    assert!(
        skill_dir_populated(&ctx, "serde-guidance"),
        "enabled skill should be installed"
    );
}

// -----------------------------------------------------------------------
// Scenario 3: Toggle skill false → true → false
// -----------------------------------------------------------------------

/// Start with skill disabled, enable it and sync (skill installed),
/// then disable it and sync again (skill removed).
#[tokio::test]
async fn toggle_skill_false_to_true_installs() {
    let mut ctx = symposium_testlib::with_fixture(&["plugins0", "workspace0"]);

    ctx.symposium(&["init", "--user", "--add-agent", "claude"])
        .await
        .unwrap();
    ctx.symposium(&["init", "--project"]).await.unwrap();

    let workspace_root = ctx.workspace_root.clone().unwrap();
    let config_path = workspace_root.join(".symposium").join("config.toml");

    // Verify skill starts disabled
    let config = symposium::config::ProjectConfig::load(&workspace_root).unwrap();
    assert_eq!(config.skills.get("serde"), Some(&false));

    // Skill should not be installed yet
    ctx.symposium(&["sync", "--agent"]).await.unwrap();
    assert!(
        !skill_dir_populated(&ctx, "serde-guidance"),
        "disabled skill should not be installed"
    );

    // Enable the skill
    let content = std::fs::read_to_string(&config_path).unwrap();
    let content = content.replace("serde = false", "serde = true");
    std::fs::write(&config_path, &content).unwrap();

    // Sync agent — should install the skill
    ctx.symposium(&["sync", "--agent"]).await.unwrap();
    assert!(
        skill_dir_populated(&ctx, "serde-guidance"),
        "enabled skill should be installed after sync"
    );

    // Disable the skill again
    let content = std::fs::read_to_string(&config_path).unwrap();
    let content = content.replace("serde = true", "serde = false");
    std::fs::write(&config_path, &content).unwrap();

    // Sync agent — currently install_skills only installs enabled skills,
    // it does NOT remove previously-installed disabled ones.
    ctx.symposium(&["sync", "--agent"]).await.unwrap();

    // BUG/GAP: the skill directory is still present after disabling.
    // sync --agent should clean up skill dirs for disabled skills.
    let still_present = skill_dir_populated(&ctx, "serde-guidance");
    assert!(
        still_present,
        "skill directory is NOT cleaned up when disabled (known gap — \
         sync --agent only installs, doesn't remove)"
    );
}


// -----------------------------------------------------------------------
// Scenario 4: Hook invocation should trigger skill installation via sync
// -----------------------------------------------------------------------

/// Enable a skill, then invoke a PostToolUse hook (simulating a file write).
/// BUG: execute_hook does not run sync --agent, so the skill is NOT installed.
/// The sync side-effect only lives in the CLI run() path.
#[tokio::test]
async fn hook_does_not_trigger_sync_bug() {
    let mut ctx = symposium_testlib::with_fixture(&["plugins0", "workspace0"]);

    ctx.symposium(&["init", "--user", "--add-agent", "claude"])
        .await
        .unwrap();
    ctx.symposium(&["init", "--project"]).await.unwrap();

    let workspace_root = ctx.workspace_root.clone().unwrap();
    let config_path = workspace_root.join(".symposium").join("config.toml");

    // Skill starts disabled, not installed
    assert!(!skill_dir_populated(&ctx, "serde-guidance"));

    // Enable the skill in config
    let content = std::fs::read_to_string(&config_path).unwrap();
    std::fs::write(&config_path, content.replace("serde = false", "serde = true")).unwrap();

    // Invoke a hook — this SHOULD install the skill, but doesn't because
    // execute_hook() doesn't run the sync side-effect.
    use symposium::hook::HookEvent;
    use symposium::hook_schema::HookAgent;
    ctx.invoke_hook(
        HookAgent::Claude,
        HookEvent::PostToolUse,
        &serde_json::json!({
            "hook_event_name": "PostToolUse",
            "tool_name": "Write",
            "tool_input": {"file_path": "src/main.rs", "content": "fn main() {}"},
            "tool_response": {"success": true},
            "session_id": "s1",
            "cwd": workspace_root.to_string_lossy().to_string(),
        }),
    )
    .await
    .unwrap();

    // BUG: skill is NOT installed because sync only runs in CLI run() path
    assert!(
        !skill_dir_populated(&ctx, "serde-guidance"),
        "BUG: execute_hook does not run sync, so skill is not installed"
    );
}
