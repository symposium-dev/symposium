//! The crate_source tool - fetch Rust crate sources by name and version.

use std::path::PathBuf;

use sacp::{ProxyToConductor, mcp_server::McpServerBuilder};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::crate_sources::RustCrateFetch;

/// Parameters for the crate_source tool
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CrateSourceParams {
    /// Name of the Rust crate to fetch
    pub crate_name: String,
    /// Optional version specification (e.g., "1.0", "^1.2", "~1.2.3")
    /// Defaults to latest version if not specified
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Output from the crate_source tool
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CrateSourceOutput {
    /// The crate name
    pub crate_name: String,
    /// The resolved version that was fetched
    pub version: String,
    /// Path to the extracted crate sources on disk
    pub path: String,
}

/// Register the crate_source tool with the MCP server builder.
pub fn register(
    builder: McpServerBuilder<ProxyToConductor, impl sacp::JrResponder<ProxyToConductor>>,
    enabled: bool,
    cwd: PathBuf,
) -> McpServerBuilder<ProxyToConductor, impl sacp::JrResponder<ProxyToConductor>> {
    const TOOL_NAME: &str = "crate_sources";

    let builder = builder.tool_fn_mut(
        TOOL_NAME,
        indoc::indoc! {r#"
            Fetch and extract Rust crate source code from crates.io.

            Returns the local path where the crate sources are available for reading.
            Use this to inspect crate implementations, understand APIs, or debug issues.

            If no version is given, default to the version used in the current workspace,
            or the latest version if crate is not used.

            Examples:
            - Fetch the version of tokio used in the workspace: { "crate_name": "tokio" }
            - Fetch specific version: { "crate_name": "serde", "version": "1.0.193" }
            - Fetch with semver range: { "crate_name": "anyhow", "version": "^1.0" }
        "#},
        async move |input: CrateSourceParams, _context| -> Result<CrateSourceOutput, sacp::Error> {
            let CrateSourceParams {
                crate_name,
                version,
            } = input;

            tracing::info!(
                crate_name = %crate_name,
                version = ?version,
                "Fetching crate sources"
            );

            let mut fetch = RustCrateFetch::new(&crate_name, &cwd);
            if let Some(version_spec) = version {
                fetch = fetch.version(&version_spec);
            }

            let result = fetch
                .fetch()
                .await
                .map_err(|e| sacp::util::internal_error(format!("Failed to fetch crate: {}", e)))?;

            Ok(CrateSourceOutput {
                crate_name,
                version: result.version,
                path: result.path.display().to_string(),
            })
        },
        sacp::tool_fn_mut!(),
    );

    if enabled {
        builder.enable_tool(TOOL_NAME).expect("valid tool name")
    } else {
        builder.disable_tool(TOOL_NAME).expect("valid tool name")
    }
}
