#!/usr/bin/env cargo
//! Symposium Development Setup Tool
//!
//! Installs ACP binaries, VSCode extension, and configures Zed editor
//! for use with Symposium.

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use std::path::PathBuf;

mod acp;
mod vscode;
mod zed;

#[derive(Parser)]
#[command(
    name = "setup",
    about = "Install Symposium components and configure editors",
    long_about = r#"
Install Symposium components and configure editors

Examples:
  cargo setup --all                    # Install everything (ACP binaries, VSCode extension, Zed config)
  cargo setup --acp                    # Install ACP binaries only
  cargo setup --vscode                 # Install VSCode extension only
  cargo setup --zed                    # Configure Zed editor only
  cargo setup --acp --zed              # Install ACP binaries and configure Zed

Prerequisites:
  - Rust and Cargo (https://rustup.rs/)
  - Node.js and npm (for VSCode extension)
  - VSCode with 'code' command (for --vscode)
  - Zed editor (for --zed)
  - Claude Code or Codex CLI (for --zed agent detection)
"#
)]
struct Args {
    /// Install all components (ACP binaries, VSCode extension, and configure Zed)
    #[arg(long)]
    all: bool,

    /// Install ACP binaries (sacp-conductor, elizacp, sacp-tee, symposium-acp-proxy)
    #[arg(long)]
    acp: bool,

    /// Build and install VSCode extension
    #[arg(long)]
    vscode: bool,

    /// Configure Zed editor with detected agents
    #[arg(long)]
    zed: bool,

    /// Dry run - show what would be done without making changes
    #[arg(long)]
    dry_run: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Show help if no components specified
    if !args.all && !args.acp && !args.vscode && !args.zed {
        show_help();
        return Ok(());
    }

    // Determine what to install
    // --zed implies --acp since Zed config points to ACP binaries
    let install_acp = args.all || args.acp || args.zed;
    let install_vscode = args.all || args.vscode;
    let configure_zed = args.all || args.zed;

    println!("🎭 Symposium Setup");
    println!("{}", "=".repeat(35));

    if args.dry_run {
        println!("🔍 DRY RUN MODE - No changes will be made");
        println!();
    }

    // Get repository root
    let repo_root = get_repo_root()?;
    println!("📁 Repository: {}", repo_root.display());
    println!();

    // Check prerequisites based on what we're installing
    check_rust()?;
    if install_vscode {
        vscode::check_node_available()?;
        vscode::check_vscode_available()?;
    }

    // Install components
    if install_acp {
        acp::install_acp_binaries(&repo_root, args.dry_run)?;
        println!();
    }

    if install_vscode {
        vscode::build_and_install_extension(&repo_root, args.dry_run)?;
        println!();
    }

    if configure_zed {
        let symposium_acp_agent_path = acp::get_binary_path("symposium-acp-agent")?;
        zed::configure_zed(&symposium_acp_agent_path, args.dry_run)?;
        println!();
    }

    print_completion_message(install_acp, install_vscode, configure_zed)?;

    Ok(())
}

fn show_help() {
    println!("🎭 Symposium Setup");
    println!("{}", "=".repeat(35));
    println!();
    println!("Common examples:");
    println!("  cargo setup --all                    # Install everything");
    println!("  cargo setup --acp --zed              # Install ACP and configure Zed");
    println!("  cargo setup --vscode                 # Install VSCode extension only");
    println!("  cargo setup --help                   # See all options");
}

fn check_rust() -> Result<()> {
    if which::which("cargo").is_err() {
        return Err(anyhow!(
            "❌ Cargo not found. Please install Rust first.\n   Visit: https://rustup.rs/"
        ));
    }
    Ok(())
}

fn print_completion_message(
    installed_acp: bool,
    installed_vscode: bool,
    configured_zed: bool,
) -> Result<()> {
    println!("🎉 Setup complete!");
    println!();

    if installed_acp {
        println!("📦 ACP binaries installed to ~/.cargo/bin/:");
        println!("   • elizacp");
        println!("   • symposium-acp-agent");
        println!();
    }

    if installed_vscode {
        println!("📋 VSCode extension installed");
        println!("   Next steps:");
        println!("   1. Restart VSCode to activate the extension");
        println!("   2. Use Symposium panel for AI interactions");
        println!();
    }

    if configured_zed {
        let agents = zed::detect_zed_agents();
        if !agents.is_empty() {
            println!("🔧 Zed configured with {} agent(s)", agents.len());
            println!("   Restart Zed to use the new configuration");
            println!();
        }
    }

    Ok(())
}

fn get_repo_root() -> Result<PathBuf> {
    // Require CARGO_MANIFEST_DIR - only available when running via cargo
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").context(
        "❌ Setup tool must be run via cargo (e.g., 'cargo setup'). CARGO_MANIFEST_DIR not found.",
    )?;

    let manifest_path = PathBuf::from(manifest_dir);
    // If we're in a workspace member (like setup/), go up to workspace root
    if manifest_path.file_name() == Some(std::ffi::OsStr::new("setup")) {
        if let Some(parent) = manifest_path.parent() {
            return Ok(parent.to_path_buf());
        }
    }
    Ok(manifest_path)
}
