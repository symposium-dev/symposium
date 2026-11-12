//! Dependency discovery must recognize Agent Plugins packages with only MCP servers.

use symposium::pm::{CARGO_PM, PackageId};
use symposium_install::UpdateLevel;
use symposium_testlib::{TestMode, with_fixture};

#[tokio::test]
async fn an_mcp_only_dependency_package_is_offered_for_consent() {
    with_fixture(
        TestMode::SimulationOnly,
        &["agent-plugin-dep0"],
        async |ctx| {
            let workspace = ctx.workspace_root.as_ref().unwrap();
            std::fs::remove_dir_all(workspace.join("portable-dep/skills"))?;
            let ws = ctx.sym.workspace(workspace);
            let pms = ws.pms();
            let id = PackageId::any_version(CARGO_PM, "portable-dep");

            // Control: the package is valid and its MCP server loads when
            // explicitly requested, without any skill files or Symposium.toml.
            let loaded = pms.load_plugin(&id, UpdateLevel::None).await;
            assert_eq!(loaded.len(), 1);
            assert_eq!(loaded[0].manifest.name, "portable-guidance");
            assert_eq!(loaded[0].manifest.mcp_servers.len(), 1);
            assert!(loaded[0].manifest.skills.is_empty());

            let registry = symposium::plugins::load_registry(&ctx.sym).await;
            let discovery = symposium::discovery::discover(&ctx.sym, &ws, &registry.plugins).await;
            assert!(
                discovery
                    .candidates
                    .iter()
                    .any(|candidate| candidate.id.pm == CARGO_PM
                        && candidate.id.name == "portable-dep"),
                "a valid dependency package's MCP server must be offered for consent: {discovery:?}"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}
