//! Ferris MCP Server - helpful tools for Rust development

use anyhow::Result;
use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo, ToolsCapability,
    },
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The Ferris MCP server.
#[derive(Clone)]
pub struct FerrisServer {
    tool_router: ToolRouter<FerrisServer>,
}

impl FerrisServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

impl Default for FerrisServer {
    fn default() -> Self {
        Self::new()
    }
}

/// Input for the rust_crate_source tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RustCrateSourceInput {
    /// Name of the crate to fetch
    pub crate_name: String,
    /// Optional semver range (e.g., "1.0", "^1.2", "~1.2.3").
    /// If not specified, uses the version from the current project's Cargo.toml,
    /// or falls back to the latest version from crates.io.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crate_version: Option<String>,
}

/// Output from the rust_crate_source tool.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RustCrateSourceOutput {
    /// Name of the crate
    pub crate_name: String,
    /// The exact version that was fetched
    pub version: String,
    /// Local filesystem path to the extracted crate sources
    pub source_path: String,
}

#[tool_router]
impl FerrisServer {
    #[tool(
        description = "Get the local filesystem path to a Rust crate's source code. The crate will be downloaded and extracted if not already cached. Use this to read and analyze crate source code."
    )]
    async fn rust_crate_source(
        &self,
        Parameters(input): Parameters<RustCrateSourceInput>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let RustCrateSourceInput {
            crate_name,
            crate_version,
        } = input;

        tracing::info!(
            "Fetching Rust crate source: {} {:?}",
            crate_name,
            crate_version
        );

        let mut fetch = symposium_ferris::Ferris::rust_crate(&crate_name);

        if let Some(version_spec) = crate_version {
            fetch = fetch.version(&version_spec);
        }

        let result = fetch.fetch().await.map_err(|e| {
            rmcp::ErrorData::internal_error(format!("Failed to fetch crate: {}", e), None)
        })?;

        let output = RustCrateSourceOutput {
            crate_name,
            version: result.version,
            source_path: result.path.display().to_string(),
        };

        Ok(CallToolResult::success(vec![
            Content::json(output).map_err(|e| {
                rmcp::ErrorData::internal_error(format!("Failed to serialize output: {}", e), None)
            })?,
        ]))
    }
}

#[tool_handler]
impl ServerHandler for FerrisServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: Default::default(),
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability::default()),
                ..Default::default()
            },
            server_info: Implementation {
                name: "ferris".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                ..Default::default()
            },
            instructions: Some(
                "Ferris provides tools for Rust development. \
                 Use rust_crate_source to get the local path to a crate's source code \
                 for reading and analysis."
                    .into(),
            ),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("Starting Ferris MCP server");

    let server = FerrisServer::new();
    let transport = rmcp::transport::stdio();
    let service = server.serve(transport).await?;
    service.waiting().await?;

    Ok(())
}
