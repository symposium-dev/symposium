use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use expect_test::expect;
use symposium_testlib::{
    TestContext, TestMode, find_installed_skill, find_installed_skills, with_fixture,
};

const SCOPES: &[&str] = &["agent-plugin-scopes0"];

fn staging_root(ctx: &TestContext) -> PathBuf {
    ctx.sym.config_dir().join("installed")
}

fn settings(ctx: &TestContext) -> PathBuf {
    ctx.sym.home_dir().join(".copilot").join("settings.json")
}

fn read_settings(path: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn shared_skills(ctx: &TestContext) -> PathBuf {
    ctx.workspace_root
        .as_ref()
        .unwrap()
        .join(".agents")
        .join("skills")
}

fn copilot_skill_dirs(ctx: &TestContext) -> Vec<PathBuf> {
    vec![shared_skills(ctx), staging_root(ctx)]
}

fn drop_use(ctx: &mut TestContext, name: &str) -> anyhow::Result<()> {
    ctx.sym
        .config
        .plugins
        .used
        .retain(|entry| entry.name() != name);
    ctx.sym.save_config()
}

#[tokio::test]
async fn a_global_plugin_reaches_copilot_through_its_settings_and_kiro_keeps_per_skill_copies() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "copilot", "--add-agent", "kiro"])
            .await?;
        ctx.symposium(&["sync"]).await?;

        expect![[r#"
            {
              "enabledPlugins": {
                "global-guide@symposium": true
              },
              "extraKnownMarketplaces": {
                "symposium": {
                  "source": {
                    "path": "$CONFIG_DIR/installed",
                    "source": "directory"
                  }
                }
              }
            }"#]]
        .assert_eq(&ctx.normalize_json(&std::fs::read_to_string(settings(&ctx))?));
        assert_eq!(
            find_installed_skill(&copilot_skill_dirs(&ctx), "global-guide"),
            staging_root(&ctx).join("global-guide/skills/global-guide")
        );
        assert!(
            staging_root(&ctx)
                .join(".claude-plugin/marketplace.json")
                .is_file()
        );
        for skill in ["workspace-guide", "always-guide"] {
            assert_eq!(
                find_installed_skill(&copilot_skill_dirs(&ctx), skill),
                shared_skills(&ctx).join(skill)
            );
        }
        let kiro_skills = ctx.workspace_root.as_ref().unwrap().join(".kiro/skills");
        for skill in ["global-guide", "workspace-guide", "always-guide"] {
            find_installed_skill(std::slice::from_ref(&kiro_skills), skill);
        }

        let an_hour_ago = SystemTime::now() - Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(settings(&ctx))?
            .set_modified(an_hour_ago)?;
        let source = ctx.sym.config_dir().join("plugins/global-guide/SKILL.md");
        let edited = std::fs::read_to_string(&source)?.replace("Global guidance.", "Edited.");
        std::fs::write(&source, &edited)?;
        ctx.symposium(&["sync"]).await?;

        assert_eq!(
            std::fs::read_to_string(
                staging_root(&ctx).join("global-guide/skills/global-guide/SKILL.md")
            )?,
            edited
        );
        assert_eq!(
            std::fs::metadata(settings(&ctx))?.modified()?,
            an_hour_ago,
            "Copilot reads the staging root in place, so an edit needs no settings change"
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn a_skill_copy_another_agent_still_needs_stays_in_the_shared_directory() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "copilot", "--add-agent", "opencode"])
            .await?;
        ctx.symposium(&["sync"]).await?;

        let mut expected = vec![
            shared_skills(&ctx).join("global-guide"),
            staging_root(&ctx).join("global-guide/skills/global-guide"),
        ];
        expected.sort();
        assert_eq!(
            find_installed_skills(&copilot_skill_dirs(&ctx), "global-guide"),
            expected
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn dropping_the_global_use_unregisters_the_plugin_and_reports_it() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "copilot"]).await?;
        let shown = ctx.normalize_paths(&symposium::output::display_path(&settings(&ctx)));

        let installed = ctx.sync_with_report(tracing::Level::INFO).await?;
        assert!(
            installed.iter().any(|e| e["kind"] == "plugin_installed"
                && e["plugin"] == "global-guide"
                && e["agent"] == "copilot"
                && ctx.normalize_paths(e["dest"].as_str().unwrap()) == shown),
            "{installed:#?}"
        );

        drop_use(&mut ctx, "global-guide")?;
        let removed = ctx.sync_with_report(tracing::Level::INFO).await?;

        assert_eq!(read_settings(&settings(&ctx)), serde_json::json!({}));
        let expected = format!("removed plugin global-guide for copilot from {shown}");
        assert!(
            removed.iter().any(|e| e["kind"] == "info"
                && ctx.normalize_paths(e["message"].as_str().unwrap()) == expected),
            "{expected} not reported: {removed:#?}"
        );
        assert!(!staging_root(&ctx).join(".claude-plugin").exists());
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn a_users_false_outlives_the_plugin() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "copilot"]).await?;
        ctx.symposium(&["sync"]).await?;

        let mut disabled = read_settings(&settings(&ctx));
        disabled["enabledPlugins"]["global-guide@symposium"] = serde_json::json!(false);
        std::fs::write(settings(&ctx), disabled.to_string())?;
        ctx.symposium(&["sync"]).await?;

        assert_eq!(read_settings(&settings(&ctx)), disabled);
        assert!(find_installed_skills(&[shared_skills(&ctx)], "global-guide").is_empty());

        drop_use(&mut ctx, "global-guide")?;
        ctx.symposium(&["sync"]).await?;

        assert_eq!(
            read_settings(&settings(&ctx)),
            serde_json::json!({ "enabledPlugins": { "global-guide@symposium": false } })
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn removing_copilot_unregisters_its_plugins() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "copilot", "--add-agent", "kiro"])
            .await?;
        ctx.symposium(&["sync"]).await?;
        assert_ne!(read_settings(&settings(&ctx)), serde_json::json!({}));

        ctx.symposium(&["init", "--remove-agent", "copilot"])
            .await?;

        assert_eq!(read_settings(&settings(&ctx)), serde_json::json!({}));
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn copilot_home_relocates_the_registration() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        let relocated = ctx.tempdir.join("relocated-copilot");
        let value = relocated.clone().into_os_string();
        ctx.sym
            .set_env(move |name| (name == "COPILOT_HOME").then(|| value.clone()));

        ctx.symposium(&["init", "--add-agent", "copilot"]).await?;
        ctx.symposium(&["sync"]).await?;

        let registered = read_settings(&relocated.join("settings.json"));
        assert_eq!(
            registered["enabledPlugins"]["global-guide@symposium"],
            serde_json::json!(true)
        );
        assert!(!settings(&ctx).exists());
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn settings_symposium_cannot_own_are_untouched_and_the_skills_arrive_per_project() {
    let foreign = r#"{
  "extraKnownMarketplaces": {
    "symposium": { "source": { "source": "github", "repo": "someone/symposium" } }
  }
}"#;
    let unparseable = "{\n  // Copilot accepts comments here, symposium does not\n}";
    for contents in [foreign, unparseable] {
        with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "copilot"]).await?;
            std::fs::create_dir_all(settings(&ctx).parent().unwrap())?;
            std::fs::write(settings(&ctx), contents)?;

            let events = ctx.sync_with_report(tracing::Level::INFO).await?;

            assert_eq!(std::fs::read_to_string(settings(&ctx))?, contents);
            assert!(events.iter().any(|e| e["kind"] == "warning"), "{events:#?}");
            assert_eq!(
                find_installed_skill(&[shared_skills(&ctx)], "global-guide"),
                shared_skills(&ctx).join("global-guide")
            );
            Ok(())
        })
        .await
        .unwrap();
    }
}
