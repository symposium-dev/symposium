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

pub mod logging;

use anyhow::Result;
use sacp::component::Component;
use sacp_conductor::conductor::Conductor;

/// Run the Symposium ACP meta proxy
///
/// This is the main entry point that:
/// - Creates session logging infrastructure
/// - Listens for the Initialize request
/// - Uses conductor with lazy initialization to build the proxy chain
/// - Forwards all messages bidirectionally
pub async fn run() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!("Starting Symposium ACP meta proxy");

    // Create session logger
    let session_logger = logging::SessionLogger::new().await?;
    tracing::info!(
        "Session directory: {}",
        session_logger.session_dir().display()
    );

    // Get stage loggers for wrapping transports
    let stage0_logger = session_logger.stage_logger("stage0".to_string());

    // Wrap stdio with logging
    use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
    let stdout =
        logging::LoggingWriter::new(tokio::io::stdout().compat_write(), stage0_logger.clone());
    let stdin = logging::LoggingReader::new(tokio::io::stdin().compat(), stage0_logger);

    // Create conductor with lazy initialization
    // The closure will be called when Initialize is received
    let conductor = symposium_conductor()?;

    // Convert to handler chain and serve
    conductor
        .into_handler_chain()
        .connect_to(sacp::ByteStreams::new(stdout, stdin))?
        .serve()
        .await?;

    Ok(())
}

/// Create and return the "symposium conductor", which assembles the symposium libraries together.
pub fn symposium_conductor() -> Result<Conductor> {
    // Create conductor with lazy initialization using ComponentList trait
    // The closure receives the Initialize request and returns components to spawn
    let conductor = Conductor::new(
        "symposium".to_string(),
        |init_req| async move {
            tracing::info!("Building proxy chain based on capabilities");

            // TODO: Examine init_req.capabilities to determine what's needed

            let components: Vec<Box<dyn Component>> =
                vec![Box::new(rust_crate_sources_proxy::CrateSourcesProxy {})];

            // TODO: Add more components based on capabilities
            // - Check for IDE operation capabilities
            // - Spawn ide-ops adapter if missing
            // - Spawn ide-ops component to provide MCP tools

            Ok((init_req, components))
        },
        None, // No custom conductor command
    );

    Ok(conductor)
}
