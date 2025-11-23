//! Symposium ACP Meta Proxy
//!
//! This is the main Symposium binary that acts as an ACP proxy. It receives
//! initialization from an editor, examines the capabilities provided, and uses
//! sacp-conductor to orchestrate a dynamic chain of component proxies.
//!
//! Architecture:
//! 1. Receive Initialize request from editor
//! 2. Examine capabilities to determine what components are needed
//! 3. Build proxy chain dynamically using conductor's lazy initialization
//! 4. Forward Initialize through the chain
//! 5. Bidirectionally forward all subsequent messages

use std::fs::File;
use std::path::PathBuf;

use anyhow::Result;
use sacp_conductor::conductor::Conductor;

#[derive(Debug, Clone, PartialEq, Eq, clap::Args)]
pub struct SymposiumArgs {
    /// Enable Sparkle integration
    #[arg(long, default_value = "true")]
    pub sparkle: bool,

    /// Enable logging of input messages (from editor/client)
    #[arg(long)]
    log_input: bool,

    /// Enable logging of output messages (to agent/server)
    #[arg(long)]
    log_output: bool,

    /// Directory path for log files
    #[arg(long, default_value = ".symposium/logs")]
    log_path: PathBuf,

    /// Redirect tracing output to a file instead of stderr
    #[arg(long)]
    log_to: Option<PathBuf>,

    /// Set tracing filter (e.g., "info", "debug", "foo=trace,bar=debug")
    #[arg(long)]
    log: Option<String>,
}

impl SymposiumArgs {
    /// Path where input messages are logged; creates log directory if necessary
    fn log_input_path(&self) -> Result<Option<PathBuf>> {
        if self.log_input {
            self.create_log_dir()?;
            Ok(Some(self.log_path.join("log_in.txt")))
        } else {
            Ok(None)
        }
    }

    /// Path where output messages are logged; creates log directory if necessary
    fn log_output_path(&self) -> Result<Option<PathBuf>> {
        if self.log_output {
            self.create_log_dir()?;
            Ok(Some(self.log_path.join("log_out.txt")))
        } else {
            Ok(None)
        }
    }

    fn create_log_dir(&self) -> Result<()> {
        Ok(std::fs::create_dir_all(&self.log_path)?)
    }
}

/// Run the Symposium ACP meta proxy
///
/// This is the main entry point that:
/// - Creates session logging infrastructure
/// - Listens for the Initialize request
/// - Uses conductor with lazy initialization to build the proxy chain
/// - Forwards all messages bidirectionally
pub async fn run(args: &SymposiumArgs) -> Result<()> {
    // Determine the tracing filter
    let filter = if let Some(ref log_filter) = args.log {
        tracing_subscriber::EnvFilter::new(log_filter)
    } else {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
    };

    // Initialize tracing - either to a file or to stderr
    if let Some(ref log_file) = args.log_to {
        // Create parent directory if it doesn't exist
        if let Some(parent) = log_file.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = File::create(log_file)?;
        tracing_subscriber::fmt()
            .with_writer(file)
            .with_env_filter(filter)
            .with_ansi(false) // Disable ANSI colors for file output
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .with_env_filter(filter)
            .init();
    }

    tracing::info!("Starting Symposium ACP meta proxy");

    // Create conductor with lazy initialization
    symposium_conductor(args)?
        .run(sacp_tokio::Stdio::new())
        .await?;

    Ok(())
}

/// Create and return the "symposium conductor", which assembles the symposium libraries together.
pub fn symposium_conductor(args: &SymposiumArgs) -> Result<Conductor> {
    // Create conductor with lazy initialization using ComponentList trait
    // The closure receives the Initialize request and returns components to spawn
    let args = args.clone();
    let conductor = Conductor::new(
        "symposium".to_string(),
        |init_req| async move {
            tracing::info!("Building proxy chain based on capabilities");

            // TODO: Examine init_req.capabilities to determine what's needed

            let mut components = vec![];

            if let Some(input_path) = args.log_input_path()? {
                components.push(sacp::DynComponent::new(sacp_tee::Tee::new(input_path)));
            }

            components.push(sacp::DynComponent::new(
                symposium_crate_sources_proxy::CrateSourcesProxy {},
            ));

            if args.sparkle {
                components.push(sacp::DynComponent::new(sparkle::SparkleComponent::new()));
            }

            // TODO: Add more components based on capabilities
            // - Check for IDE operation capabilities
            // - Spawn ide-ops adapter if missing
            // - Spawn ide-ops component to provide MCP tools

            if let Some(output_path) = args.log_output_path()? {
                components.push(sacp::DynComponent::new(sacp_tee::Tee::new(output_path)));
            }

            Ok((init_req, components))
        },
        None, // No custom conductor command
    );

    Ok(conductor)
}
