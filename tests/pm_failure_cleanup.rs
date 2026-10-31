//! A package-manager failure must preserve installed MCP servers as well as skills.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use serde_json::Value;
use symposium::UpdateLevel;
use symposium::pm::{
    PackageId, PackageManager, PluginKind, PmInstance, PmRegistry, ProjectInfo, UnvalidatedPlugin,
    Workspace,
};
use symposium_testlib::{TestMode, with_fixture};

struct IntermittentPm {
    root: PathBuf,
    fail_dependencies: bool,
}

#[async_trait::async_trait]
impl PackageManager for IntermittentPm {
    fn name(&self) -> &str {
        "cargo"
    }

    async fn project_info(&self) -> Result<ProjectInfo> {
        Ok(ProjectInfo {
            root: Some(self.root.clone()),
            ..ProjectInfo::default()
        })
    }

    async fn list_deps(&self) -> Result<Vec<PackageId>> {
        if self.fail_dependencies {
            anyhow::bail!("the package manager temporarily cannot list dependencies");
        }
        Ok(vec![PackageId::new("cargo", "review-dep", "1.0.0")])
    }

    async fn load_plugin(
        &self,
        _id: &PackageId,
        _update: UpdateLevel,
    ) -> Result<Vec<UnvalidatedPlugin>> {
        Ok(Vec::new())
    }
}

fn workspace(root: PathBuf, fail_dependencies: bool) -> Arc<Workspace> {
    Arc::new(Workspace::new(
        Some(root.clone()),
        PmRegistry::new(vec![PmInstance::new(
            "cargo",
            false,
            PluginKind::Package,
            Box::new(IntermittentPm {
                root,
                fail_dependencies,
            }),
        )]),
    ))
}

#[tokio::test]
async fn failed_dependency_listing_keeps_the_installed_mcp_server() {
    with_fixture(
        TestMode::SimulationOnly,
        &["dormant-plugin0"],
        async |mut ctx| {
            let manifest = ctx
                .sym
                .config_dir()
                .join("plugins/gateless-plugin/Symposium.toml");
            std::fs::write(
                &manifest,
                r#"
name = "gateless-plugin"
depends-on = ["review-dep"]

[[skills]]
source.path = "skills"

[[mcp_servers]]
name = "review-server"
command = "/usr/bin/true"
args = []
env = []
"#,
            )?;
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            let root = ctx.workspace_root.clone().unwrap();
            symposium::sync::sync(&ctx.sym, &workspace(root.clone(), false), UpdateLevel::None)
                .await?;
            let config = root.join(".mcp.json");
            let before: Value = serde_json::from_str(&std::fs::read_to_string(&config)?)?;
            assert!(
                before["mcpServers"]["review-server"].is_object(),
                "{before}"
            );
            let skill_paths: Vec<_> = std::fs::read_dir(root.join(".claude/skills"))?
                .map(|entry| entry.unwrap().path())
                .collect();
            assert_eq!(skill_paths.len(), 1);
            let failed = workspace(root, true);
            symposium::sync::sync(&ctx.sym, &failed, UpdateLevel::None).await?;
            assert_eq!(failed.failed(), ["cargo"]);
            assert!(
                skill_paths[0].is_dir(),
                "the failed PM's skill must be preserved"
            );
            let after: Value = serde_json::from_str(&std::fs::read_to_string(config)?)?;
            assert_eq!(
                after["mcpServers"]["review-server"], before["mcpServers"]["review-server"],
                "a failed dependency listing must preserve the MCP server along with the skill"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}
