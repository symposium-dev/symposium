use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use symposium::config::HookScope;
use symposium_testlib::{TestMode, with_fixture};

use crate::SCOPES;

pub(crate) fn plugin_dir(root: &Path, plugin: &str) -> PathBuf {
    let settings = read(&root.join("settings.json"));
    let enabled = settings["enabledPlugins"][format!("{plugin}@symposium")] == true;
    match settings["extraKnownMarketplaces"]["symposium"]["source"]["path"].as_str() {
        Some(marketplace) if enabled => Path::new(marketplace).join(plugin),
        _ => root.join("installed-plugins").join(plugin),
    }
}

fn read(path: &Path) -> Value {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

#[tokio::test]
async fn copilot_gets_the_entries_its_own_plugin_install_writes() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        let staging = ctx.sym.config_dir().join("installed");
        let index = staging.join(".claude-plugin/marketplace.json");
        let settings = ctx.sym.home_dir().join(".copilot/settings.json");

        ctx.symposium(&["init", "--add-agent", "copilot"]).await?;
        let installed = ctx.sync_with_report(tracing::Level::INFO).await?;

        assert!(
            installed.iter().any(|e| e["kind"] == "skill_installed"
                && e["agent"] == "copilot"
                && e["plugin"] == true),
            "{installed:#?}"
        );
        let source = json!({ "source": "directory", "path": staging.display().to_string() });
        assert_eq!(
            read(&settings),
            json!({
                "extraKnownMarketplaces": { "symposium": { "source": source } },
                "enabledPlugins": { "global-guide@symposium": true },
            })
        );
        assert_eq!(
            read(&index),
            json!({
                "name": "symposium",
                "owner": { "name": "symposium" },
                "plugins": [{ "name": "global-guide", "source": "./global-guide" }],
            })
        );

        ctx.sym
            .config
            .plugins
            .used
            .retain(|entry| entry.name() != "global-guide");
        ctx.sym.save_config()?;
        let removed = ctx.sync_with_report(tracing::Level::INFO).await?;

        assert!(
            removed.iter().any(|e| e["kind"] == "skill_removed"
                && e["path"]
                    .to_string()
                    .contains("global-guide@symposium from")),
            "{removed:#?}"
        );
        assert_eq!(
            read(&settings),
            json!({ "extraKnownMarketplaces": {}, "enabledPlugins": {} })
        );
        assert!(!index.exists());
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn settings_symposium_cannot_read_are_left_alone_and_the_skills_arrive_per_project() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        ctx.sym.config.hook_scope = HookScope::Global;
        ctx.sym.save_config()?;
        ctx.symposium(&["init", "--add-agent", "copilot"]).await?;
        let settings = ctx.sym.home_dir().join(".copilot/settings.json");
        let unreadable = "{\n  // Copilot accepts comments here, symposium does not\n}";
        fs::write(&settings, unreadable)?;

        let events = ctx.sync_with_report(tracing::Level::INFO).await?;

        assert_eq!(fs::read_to_string(&settings)?, unreadable);
        let workspace = ctx.workspace_root.clone().unwrap();
        assert!(
            workspace
                .join(".agents/skills/global-guide/SKILL.md")
                .is_file()
        );
        assert!(
            events.iter().any(|e| e["kind"] == "warning"
                && e["message"].to_string().contains("plugins for copilot")),
            "{events:#?}"
        );
        Ok(())
    })
    .await
    .unwrap();
}
