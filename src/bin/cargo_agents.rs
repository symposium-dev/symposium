use clap::Parser;
use std::process::ExitCode;

use cargo_agents::cli::{Cli, Commands, PluginCommand};
use cargo_agents::config;
use cargo_agents::hook;
use cargo_agents::mcp;
use cargo_agents::output::Output;
use cargo_agents::plugins::{self, ParsedPlugin};

#[tokio::main]
async fn main() -> ExitCode {
    // When invoked as `cargo agents`, cargo passes "agents" as the first arg.
    // Strip it so clap sees the right subcommand.
    let args: Vec<String> = std::env::args().collect();
    let args = if args.len() > 1 && args[1] == "agents" {
        let mut filtered = vec![args[0].clone()];
        filtered.extend_from_slice(&args[2..]);
        filtered
    } else {
        args
    };

    let mut sym = config::Symposium::from_environment();
    sym.init_logging();

    let cli = Cli::parse_from(&args);

    // Hook commands are quiet by default (they're invoked by the agent, not the user)
    let is_hook = matches!(cli.command, Some(Commands::Hook { .. }));
    let out = if cli.quiet || is_hook {
        Output::quiet()
    } else {
        Output::normal()
    };

    // Ensure git-based plugin sources are up to date (non-blocking on failure).
    plugins::ensure_plugin_sources(&sym, cli.update).await;

    let cwd = std::env::current_dir().expect("failed to get current directory");

    match cli.command {
        // Commands that need direct I/O (stdin/stdout) stay in the binary
        Some(Commands::Hook { agent, event }) => hook::run(&sym, agent, event, &cwd).await,

        Some(Commands::Mcp) => match mcp::serve(&sym, &cwd).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("MCP server error: {e}");
                ExitCode::FAILURE
            }
        },

        Some(Commands::Plugin { command }) => handle_plugin_command(&sym, command).await,

        None => {
            use clap::CommandFactory;
            Cli::command().print_help().ok();
            println!();
            ExitCode::SUCCESS
        }

        // Everything else delegates to the library
        Some(cmd) => {
            match cargo_agents::cli::run(&mut sym, cmd, &cwd, &out).await {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("Error: {e:#}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}

async fn handle_plugin_command(sym: &config::Symposium, command: PluginCommand) -> ExitCode {
    match command {
        PluginCommand::Sync { provider } => {
            match plugins::sync_plugin_source(sym, provider.as_deref()).await {
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
            let providers = plugins::list_plugins(sym);
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
        PluginCommand::Validate {
            path,
            no_check_crates,
        } => {
            if path.is_dir() {
                let mut errors = 0;

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

                if !no_check_crates {
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
        PluginCommand::Show { plugin } => match plugins::find_plugin(sym, &plugin) {
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
    }
}
