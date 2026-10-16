use std::path::{Path, PathBuf};

use expect_test::expect;
use symposium_testlib::{TestContext, TestMode, find_installed_skill, tree, with_fixture};

const SCOPES: &[&str] = &["agent-plugin-scopes0"];

fn staged(ctx: &TestContext) -> PathBuf {
    ctx.sym.config_dir().join("installed/global-guide")
}

fn user_plugins(ctx: &TestContext) -> PathBuf {
    ctx.sym.home_dir().join(".agents/plugins")
}

fn project_skills(ctx: &TestContext) -> PathBuf {
    ctx.workspace_root.as_ref().unwrap().join(".agents/skills")
}

fn files(dir: &Path) -> Vec<(String, Vec<u8>)> {
    tree(dir)
        .lines()
        .filter(|rel| !rel.ends_with('/'))
        .map(|rel| (rel.to_string(), std::fs::read(dir.join(rel)).unwrap()))
        .collect()
}

#[tokio::test]
async fn a_global_plugin_reaches_goose_and_antigravity_without_per_skill_copies() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "goose", "--add-agent", "antigravity"])
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
            "the copy is the staged directory, skill names left as they are"
        );

        let goose_sees = [project_skills(&ctx), user_plugins(&ctx)];
        assert_eq!(
            find_installed_skill(&goose_sees, "global-guide"),
            user_plugins(&ctx).join("global-guide/skills/global-guide")
        );
        for skill in ["workspace-guide", "always-guide"] {
            assert_eq!(
                find_installed_skill(&goose_sees, skill),
                project_skills(&ctx).join(skill)
            );
        }
        assert!(
            ctx.sym
                .home_dir()
                .join(".gemini/config/plugins/global-guide/skills/global-guide/SKILL.md")
                .is_file()
        );
        assert!(
            !ctx.sym
                .home_dir()
                .join(".config/goose/config.yaml")
                .exists(),
            "Goose records discovered plugins in its config.yaml itself"
        );
        Ok(())
    })
    .await
    .unwrap();
}

const MCP_SERVER_PLUGIN: &str = r#"name = "mcp-server"
depends-on = ["*"]

[[mcp_servers]]
name = "probe-server"
command = "/usr/bin/true"
args = ["--stdio"]
env = []
"#;

#[tokio::test]
async fn goose_path_root_relocates_mcp_servers_and_plugins() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        let relocated = ctx.tempdir.join("goose-root");
        let value = relocated.clone().into_os_string();
        ctx.sym
            .set_env(move |name| (name == "GOOSE_PATH_ROOT").then(|| value.clone()));
        let plugin = ctx.sym.config_dir().join("plugins/mcp-server");
        std::fs::create_dir_all(&plugin)?;
        std::fs::write(
            plugin.join(symposium::pm::layout::MANIFEST_FILE),
            MCP_SERVER_PLUGIN,
        )?;

        ctx.symposium(&["init", "--add-agent", "goose"]).await?;
        ctx.symposium(&["sync"]).await?;

        let config = relocated.join("config/config.yaml");
        let copy = relocated.join(".agents/plugins/global-guide");
        assert!(std::fs::read_to_string(&config)?.contains("probe-server:"));
        assert!(copy.join("plugin.json").is_file());
        for default in [".config/goose", ".agents"] {
            assert!(!ctx.sym.home_dir().join(default).exists(), "{default}");
        }

        ctx.symposium(&["init", "--remove-agent", "goose"]).await?;

        let unregistered = std::fs::read_to_string(&config)?;
        assert!(!unregistered.contains("probe-server"), "{unregistered}");
        assert!(!copy.exists());
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn dropping_the_global_use_or_goose_removes_the_copy() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "goose", "--add-agent", "kiro"])
            .await?;
        let installed = ctx.sync_with_report(tracing::Level::INFO).await?;
        assert!(
            installed.iter().any(|e| e["kind"] == "plugin_installed"
                && e["plugin"] == "global-guide"
                && e["agent"] == "goose"),
            "{installed:#?}"
        );
        let copy = user_plugins(&ctx).join("global-guide");

        ctx.symposium(&["init", "--remove-agent", "goose"]).await?;
        assert!(!copy.exists());

        ctx.symposium(&["init", "--add-agent", "goose"]).await?;
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
async fn a_users_own_plugin_and_goose_config_are_left_alone() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        let users_plugin = user_plugins(&ctx).join("global-guide");
        std::fs::create_dir_all(&users_plugin)?;
        std::fs::write(
            users_plugin.join(".goose-plugin-install.json"),
            r#"{"source_type": "git"}"#,
        )?;
        let config = ctx.sym.home_dir().join(".config/goose/config.yaml");
        std::fs::create_dir_all(config.parent().unwrap())?;
        let disabled = format!(
            "plugins:\n  {}:\n    enabled: false\n",
            users_plugin.display()
        );
        std::fs::write(&config, &disabled)?;

        ctx.symposium(&["init", "--add-agent", "goose"]).await?;
        let report = ctx.sync_with_report(tracing::Level::INFO).await?;

        assert_eq!(tree(&users_plugin), ".goose-plugin-install.json");
        assert_eq!(std::fs::read_to_string(&config)?, disabled);
        assert!(
            report.iter().any(|e| e["kind"] == "warning"
                && e["message"]
                    .as_str()
                    .unwrap()
                    .contains("skipping plugin global-guide for goose")),
            "{report:#?}"
        );
        Ok(())
    })
    .await
    .unwrap();
}
