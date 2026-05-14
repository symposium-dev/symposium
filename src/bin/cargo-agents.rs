use clap::Parser;
use std::process::ExitCode;

use symposium::cli::{Cli, Commands, PluginCommand};
use symposium::config::{self, AutoUpdate};
use symposium::hook;
use symposium::output::Output;
use symposium::plugins;
use symposium::self_update;
use symposium::state;

#[tokio::main]
async fn main() -> ExitCode {
    let mut sym = config::Symposium::from_environment();
    sym.init_logging();

    // When invoked as `cargo agents`, cargo passes "agents" as the first arg.
    // Strip it so clap sees the real arguments.
    let args: Vec<_> = std::env::args_os().collect();
    let filtered: Vec<_> = if args.len() > 1 && args[1] == "agents" {
        std::iter::once(args[0].clone())
            .chain(args[2..].iter().cloned())
            .collect()
    } else {
        args
    };
    let cli = Cli::parse_from(filtered);

    // Log the command being invoked
    match &cli.command {
        Some(Commands::Init { .. }) => tracing::info!("cargo agents init"),
        Some(Commands::Sync) => tracing::info!("cargo agents sync"),
        Some(Commands::Plugin { command }) => {
            tracing::info!(subcommand = ?command, "cargo agents plugin");
        }
        Some(Commands::Hook { agent, event }) => {
            tracing::debug!(?agent, ?event, "cargo agents hook");
        }
        Some(Commands::SelfUpdate) => tracing::info!("cargo agents self-update"),
        Some(Commands::CrateInfo { name, version }) => {
            tracing::debug!(%name, version = ?version, "cargo agents crate-info");
        }
        None => {}
    }

    // Stamp state.toml with the running binary version (silently updates on mismatch).
    state::ensure_current(sym.config_dir());

    // Hook commands are quiet by default (they're invoked by the agent, not the user)
    let is_hook = matches!(cli.command, Some(Commands::Hook { .. }));
    let out = if cli.quiet || is_hook {
        Output::quiet()
    } else {
        Output::normal()
    };

    // Ensure git-based plugin sources are up to date (non-blocking on failure).
    plugins::ensure_plugin_sources(&sym, cli.update).await;

    // Auto-update check (skip for self-update itself, which always checks).
    // For hooks: "warn" is skipped (stderr isn't visible to the agent), but
    // "on" still auto-updates silently so the next invocation uses the new binary.
    if !matches!(cli.command, Some(Commands::SelfUpdate))
        && sym.config.auto_update != AutoUpdate::Off
        && state::should_check_for_update(sym.config_dir())
    {
        state::record_update_check(sym.config_dir());
        match sym.config.auto_update {
            AutoUpdate::Off => unreachable!(),
            AutoUpdate::Warn if !is_hook => {
                if let Ok(Some(latest)) = self_update::check_upgrade() {
                    out.warn(format!(
                        "symposium {} is available (current: {}). \
                         Run `cargo agents self-update` to upgrade.",
                        latest,
                        state::CURRENT_VERSION,
                    ));
                }
            }
            AutoUpdate::On => {
                if let Ok(Some(latest)) = self_update::check_upgrade() {
                    if !is_hook {
                        out.info(format!(
                            "auto-updating symposium {} → {latest}...",
                            state::CURRENT_VERSION,
                        ));
                    }
                    if let Err(e) =
                        self_update::self_update(&Output::quiet(), sym.config.update_source).await
                    {
                        if !is_hook {
                            out.warn(format!("auto-update failed: {e}"));
                        }
                    } else {
                        self_update::re_exec();
                    }
                }
            }
            _ => {}
        }
    }

    let cwd = std::env::current_dir().expect("failed to get current directory");

    match cli.command {
        // Commands that need direct I/O (stdin/stdout) stay in the binary
        Some(Commands::Hook { agent, event }) => hook::run(&sym, agent, event).await,

        Some(Commands::Plugin { command }) => handle_plugin_command(&sym, command).await,

        None => {
            use clap::CommandFactory;
            Cli::command().print_help().ok();
            println!();
            ExitCode::SUCCESS
        }

        // Everything else delegates to the library
        Some(cmd) => match symposium::cli::run(&mut sym, cmd, &cwd, &out).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("Error: {e:#}");
                ExitCode::FAILURE
            }
        },
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
                            errors += print_validation_result(r, "  ");
                        }
                        let total = count_results(&results);
                        let passed = total - errors;
                        println!("\n  {passed}/{total} valid");
                    }
                    Err(e) => {
                        eprintln!("✗ {}: {e}", path.display());
                        return ExitCode::FAILURE;
                    }
                }

                if !no_check_crates {
                    match plugins::collect_crate_names_in_source_dir(&path) {
                        Ok(crate_names) => {
                            if !crate_names.is_empty() {
                                println!(
                                    "\n📦 Checking {} crate name(s) on crates.io...",
                                    crate_names.len()
                                );
                                for name in &crate_names {
                                    if plugins::check_crate_exists(name).await {
                                        println!("  ✅ {name}");
                                    } else {
                                        eprintln!("  ✗ {name} — not found on crates.io");
                                        errors += 1;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("✗ failed to collect crate names: {e}");
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
                let parent = path.parent().unwrap_or(&path);
                match plugins::load_plugin(&path, "", parent) {
                    Ok(p) => {
                        println!("{}", tokio::fs::read_to_string(p.path).await.unwrap());
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
            Some(p) => {
                println!("# Source: {}", p.path.display());
                println!();
                print!("{}", tokio::fs::read_to_string(p.path).await.unwrap());
                ExitCode::SUCCESS
            }
            None => {
                eprintln!("Plugin not found: {plugin}");
                ExitCode::FAILURE
            }
        },
    }
}

fn print_validation_result(r: &plugins::ValidationResult, indent: &str) -> usize {
    let mut errors = 0;
    match &r.result {
        Ok(()) => {
            if let Some(ref w) = r.warning {
                println!("{indent}⚠️  {} ({}): {w}", r.path.display(), r.kind);
            } else {
                println!("{indent}✅ {} ({})", r.path.display(), r.kind);
            }
        }
        Err(e) => {
            eprintln!("{indent}✗ {} ({}): {e}", r.path.display(), r.kind);
            errors += 1;
        }
    }
    let child_indent = format!("{indent}    ");
    for child in &r.children {
        errors += print_validation_result(child, &child_indent);
    }
    errors
}

fn count_results(results: &[plugins::ValidationResult]) -> usize {
    results.iter().map(|r| 1 + count_results(&r.children)).sum()
}
