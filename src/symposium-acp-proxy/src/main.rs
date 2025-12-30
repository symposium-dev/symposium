//! Symposium ACP Proxy - Main entry point
//!
//! A proxy that sits between an editor and an existing agent, enriching the
//! connection with Symposium's capabilities.
//!
//! Usage:
//!   symposium-acp-proxy [OPTIONS]

use anyhow::Result;
use clap::Parser;
use sacp::Component;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "symposium-acp-proxy")]
#[command(about = "Symposium ACP proxy - enriches editor-agent connections")]
struct Cli {
    /// Enable or disable Sparkle integration (default: yes)
    #[arg(long, default_value = "yes", value_parser = parse_yes_no)]
    sparkle: bool,

    /// Enable or disable Ferris tools (default: yes)
    #[arg(long, default_value = "yes", value_parser = parse_yes_no)]
    ferris: bool,

    /// Comma-separated list of Ferris tools to enable.
    /// Available tools: crate_source, rust_researcher
    /// Default: crate_source
    #[arg(long, default_value = "crate_source", value_delimiter = ',')]
    ferris_tools: Vec<String>,

    /// Enable or disable Cargo tools (default: yes)
    #[arg(long, default_value = "yes", value_parser = parse_yes_no)]
    cargo: bool,

    /// Enable trace logging to the specified directory.
    /// Traces are written as timestamped .jsons files viewable with sacp-trace-viewer.
    #[arg(long)]
    trace_dir: Option<PathBuf>,

    /// Enable logging to stderr at the specified level (error, warn, info, debug, trace).
    #[arg(long)]
    log: Option<tracing::Level>,
}

fn parse_yes_no(s: &str) -> Result<bool, String> {
    match s.to_lowercase().as_str() {
        "yes" | "true" | "1" => Ok(true),
        "no" | "false" | "0" => Ok(false),
        _ => Err(format!("expected 'yes' or 'no', got '{}'", s)),
    }
}

fn build_ferris_config(enabled: bool, tools: &[String]) -> Option<symposium_ferris::Ferris> {
    if !enabled {
        return None;
    }

    let crate_sources = tools.iter().any(|t| t == "crate_source");
    let rust_researcher = tools.iter().any(|t| t == "rust_researcher");

    Some(
        symposium_ferris::Ferris::new()
            .crate_sources(crate_sources)
            .rust_researcher(rust_researcher),
    )
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Set up logging if requested
    if let Some(level) = cli.log {
        use tracing_subscriber::EnvFilter;
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::new(level.to_string()))
            .with_writer(std::io::stderr)
            .init();
    }

    // Run Symposium as a proxy
    let ferris_config = build_ferris_config(cli.ferris, &cli.ferris_tools);

    let mut symposium = symposium_acp_proxy::Symposium::new()
        .sparkle(cli.sparkle)
        .ferris(ferris_config)
        .cargo(cli.cargo);

    if let Some(trace_dir) = cli.trace_dir {
        symposium = symposium.trace_dir(trace_dir);
    }

    symposium.serve(sacp_tokio::Stdio::new()).await?;

    Ok(())
}
