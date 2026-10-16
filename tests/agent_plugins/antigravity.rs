use std::path::{Path, PathBuf};

use expect_test::expect;
use symposium_testlib::{TestContext, TestMode, find_installed_skill, tree, with_fixture};

const SCOPES: &[&str] = &["agent-plugin-scopes0"];

fn staged(ctx: &TestContext) -> PathBuf {
    ctx.sym.config_dir().join("installed/global-guide")
}

fn user_plugins(ctx: &TestContext) -> PathBuf {
    ctx.sym.home_dir().join(".gemini/config/plugins")
}

fn project_skills(ctx: &TestContext, agent_dir: &str) -> PathBuf {
    ctx.workspace_root
        .as_ref()
        .unwrap()
        .join(agent_dir)
        .join("skills")
}

fn skill_dirs(ctx: &TestContext) -> Vec<PathBuf> {
    vec![project_skills(ctx, ".agents"), user_plugins(ctx)]
}

fn files(dir: &Path) -> Vec<(String, Vec<u8>)> {
    tree(dir)
        .lines()
        .filter(|rel| !rel.ends_with('/'))
        .map(|rel| (rel.to_string(), std::fs::read(dir.join(rel)).unwrap()))
        .collect()
}

#[tokio::test]
async fn a_global_plugin_reaches_antigravity_once_as_a_plugin_directory() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "antigravity", "--add-agent", "kiro"])
            .await?;
        ctx.symposium(&["sync"]).await?;

        expect![[r#"
            global-guide/
            global-guide/.claude-plugin/
            global-guide/.claude-plugin/plugin.json
            global-guide/.symposium
            global-guide/plugin.json
            global-guide/skills/
            global-guide/skills/global-guide/
            global-guide/skills/global-guide/SKILL.md
            global-guide/skills/global-guide/reference.md"#]]
        .assert_eq(&tree(&user_plugins(&ctx)));
        assert_eq!(
            files(&user_plugins(&ctx).join("global-guide")),
            files(&staged(&ctx)),
            "`agy plugin install` copies a plugin directory unchanged, and so does symposium"
        );

        assert_eq!(
            find_installed_skill(&skill_dirs(&ctx), "global-guide"),
            user_plugins(&ctx).join("global-guide/skills/global-guide")
        );
        for skill in ["workspace-guide", "always-guide"] {
            assert_eq!(
                find_installed_skill(&skill_dirs(&ctx), skill),
                project_skills(&ctx, ".agents").join(skill)
            );
        }
        for skill in ["global-guide", "workspace-guide", "always-guide"] {
            find_installed_skill(&[project_skills(&ctx, ".kiro")], skill);
        }

        let gemini = ctx.sym.home_dir().join(".gemini");
        for owned_by_antigravity in [
            "config/config.json",
            "config/import_manifest.json",
            "config/plugins.json",
            "antigravity-cli",
        ] {
            assert!(
                !gemini.join(owned_by_antigravity).exists(),
                "{owned_by_antigravity} belongs to Antigravity"
            );
        }
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn a_per_skill_copy_stays_while_another_agent_reading_it_is_configured() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        ctx.symposium(&[
            "init",
            "--add-agent",
            "antigravity",
            "--add-agent",
            "opencode",
        ])
        .await?;
        ctx.symposium(&["sync"]).await?;

        let per_skill = project_skills(&ctx, ".agents").join("global-guide");
        assert!(per_skill.join("SKILL.md").is_file());
        assert!(
            user_plugins(&ctx)
                .join("global-guide/skills/global-guide/SKILL.md")
                .is_file()
        );

        ctx.symposium(&["init", "--remove-agent", "opencode"])
            .await?;
        ctx.symposium(&["sync"]).await?;

        assert!(!per_skill.exists());
        assert_eq!(
            find_installed_skill(&skill_dirs(&ctx), "global-guide"),
            user_plugins(&ctx).join("global-guide/skills/global-guide")
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn dropping_the_global_use_or_antigravity_removes_the_copy() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "antigravity", "--add-agent", "kiro"])
            .await?;
        let installed = ctx.sync_with_report(tracing::Level::INFO).await?;
        assert!(
            installed.iter().any(|e| e["kind"] == "plugin_installed"
                && e["plugin"] == "global-guide"
                && e["agent"] == "antigravity"),
            "{installed:#?}"
        );
        let copy = user_plugins(&ctx).join("global-guide");

        ctx.symposium(&["init", "--remove-agent", "antigravity"])
            .await?;
        assert!(!copy.exists());

        ctx.symposium(&["init", "--add-agent", "antigravity"])
            .await?;
        ctx.symposium(&["sync"]).await?;
        assert!(copy.exists());

        ctx.symposium(&["use", "--remove", "--global", "global-guide"])
            .await?;
        assert!(!copy.exists());
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn a_users_own_plugin_and_antigravity_settings_are_left_alone() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        let users_plugin = user_plugins(&ctx).join("global-guide");
        std::fs::create_dir_all(&users_plugin)?;
        std::fs::write(
            users_plugin.join("plugin.json"),
            r#"{"name": "global-guide"}"#,
        )?;
        let settings = ctx.sym.home_dir().join(".gemini/config/config.json");
        let disabled = r#"{"plugins": {"global-guide": {"enabled": false}}}"#;
        std::fs::write(&settings, disabled)?;

        ctx.symposium(&["init", "--add-agent", "antigravity"])
            .await?;
        let report = ctx.sync_with_report(tracing::Level::INFO).await?;

        assert_eq!(tree(&users_plugin), "plugin.json");
        assert_eq!(std::fs::read_to_string(&settings)?, disabled);
        assert!(
            report.iter().any(|e| e["kind"] == "warning"
                && e["message"]
                    .as_str()
                    .unwrap()
                    .contains("skipping plugin global-guide for antigravity")),
            "{report:#?}"
        );
        Ok(())
    })
    .await
    .unwrap();
}
