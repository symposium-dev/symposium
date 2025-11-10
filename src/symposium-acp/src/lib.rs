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
use futures::channel::mpsc;
use sacp::schema::InitializeRequest;
use sacp::JrConnectionCx;
use sacp_conductor::conductor::{Conductor, ConductorMessage};

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
    let conductor = Conductor::new(
        "symposium".to_string(),
        build_proxy_chain,
        None, // No custom conductor command
    );

    // Convert to handler chain and serve
    conductor
        .into_handler_chain()
        .connect_to(sacp::ByteStreams::new(stdout, stdin))?
        .serve()
        .await?;

    Ok(())
}

/// Build the proxy chain based on editor capabilities
///
/// This is called by the conductor when it receives the Initialize request.
/// We examine the client capabilities and decide which components to spawn.
///
/// For now, this is a stub that just forwards to a simple agent proxy.
/// Future enhancements will:
/// - Check for IDE operation capabilities
/// - Spawn ide-ops adapter if missing
/// - Spawn ide-ops component to provide MCP tools
/// - Add other components as needed
async fn build_proxy_chain(
    _cx: JrConnectionCx,
    _conductor_tx: mpsc::Sender<ConductorMessage>,
    init_req: InitializeRequest,
) -> Result<(InitializeRequest, Vec<JrConnectionCx>), sacp::Error> {
    tracing::info!("Building proxy chain based on capabilities");

    // TODO: Examine init_req.client_capabilities to determine what's needed
    // TODO: Spawn components/adapters based on capability gaps

    // For now, just create a passthrough to the agent
    // This will be replaced with actual component spawning logic
    let agent = create_agent_proxy(&_cx, &_conductor_tx).await?;

    Ok((init_req, vec![agent]))
}

/// Create a proxy to the downstream agent
///
/// For now this is a stub. Eventually this will connect to the actual
/// agent specified in configuration or command line arguments.
async fn create_agent_proxy(
    _cx: &JrConnectionCx,
    _conductor_tx: &mpsc::Sender<ConductorMessage>,
) -> Result<JrConnectionCx, sacp::Error> {
    // TODO: Actually spawn or connect to an agent
    // For now, just error since we don't have a real agent yet
    Err(sacp::Error::internal_error())
}
