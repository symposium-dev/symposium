use std::path::{Path, PathBuf};

use expect_test::expect;
use symposium::config::{HookScope, UseEntry};
use symposium_testlib::{
    TestContext, TestMode, find_installed_skill, find_installed_skills, tree, with_fixture,
};

const SCOPES: &[&str] = &["agent-plugin-scopes0"];

fn staging_root(ctx: &TestContext) -> PathBuf {
    ctx.sym.config_dir().join("installed")
}

fn user_skills(ctx: &TestContext) -> PathBuf {
    ctx.sym.home_dir().join(".claude").join("skills")
}

fn project_skills(ctx: &TestContext, agent_dir: &str) -> PathBuf {
    ctx.workspace_root
        .as_ref()
        .unwrap()
        .join(agent_dir)
        .join("skills")
}

fn project_symposium_dir(ctx: &TestContext) -> PathBuf {
    ctx.workspace_root.as_ref().unwrap().join(".symposium")
}

fn project_staging(ctx: &TestContext) -> PathBuf {
    project_symposium_dir(ctx).join("plugins")
}

fn claude_skill_dirs(ctx: &TestContext) -> Vec<PathBuf> {
    vec![project_skills(ctx, ".claude"), user_skills(ctx)]
}

fn project_plugin_skill(ctx: &TestContext, plugin: &str, skill: &str) -> PathBuf {
    project_skills(ctx, ".claude")
        .join(plugin)
        .join("skills")
        .join(skill)
}

fn drop_use(ctx: &mut TestContext, name: &str) -> anyhow::Result<()> {
    ctx.sym
        .config
        .plugins
        .used
        .retain(|entry| entry.name() != name);
    ctx.sym.save_config()
}

fn write_workspace(dir: &Path) {
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"other-host\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/lib.rs"), "").unwrap();
}

#[tokio::test]
async fn a_global_plugin_reaches_claude_once_and_other_agents_keep_per_skill_copies() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        ctx.symposium(&[
            "init",
            "--add-agent",
            "claude",
            "--add-agent",
            "codex",
            "--add-agent",
            "kiro",
        ])
        .await?;
        ctx.symposium(&["sync"]).await?;

        expect![[r#"
            .claude-plugin/
            .claude-plugin/marketplace.json
            global-guide/
            global-guide/.claude-plugin/
            global-guide/.claude-plugin/plugin.json
            global-guide/.symposium
            global-guide/plugin.json
            global-guide/skills/
            global-guide/skills/global-guide/
            global-guide/skills/global-guide/SKILL.md
            global-guide/skills/global-guide/reference.md"#]]
        .assert_eq(&tree(&staging_root(&ctx)));
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
        .assert_eq(&tree(&user_skills(&ctx)));
        expect![[r#"
            {
              "name": "symposium",
              "owner": {
                "name": "symposium"
              },
              "plugins": [
                {
                  "name": "global-guide",
                  "source": "./global-guide"
                }
              ]
            }"#]]
        .assert_eq(&ctx.normalize_json(&std::fs::read_to_string(
            staging_root(&ctx).join(".claude-plugin/marketplace.json"),
        )?));
        expect![[r#"
            {
              "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
              "name": "global-guide",
              "version": "0.0.0"
            }"#]]
        .assert_eq(&ctx.normalize_json(&std::fs::read_to_string(
            user_skills(&ctx).join("global-guide/.claude-plugin/plugin.json"),
        )?));

        assert_eq!(
            find_installed_skill(&claude_skill_dirs(&ctx), "global-guide"),
            user_skills(&ctx).join("global-guide/skills/global-guide")
        );
        for skill in ["workspace-guide", "always-guide"] {
            assert_eq!(
                find_installed_skill(&claude_skill_dirs(&ctx), skill),
                project_plugin_skill(&ctx, skill, skill)
            );
        }
        for agent_dir in [".agents", ".kiro"] {
            for skill in ["global-guide", "workspace-guide", "always-guide"] {
                find_installed_skill(&[project_skills(&ctx, agent_dir)], skill);
            }
        }
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn project_plugins_reach_claude_as_plugins_in_the_project() {
    with_fixture(
        TestMode::SimulationOnly,
        &["agent-plugin-scopes0", "member-agents-skills0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            expect![[r#"
                always-guide/
                always-guide/.claude-plugin/
                always-guide/.claude-plugin/plugin.json
                always-guide/.gitignore
                always-guide/.symposium
                always-guide/plugin.json
                always-guide/skills/
                always-guide/skills/always-guide/
                always-guide/skills/always-guide/SKILL.md
                tool/
                tool/.claude-plugin/
                tool/.claude-plugin/plugin.json
                tool/.gitignore
                tool/.symposium
                tool/plugin.json
                tool/skills/
                tool/skills/plain-notes/
                tool/skills/plain-notes/SKILL.md
                tool/skills/tool-maintainer/
                tool/skills/tool-maintainer/SKILL.md
                workspace-guide/
                workspace-guide/.claude-plugin/
                workspace-guide/.claude-plugin/plugin.json
                workspace-guide/.gitignore
                workspace-guide/.symposium
                workspace-guide/plugin.json
                workspace-guide/skills/
                workspace-guide/skills/workspace-guide/
                workspace-guide/skills/workspace-guide/SKILL.md"#]]
            .assert_eq(&tree(&project_skills(&ctx, ".claude")));
            expect![[r#"
                .gitignore
                plugins/
                plugins/.claude-plugin/
                plugins/.claude-plugin/marketplace.json
                plugins/always-guide/
                plugins/always-guide/.claude-plugin/
                plugins/always-guide/.claude-plugin/plugin.json
                plugins/always-guide/.symposium
                plugins/always-guide/plugin.json
                plugins/always-guide/skills/
                plugins/always-guide/skills/always-guide/
                plugins/always-guide/skills/always-guide/SKILL.md
                plugins/tool/
                plugins/tool/.claude-plugin/
                plugins/tool/.claude-plugin/plugin.json
                plugins/tool/.symposium
                plugins/tool/plugin.json
                plugins/tool/skills/
                plugins/tool/skills/plain-notes/
                plugins/tool/skills/plain-notes/SKILL.md
                plugins/tool/skills/tool-maintainer/
                plugins/tool/skills/tool-maintainer/SKILL.md
                plugins/workspace-guide/
                plugins/workspace-guide/.claude-plugin/
                plugins/workspace-guide/.claude-plugin/plugin.json
                plugins/workspace-guide/.symposium
                plugins/workspace-guide/plugin.json
                plugins/workspace-guide/skills/
                plugins/workspace-guide/skills/workspace-guide/
                plugins/workspace-guide/skills/workspace-guide/SKILL.md"#]]
            .assert_eq(&tree(&project_symposium_dir(&ctx)));
            assert_eq!(
                std::fs::read_to_string(project_symposium_dir(&ctx).join(".gitignore"))?,
                "*\n"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn hook_scope_does_not_decide_where_a_global_plugin_installs() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        ctx.sym.config.hook_scope = HookScope::Global;
        ctx.sym.save_config()?;
        ctx.symposium(&["init", "--add-agent", "claude"]).await?;
        ctx.symposium(&["sync"]).await?;

        assert_eq!(
            find_installed_skill(&claude_skill_dirs(&ctx), "global-guide"),
            user_skills(&ctx).join("global-guide/skills/global-guide")
        );
        assert_eq!(
            find_installed_skill(&claude_skill_dirs(&ctx), "workspace-guide"),
            project_plugin_skill(&ctx, "workspace-guide", "workspace-guide")
        );
        let settings: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(
            ctx.sym.home_dir().join(".claude/settings.json"),
        )?)?;
        assert!(settings.get("hooks").is_some(), "global hooks registered");
        assert!(
            settings.get("enabledPlugins").is_none()
                && settings.get("extraKnownMarketplaces").is_none(),
            "plugin delivery writes no settings: {settings}"
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn another_workspace_shares_the_global_plugin_and_gets_no_project_copy() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "claude"]).await?;
        ctx.symposium(&["sync"]).await?;
        let staged = tree(&staging_root(&ctx));
        let delivered = tree(&user_skills(&ctx));
        let first_project = tree(&project_skills(&ctx, ".claude"));

        let other = ctx.tempdir.join("other-workspace");
        write_workspace(&other);
        symposium::sync::sync(
            &ctx.sym,
            &ctx.sym.workspace_deps(&other),
            symposium::UpdateLevel::None,
        )
        .await?;

        assert_eq!(tree(&staging_root(&ctx)), staged);
        assert_eq!(tree(&user_skills(&ctx)), delivered);
        assert_eq!(tree(&project_skills(&ctx, ".claude")), first_project);
        let other_skills = other.join(".claude/skills");
        assert!(
            find_installed_skills(std::slice::from_ref(&other_skills), "global-guide").is_empty()
        );
        assert!(
            find_installed_skills(std::slice::from_ref(&other_skills), "workspace-guide")
                .is_empty()
        );
        find_installed_skill(&[other_skills], "always-guide");
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn dropping_a_use_removes_what_symposium_wrote_and_reports_it() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "claude"]).await?;

        let installed = ctx.sync_with_report(tracing::Level::INFO).await?;
        for plugin in ["global-guide", "workspace-guide"] {
            assert!(
                installed.iter().any(|e| e["kind"] == "plugin_installed"
                    && e["plugin"] == plugin
                    && e["agent"] == "claude"),
                "{installed:#?}"
            );
        }

        drop_use(&mut ctx, "global-guide")?;
        drop_use(&mut ctx, "workspace-guide")?;
        let removed = ctx.sync_with_report(tracing::Level::INFO).await?;

        for path in [
            staging_root(&ctx).join("global-guide"),
            user_skills(&ctx).join("global-guide"),
            project_staging(&ctx).join("workspace-guide"),
            project_skills(&ctx, ".claude").join("workspace-guide"),
        ] {
            assert!(!path.exists(), "{} is left behind", path.display());
            let tail = format!(
                "/{}",
                path.strip_prefix(&ctx.tempdir)?
                    .to_string_lossy()
                    .replace('\\', "/")
            );
            assert!(
                removed.iter().any(|e| e["kind"] == "skill_removed"
                    && e["path"]
                        .as_str()
                        .unwrap()
                        .replace('\\', "/")
                        .ends_with(&tail)),
                "{tail} removal not reported: {removed:#?}"
            );
        }
        assert!(!staging_root(&ctx).join(".claude-plugin").exists());
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn moving_a_plugin_between_scopes_leaves_nothing_behind() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "claude"]).await?;
        ctx.symposium(&["sync"]).await?;
        let user_scope = tree(&user_skills(&ctx));

        ctx.symposium(&["use", "--remove", "--global", "global-guide"])
            .await?;
        ctx.symposium(&["use", "global-guide"]).await?;

        assert_eq!(tree(&staging_root(&ctx)), "");
        assert_eq!(tree(&user_skills(&ctx)), "");
        assert_eq!(
            find_installed_skill(&claude_skill_dirs(&ctx), "global-guide"),
            project_plugin_skill(&ctx, "global-guide", "global-guide")
        );

        ctx.symposium(&["use", "--remove", "global-guide"]).await?;
        ctx.symposium(&["use", "--global", "global-guide"]).await?;

        assert_eq!(tree(&user_skills(&ctx)), user_scope);
        for path in [
            project_staging(&ctx).join("global-guide"),
            project_skills(&ctx, ".claude").join("global-guide"),
        ] {
            assert!(!path.exists(), "{} is left behind", path.display());
        }
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn removing_claude_removes_its_plugins() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "claude", "--add-agent", "codex"])
            .await?;
        ctx.symposium(&["sync"]).await?;
        assert!(user_skills(&ctx).join("global-guide").exists());
        assert!(
            project_skills(&ctx, ".claude")
                .join("always-guide")
                .exists()
        );

        ctx.symposium(&["init", "--remove-agent", "claude"]).await?;
        assert!(!user_skills(&ctx).join("global-guide").exists());

        ctx.symposium(&["sync"]).await?;
        assert_eq!(tree(&project_skills(&ctx, ".claude")), "");
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn directories_symposium_does_not_own_are_left_alone() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        let users_staged = staging_root(&ctx).join("global-guide");
        std::fs::create_dir_all(&users_staged)?;
        std::fs::write(users_staged.join("notes.txt"), "mine")?;
        let users_skill = user_skills(&ctx).join("global-guide");
        std::fs::create_dir_all(&users_skill)?;
        std::fs::write(
            users_skill.join("SKILL.md"),
            "---\nname: global-guide\ndescription: mine\n---\n",
        )?;

        ctx.symposium(&["init", "--add-agent", "claude"]).await?;
        ctx.symposium(&["sync"]).await?;

        assert_eq!(tree(&users_staged), "notes.txt");
        assert_eq!(tree(&users_skill), "SKILL.md");
        let compiled: Vec<String> = tree(&staging_root(&ctx))
            .lines()
            .filter(|line| line.ends_with("/plugin.json") && !line.contains(".claude-plugin"))
            .map(str::to_string)
            .collect();
        assert_eq!(compiled.len(), 1, "{compiled:?}");
        let name = compiled[0].trim_end_matches("/plugin.json");
        assert!(name.starts_with("global-guide-"), "{name}");
        assert!(
            user_skills(&ctx)
                .join(name)
                .join("skills/global-guide/SKILL.md")
                .is_file()
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn editing_a_global_plugins_skill_updates_claudes_copy() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "claude"]).await?;
        ctx.symposium(&["sync"]).await?;

        let source = ctx.sym.config_dir().join("plugins/global-guide/SKILL.md");
        let edited = std::fs::read_to_string(&source)?.replace("Global guidance.", "Edited.");
        std::fs::write(&source, &edited)?;
        ctx.symposium(&["sync"]).await?;

        assert_eq!(
            std::fs::read_to_string(
                user_skills(&ctx).join("global-guide/skills/global-guide/SKILL.md")
            )?,
            edited
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn claude_config_dir_relocates_the_delivery() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        let relocated = ctx.tempdir.join("relocated-claude");
        let value = relocated.clone().into_os_string();
        ctx.sym
            .set_env(move |name| (name == "CLAUDE_CONFIG_DIR").then(|| value.clone()));

        ctx.symposium(&["init", "--add-agent", "claude"]).await?;
        ctx.symposium(&["sync"]).await?;

        assert!(
            relocated
                .join("skills/global-guide/.claude-plugin/plugin.json")
                .is_file()
        );
        assert!(!user_skills(&ctx).exists());
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn a_skill_shared_by_two_global_plugins_is_delivered_once() {
    with_fixture(
        TestMode::SimulationOnly,
        &["dedup-source-origin0", "workspace0"],
        async |mut ctx| {
            ctx.sym.config.plugins.used = vec![
                UseEntry::Global("plugin-a".into()),
                UseEntry::Global("plugin-b".into()),
            ];
            ctx.sym.save_config()?;
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            assert_eq!(
                find_installed_skill(&claude_skill_dirs(&ctx), "shared-skill"),
                user_skills(&ctx).join("plugin-a/skills/shared-skill")
            );
            assert!(!staging_root(&ctx).join("plugin-b").exists());
            Ok(())
        },
    )
    .await
    .unwrap();
}
