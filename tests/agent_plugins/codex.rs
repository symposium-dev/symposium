use std::path::PathBuf;

use expect_test::expect;
use symposium_testlib::{TestContext, TestMode, find_installed_skill, tree, with_fixture};

const SCOPES: &[&str] = &["agent-plugin-scopes0"];

fn codex_dir(ctx: &TestContext) -> PathBuf {
    ctx.sym.home_dir().join(".codex")
}

fn cache(ctx: &TestContext) -> PathBuf {
    codex_dir(ctx).join("plugins/cache/symposium")
}

fn project_skills(ctx: &TestContext) -> PathBuf {
    ctx.workspace_root.as_ref().unwrap().join(".agents/skills")
}

fn codex_skill_dirs(ctx: &TestContext) -> Vec<PathBuf> {
    let mut dirs = vec![project_skills(ctx)];
    if let Ok(plugins) = std::fs::read_dir(cache(ctx)) {
        dirs.extend(plugins.flatten().map(|plugin| plugin.path()));
    }
    dirs
}

fn plugin_tables(ctx: &TestContext, codex_dir: &std::path::Path) -> String {
    let config: toml::Table =
        toml::from_str(&std::fs::read_to_string(codex_dir.join("config.toml")).unwrap()).unwrap();
    let tables: toml::Table = config
        .into_iter()
        .filter(|(key, _)| key == "marketplaces" || key == "plugins")
        .collect();
    ctx.normalize_json(&serde_json::to_string(&tables).unwrap())
}

#[tokio::test]
async fn a_global_plugin_reaches_codex_as_its_own_plugin_add_installs_it() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        let earlier_copy = project_skills(&ctx).join("global-guide");
        std::fs::create_dir_all(&earlier_copy)?;
        std::fs::write(earlier_copy.join("SKILL.md"), "an earlier per-skill copy")?;
        std::fs::write(earlier_copy.join(".symposium"), "")?;

        ctx.symposium(&["init", "--add-agent", "codex"]).await?;
        ctx.symposium(&["sync"]).await?;

        expect![[r#"
            {
              "marketplaces": {
                "symposium": {
                  "source": "$CONFIG_DIR/installed",
                  "source_type": "local"
                }
              },
              "plugins": {
                "global-guide@symposium": {
                  "enabled": true
                }
              }
            }"#]]
        .assert_eq(&plugin_tables(&ctx, &codex_dir(&ctx)));
        expect![[r#"
            global-guide/
            global-guide/.symposium
            global-guide/0.0.0/
            global-guide/0.0.0/.claude-plugin/
            global-guide/0.0.0/.claude-plugin/plugin.json
            global-guide/0.0.0/.symposium
            global-guide/0.0.0/plugin.json
            global-guide/0.0.0/skills/
            global-guide/0.0.0/skills/global-guide/
            global-guide/0.0.0/skills/global-guide/SKILL.md
            global-guide/0.0.0/skills/global-guide/reference.md"#]]
        .assert_eq(&tree(&cache(&ctx)));

        assert_eq!(
            find_installed_skill(&codex_skill_dirs(&ctx), "global-guide"),
            cache(&ctx).join("global-guide/0.0.0/skills/global-guide")
        );
        for skill in ["workspace-guide", "always-guide"] {
            assert_eq!(
                find_installed_skill(&codex_skill_dirs(&ctx), skill),
                project_skills(&ctx).join(skill)
            );
        }
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn editing_a_global_plugins_skill_updates_codexs_copy() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "codex"]).await?;
        ctx.symposium(&["sync"]).await?;

        let source = ctx.sym.config_dir().join("plugins/global-guide/SKILL.md");
        let edited = std::fs::read_to_string(&source)?.replace("Global guidance.", "Edited.");
        std::fs::write(&source, &edited)?;
        ctx.symposium(&["sync"]).await?;

        assert_eq!(
            std::fs::read_to_string(
                cache(&ctx).join("global-guide/0.0.0/skills/global-guide/SKILL.md")
            )?,
            edited
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn dropping_the_global_use_removes_what_symposium_wrote_for_codex() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "codex"]).await?;
        ctx.symposium(&["sync"]).await?;
        assert!(cache(&ctx).join("global-guide").exists());

        ctx.symposium(&["use", "--remove", "--global", "global-guide"])
            .await?;

        assert_eq!(plugin_tables(&ctx, &codex_dir(&ctx)), "{}");
        assert!(!cache(&ctx).exists());
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn a_global_use_turned_workspace_use_returns_to_per_skill_copies() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "codex"]).await?;
        ctx.symposium(&["sync"]).await?;

        ctx.symposium(&["use", "--remove", "--global", "global-guide"])
            .await?;
        ctx.symposium(&["use", "global-guide"]).await?;

        assert!(!cache(&ctx).exists());
        assert_eq!(
            find_installed_skill(&codex_skill_dirs(&ctx), "global-guide"),
            project_skills(&ctx).join("global-guide")
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn removing_codex_removes_its_plugins_and_leaves_claudes() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "claude", "--add-agent", "codex"])
            .await?;
        ctx.symposium(&["sync"]).await?;
        assert!(cache(&ctx).join("global-guide").exists());

        ctx.symposium(&["init", "--remove-agent", "codex"]).await?;

        assert_eq!(plugin_tables(&ctx, &codex_dir(&ctx)), "{}");
        assert!(!cache(&ctx).exists());
        assert!(
            ctx.sym
                .home_dir()
                .join(".claude/skills/global-guide/plugin.json")
                .is_file()
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn codex_home_relocates_the_delivery() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        let relocated = ctx.tempdir.join("relocated-codex");
        let value = relocated.clone().into_os_string();
        ctx.sym
            .set_env(move |name| (name == "CODEX_HOME").then(|| value.clone()));

        ctx.symposium(&["init", "--add-agent", "codex"]).await?;
        ctx.symposium(&["sync"]).await?;

        assert!(
            relocated
                .join("plugins/cache/symposium/global-guide/0.0.0/plugin.json")
                .is_file()
        );
        assert!(plugin_tables(&ctx, &relocated).contains("global-guide@symposium"));
        assert!(!codex_dir(&ctx).join("plugins").exists());
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn a_per_skill_copy_another_agent_needs_stays_next_to_codexs_plugin() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "codex", "--add-agent", "opencode"])
            .await?;
        ctx.symposium(&["sync"]).await?;

        assert!(
            cache(&ctx)
                .join("global-guide/0.0.0/skills/global-guide/SKILL.md")
                .is_file()
        );
        assert!(
            project_skills(&ctx).join("global-guide/SKILL.md").is_file(),
            "OpenCode reads .agents/skills and takes no plugin, so Codex sees this copy too"
        );
        Ok(())
    })
    .await
    .unwrap();
}
