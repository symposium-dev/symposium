//! Global initialization must preserve a package's activation policy.

use serde_json::Value;
use symposium::status_command::{StatusState, workspace_status};
use symposium_testlib::{TestMode, with_fixture};

#[tokio::test]
async fn init_does_not_register_a_dormant_package_mcp_server_globally() {
    with_fixture(
        TestMode::SimulationOnly,
        &["agent-plugin-registry0"],
        async |mut ctx| {
            let registry = symposium::plugins::load_registry(&ctx.sym).await;
            assert!(
                registry.plugins.iter().any(|plugin| {
                    plugin.manifest.name == "pdf-tools"
                        && plugin.manifest.mcp_servers.iter().any(|entry| {
                            matches!(&entry.server, sacp::schema::McpServer::Stdio(server)
                                if server.name == "pdf-tables")
                        })
                }),
                "the dormant package must actually offer the MCP server under test",
            );

            ctx.symposium(&["init", "--add-agent", "claude", "--hook-scope", "global"])
                .await?;

            let deps = ctx.sym.workspace(ctx.workspace_root.as_ref().unwrap());
            let entries = workspace_status(&ctx.sym, &deps).await?;
            assert!(
                entries
                    .iter()
                    .any(|entry| entry.name == "pdf-tools" && entry.state == StatusState::Dormant),
                "the fixture must still be awaiting explicit enablement: {entries:?}",
            );

            // pdf-tools has no dependency gate and has never been enabled with
            // `use`. Claude reads this user-level file in every project.
            // The unconditional registration path predates Agent Plugins, but
            // packages loaded through the new format must stay dormant too.
            let path = ctx.sym.home_dir().join(".claude.json");
            let config: Value = match std::fs::read_to_string(&path) {
                Ok(text) => serde_json::from_str(&text)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    serde_json::json!({})
                }
                Err(error) => return Err(error.into()),
            };
            assert!(
                config["mcpServers"].get("pdf-tables").is_none(),
                "a dormant package must not install a global MCP server: {config:#}",
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}
