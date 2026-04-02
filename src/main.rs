use clap::{Parser, Subcommand};
use std::process::ExitCode;

use crate::plugins::ParsedPlugin;

mod advice_for;
mod config;
mod crate_sources;
mod git_source;
mod hook;
mod mcp;
mod plugins;
mod skills;
pub mod tutorial;

#[derive(Parser)]
#[command(name = "symposium", version, about = "AI the Rust Way")]
struct Cli {
    /// Control plugin source update behavior (none, check, fetch)
    #[arg(long, global = true, default_value = "none")]
    update: git_source::UpdateLevel,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Show the Symposium tutorial for agents and humans
    Tutorial,

    /// Run as an MCP server (stdio transport)
    Mcp,

    /// Handle a hook event (invoked by editor plugins)
    Hook {
        /// The hook event (e.g., claude:pre-tool)
        event: hook::HookEvent,
    },

    /// Get Rust development guidance
    Rust {
        /// The command to run (e.g., "help")
        #[arg(default_value = "help")]
        command: String,
    },

    /// Find crate sources and guidance
    Crate {
        /// Crate name (omit to use --list)
        name: Option<String>,

        /// Version constraint (e.g., "1.0.3", "^1.0"); defaults to workspace version or latest
        #[arg(long)]
        version: Option<String>,

        /// List all workspace dependency crates
        #[arg(long)]
        list: bool,
    },

    /// Manage plugins
    Plugin {
        #[command(subcommand)]
        command: PluginCommand,
    },
}

#[derive(Subcommand)]
enum PluginCommand {
    /// Sync plugin sources from git repositories
    Sync {
        /// Provider name to sync (omit to sync all)
        provider: Option<String>,
    },

    /// List all providers and their plugins
    List,

    /// Show details for a specific plugin
    Show {
        /// Plugin name
        plugin: String,
    },

    /// Validate a plugin source directory or a single TOML manifest
    Validate {
        /// Path to a directory (scanned for .toml plugins and SKILL.md files) or a single .toml file
        path: std::path::PathBuf,

        /// Check that crate names in advice-for/applies-when predicates exist on crates.io
        #[arg(long)]
        check_crates: bool,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    config::init();

    let cli = Cli::parse();

    // Ensure git-based plugin sources are up to date (non-blocking on failure).
    plugins::ensure_plugin_sources(cli.update).await;

    match cli.command {
        Some(Commands::Tutorial) => {
            print!("{}", tutorial::render_cli());
            ExitCode::SUCCESS
        }
        Some(Commands::Mcp) => match mcp::serve().await {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("MCP server error: {e}");
                ExitCode::FAILURE
            }
        },
        Some(Commands::Hook { event }) => hook::run(event).await,
        Some(Commands::Rust { command }) => {
            print!("{}", mcp::execute_rust_command(&command));
            ExitCode::SUCCESS
        }
        Some(Commands::Crate {
            name,
            version,
            list,
        }) => {
            let cwd = std::env::current_dir().expect("failed to get current directory");

            if list {
                let workspace = crate_sources::workspace_semver_pairs(&cwd);
                let registry = plugins::load_registry();
                print!("{}", skills::list_output(&registry, &workspace).await);
                ExitCode::SUCCESS
            } else if let Some(name) = name {
                let workspace = crate_sources::workspace_semver_pairs(&cwd);
                let registry = plugins::load_registry();
                match skills::info_output(&name, version.as_deref(), &registry, &workspace).await {
                    Ok(output) => {
                        print!("{output}");
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("Error: {e}");
                        ExitCode::FAILURE
                    }
                }
            } else {
                eprintln!("Provide a crate name or use --list");
                ExitCode::FAILURE
            }
        }
        Some(Commands::Plugin { command }) => match command {
            PluginCommand::Sync { provider } => {
                match plugins::sync_plugin_source(provider.as_deref()).await {
                    Ok(synced) => {
                        if synced.is_empty() {
                            if let Some(ref p) = provider {
                                println!("No git source found for provider: {p}");
                            } else {
                                println!("No git sources to sync.");
                            }
                        } else {
                            for name in &synced {
                                println!("Synced: {name}");
                            }
                        }
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("Sync failed: {e}");
                        ExitCode::FAILURE
                    }
                }
            }
            PluginCommand::List => {
                let providers = plugins::list_plugins();
                for provider in &providers {
                    println!("Provider: {}", provider.name);
                    println!("  Type: {}", provider.source_type);
                    if let Some(ref url) = provider.git_url {
                        println!("  URL: {url}");
                    }
                    if let Some(ref path) = provider.path {
                        println!("  Path: {path}");
                    }
                    if provider.plugins.is_empty() {
                        println!("  (no plugins)");
                    } else {
                        for plugin in &provider.plugins {
                            println!(
                                "  - {} ({} hooks, {} skill groups)",
                                plugin.name, plugin.hooks_count, plugin.skill_groups_count
                            );
                        }
                    }
                    println!();
                }
                ExitCode::SUCCESS
            }
            PluginCommand::Validate { path, check_crates } => {
                if path.is_dir() {
                    let mut errors = 0;

                    // Structural validation
                    match plugins::validate_source_dir(&path) {
                        Ok(results) => {
                            if results.is_empty() {
                                eprintln!("No plugins or skills found in {}", path.display());
                                return ExitCode::FAILURE;
                            }
                            for r in &results {
                                match &r.result {
                                    Ok(()) => {
                                        println!("ok: {} ({})", r.path.display(), r.kind);
                                    }
                                    Err(e) => {
                                        eprintln!("FAIL: {} ({}): {e}", r.path.display(), r.kind);
                                        errors += 1;
                                    }
                                }
                            }
                            let total = results.len();
                            let passed = total - errors;
                            println!("\n{passed}/{total} valid");
                        }
                        Err(e) => {
                            eprintln!("{}: {e}", path.display());
                            return ExitCode::FAILURE;
                        }
                    }

                    // Crate existence check
                    if check_crates {
                        match plugins::collect_crate_names_in_source_dir(&path) {
                            Ok(crate_names) => {
                                if !crate_names.is_empty() {
                                    println!(
                                        "\nChecking {} crate name(s) on crates.io...",
                                        crate_names.len()
                                    );
                                    for name in &crate_names {
                                        if plugins::check_crate_exists(name).await {
                                            println!("  ok: {name}");
                                        } else {
                                            eprintln!("  FAIL: {name} not found on crates.io");
                                            errors += 1;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("failed to collect crate names: {e}");
                                errors += 1;
                            }
                        }
                    }

                    if errors > 0 {
                        ExitCode::FAILURE
                    } else {
                        ExitCode::SUCCESS
                    }
                } else {
                    match plugins::load_plugin(&path) {
                        Ok(ParsedPlugin { path: _, plugin }) => {
                            println!("{}", toml::to_string_pretty(&plugin).unwrap());
                            ExitCode::SUCCESS
                        }
                        Err(e) => {
                            eprintln!("{}: {e}", path.display());
                            ExitCode::FAILURE
                        }
                    }
                }
            }
            PluginCommand::Show { plugin } => match plugins::find_plugin(&plugin) {
                Some(ParsedPlugin { path, plugin }) => {
                    println!("# Source: {}", path.display());
                    println!();
                    print!("{}", toml::to_string_pretty(&plugin).unwrap());
                    ExitCode::SUCCESS
                }
                None => {
                    eprintln!("Plugin not found: {plugin}");
                    ExitCode::FAILURE
                }
            },
        },
        None => {
            println!("symposium — AI the Rust Way");
            println!();
            println!("Usage: symposium <command>");
            println!();
            println!("Commands:");
            println!("  tutorial   Show the Symposium tutorial for agents and humans");
            println!("  rust       Get Rust development guidance");
            println!("  crate      Find crate sources and guidance");
            println!("  plugin     Manage plugins");
            println!("  mcp        Run as an MCP server (stdio transport)");
            println!("  hook       Handle a hook event (invoked by editor plugins)");
            println!("  help       Show this message");
            println!();
            println!("Run `symposium <command> --help` for more information.");
            ExitCode::SUCCESS
        }
    }
}
