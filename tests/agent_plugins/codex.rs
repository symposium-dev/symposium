use std::fs;

use symposium_testlib::{TestMode, with_fixture};

use crate::SCOPES;

#[tokio::test]
async fn codex_gets_the_entries_its_own_plugin_install_writes() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        let staging = ctx.sym.config_dir().join("installed");
        let index = staging.join(".claude-plugin/marketplace.json");
        let config = ctx.sym.home_dir().join(".codex/config.toml");

        ctx.symposium(&["init", "--add-agent", "codex"]).await?;
        let installed = ctx.sync_with_report(tracing::Level::INFO).await?;

        assert!(
            installed.iter().any(|e| e["kind"] == "skill_installed"
                && e["agent"] == "codex"
                && e["plugin"] == true),
            "{installed:#?}"
        );
        let written: toml::Table = toml::from_str(&fs::read_to_string(&config)?)?;
        let marketplace = &written["marketplaces"]["symposium"];
        assert_eq!(marketplace["source_type"].as_str(), Some("local"));
        assert_eq!(
            marketplace["source"].as_str(),
            Some(staging.display().to_string().as_str())
        );
        assert_eq!(
            written["plugins"]["global-guide@symposium"]["enabled"].as_bool(),
            Some(true)
        );
        assert!(
            index.is_file(),
            "the index Codex reads exists without Copilot"
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
                && e["path"].as_str().is_some_and(|path| path
                    .replace('\\', "/")
                    .ends_with("plugins/cache/symposium/global-guide"))),
            "{removed:#?}"
        );
        assert_eq!(fs::read_to_string(&config)?, "");
        assert!(!index.exists());
        Ok(())
    })
    .await
    .unwrap();
}
