use clap::Parser;
use std::env;
use std::process::ExitCode;

use symposium::cli::{Cli, Commands, PluginCommand};
use symposium::config;
use symposium::help_render;
use symposium::hook;
use symposium::output::Output;
use symposium::plugins;
use symposium::report;
use symposium::self_update;
use symposium::state;
use symposium::subcommand_dispatch::dispatch_external;

#[tokio::main]
async fn main() -> ExitCode {
    let mut sym = config::Symposium::from_environment();

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

    let cwd = env::current_dir().expect("failed to get current directory");

    // Parse without exiting on error: a help request on a built-in with required args
    // (`crate-info --help`, `plugin --help`) surfaces a parse error that `help_text` recovers.
    // `args_str` feeds the subcommand-name walk.
    let args_str = filtered
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let parse = Cli::try_parse_from(filtered);

    // `--help` / `-h` / `help` / no subcommand -> audience-grouped top-level help (or clap's
    // per-command help for `<built-in> --help`).
    // Plugin `<name> --help` returns `None` here and is forwarded to the child by dispatch below.
    if let Some(text) = help_render::help_text(parse.as_ref(), &args_str, &sym, &cwd).await {
        print!("{text}");
        return ExitCode::SUCCESS;
    }

    let cli = match parse {
        Ok(cli) => cli,
        Err(err) => err.exit(),
    };

    // Always install the report layer. Mode determines output format:
    // --json → accumulate JSON array; -v → stderr trace; default → stdout.
    // `mcp-serve` owns stdout for JSON-RPC. Reporting there would corrupt the
    // stream, so it goes to stderr whatever the flags say.
    let is_mcp_serve = matches!(cli.command, Some(Commands::McpServe));
    let (mode, level) = if is_mcp_serve {
        (
            report::ReportMode::Verbose,
            if cli.verbose {
                tracing::Level::DEBUG
            } else {
                tracing::Level::INFO
            },
        )
    } else if cli.json {
        let level = if cli.verbose {
            tracing::Level::DEBUG
        } else {
            tracing::Level::INFO
        };
        (report::ReportMode::Json, level)
    } else if cli.verbose {
        (report::ReportMode::Verbose, tracing::Level::DEBUG)
    } else {
        (report::ReportMode::Normal, tracing::Level::INFO)
    };
    let (report_layer, report_handle) = report::ReportLayer::new(mode, level);
    sym.init_logging(Some(report_layer));

    // Log the command being invoked
    match &cli.command {
        Some(Commands::Init { .. }) => tracing::info!("cargo agents init"),
        Some(Commands::Sync) => tracing::info!("cargo agents sync"),
        Some(Commands::Search { query }) => tracing::info!(%query, "cargo agents search"),
        Some(Commands::Use {
            name,
            global,
            remove,
        }) => tracing::info!(%name, global, remove, "cargo agents use"),
        Some(Commands::Status) => tracing::info!("cargo agents status"),
        Some(Commands::Plugin { command }) => {
            tracing::info!(subcommand = ?command, "cargo agents plugin");
        }
        Some(Commands::Hook { agent, event }) => {
            tracing::debug!(?agent, ?event, "cargo agents hook");
        }
        Some(Commands::McpServe) => tracing::debug!("cargo agents mcp-serve"),
        Some(Commands::SelfUpdate) => tracing::info!("cargo agents self-update"),
        Some(Commands::CrateInfo { name, version }) => {
            tracing::debug!(%name, version = ?version, "cargo agents crate-info");
        }
        Some(Commands::Telemetry { command }) => {
            tracing::info!(subcommand = ?command, "cargo agents telemetry");
        }
        Some(Commands::External(argv)) => {
            tracing::info!(argv = ?argv, "cargo agents <external>");
        }
        None => {}
    }

    // Stamp state.toml with the running binary version (silently updates on mismatch).
    state::ensure_current(sym.config_dir());

    // Hook commands are quiet by default (they're invoked by the agent, not the user).
    // JSON mode also suppresses human output (only JSON goes to stdout).
    let is_hook = matches!(cli.command, Some(Commands::Hook { .. }));
    let out = if cli.quiet || is_hook || cli.json || is_mcp_serve {
        Output::quiet()
    } else {
        Output::normal()
    };

    // Ensure git-based plugin sources are up to date (non-blocking on failure).
    // SessionStart runs once per session, so we force a real freshness check
    // there; other invocations use the `--update` level (debounced by default).
    let source_update = match &cli.command {
        Some(Commands::Hook { event, .. })
            if *event == symposium::hook::HookEvent::SessionStart =>
        {
            symposium_install::UpdateLevel::Check
        }
        _ => cli.update,
    };
    // Skipped for `mcp-serve`: a client may spawn a throwaway copy to probe
    // the server before the real session, so startup must not fetch or write.
    if !is_mcp_serve {
        plugins::ensure_registries(&sym, source_update).await;
    }

    // Auto-update = "on": check for updates and re-exec if a new binary was
    // installed.  Skipped for self-update (which always checks explicitly)
    // and for hooks (session-start injects the warn nudge into hook output;
    // the "on" re-exec for hooks is handled here).
    // Re-exec during an MCP session would drop the client's connection.
    if !matches!(cli.command, Some(Commands::SelfUpdate)) && !is_hook && !is_mcp_serve {
        if self_update::maybe_check_for_update(&sym, &out).await {
            self_update::re_exec();
        }
    } else if is_hook
        && !is_mcp_serve
        && sym.config.auto_update == config::AutoUpdate::On
        && self_update::maybe_check_for_update(&sym, &Output::quiet()).await
    {
        self_update::re_exec();
    }

    match cli.command {
        // Commands that need direct I/O (stdin/stdout) stay in the binary
        Some(Commands::Hook { agent, event }) => hook::run(&sym, agent, event).await,

        Some(Commands::McpServe) if !sym.config.experiments.mcp_meta_server => {
            // stderr, never stdout: an enabled run owns stdout for JSON-RPC.
            eprintln!(
                "Error: the MCP meta-server is experimental and off by default. \
                 Enable it in {}:\n\n[experiments]\nmcp-meta-server = true",
                sym.config_dir().join("config.toml").display()
            );
            ExitCode::FAILURE
        }

        Some(Commands::McpServe) => {
            let resolution = symposium::mcp::resolve::resolve(&sym, &cwd).await;
            for rejection in &resolution.rejected {
                tracing::warn!(
                    server = %rejection.server,
                    "skipping mcp server: {}",
                    rejection.reason
                );
            }
            let catalog = std::sync::Arc::new(symposium::mcp::catalog::Catalog::new(
                std::sync::Arc::new(sym.clone()),
                resolution,
                symposium::mcp::supervisor::RestartPolicy {
                    max_restarts: sym.config.mcp.max_server_restarts,
                    stable_reset: std::time::Duration::from_secs(
                        sym.config.mcp.restart_stable_reset_secs,
                    ),
                    shutdown_grace: std::time::Duration::from_secs(
                        sym.config.mcp.shutdown_grace_secs,
                    ),
                    ..Default::default()
                },
                sym.config.mcp.read_only,
                cwd.clone(),
            ));
            let limits = symposium::mcp::sandbox::Limits {
                timeout: std::time::Duration::from_secs(sym.config.mcp.script_timeout_secs),
                memory_bytes: (sym.config.mcp.script_memory_limit_mb as usize) << 20,
                stack_bytes: (sym.config.mcp.script_stack_limit_kb as usize) << 10,
                max_result_bytes: sym.config.mcp.max_result_bytes,
                max_console_bytes: sym.config.mcp.max_console_bytes,
            };
            match symposium::mcp::server::serve(catalog, limits).await {
                Ok(()) => ExitCode::SUCCESS,
                Err(err) => {
                    eprintln!("Error: {err:#}");
                    ExitCode::FAILURE
                }
            }
        }

        Some(Commands::Plugin { command }) => {
            let code = handle_plugin_command(&sym, command).await;
            let events = report_handle.drain();
            if !events.is_empty() {
                println!("{}", serde_json::to_string_pretty(&events).unwrap());
            }
            code
        }

        Some(Commands::External(argv)) => match dispatch_external(&sym, &cwd, argv).await {
            Ok(result) => {
                use std::io::Write;
                std::io::stdout().write_all(&result.stdout).ok();
                std::io::stderr().write_all(&result.stderr).ok();
                ExitCode::from(result.exit_code)
            }
            Err(err) => {
                eprintln!("Error: {err:#}");
                ExitCode::FAILURE
            }
        },
        // No-subcommand and the `help` keyword are handled by the help branch
        // right after parsing, above.
        None => unreachable!("no-subcommand routes to the help renderer above"),

        // Everything else delegates to the library
        Some(cmd) => match symposium::cli::run(&mut sym, cmd, &cwd, &out, cli.update).await {
            Ok(()) => {
                let events = report_handle.drain();
                if !events.is_empty() {
                    println!("{}", serde_json::to_string_pretty(&events).unwrap());
                }
                ExitCode::SUCCESS
            }
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
            match plugins::sync_registries(sym, provider.as_deref()).await {
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
            let providers = plugins::list_plugins(sym).await;
            for provider in &providers {
                tracing::info!(
                    report = %report::ReportEvent::ProviderListed {
                        name: provider.name.clone(),
                        source_type: provider.source_type.to_string(),
                        url: provider.git_url.clone(),
                        path: provider.path.clone(),
                        plugins: provider.plugins.iter().map(|p| p.name.clone()).collect(),
                    },
                );
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
                            errors += emit_validation_results(r);
                        }
                    }
                    Err(e) => {
                        eprintln!("✗ {}: {e}", path.display());
                        return ExitCode::FAILURE;
                    }
                }

                if !no_check_crates {
                    match plugins::collect_crate_names_in_source_dir(&path) {
                        Ok(crate_names) => {
                            for name in &crate_names {
                                let exists = plugins::check_crate_exists(name).await;
                                tracing::info!(
                                    report = %report::ReportEvent::Validated {
                                        path: name.clone(),
                                        item_kind: "crate".into(),
                                        valid: exists,
                                        error: if exists { None } else { Some("not found on crates.io".into()) },
                                        warning: None,
                                    },
                                );
                                if !exists {
                                    errors += 1;
                                }
                            }
                        }
                        Err(e) => {
                            tracing::info!(
                                report = %report::ReportEvent::Validated {
                                    path: path.display().to_string(),
                                    item_kind: "crate-check".into(),
                                    valid: false,
                                    error: Some(format!("failed to collect crate names: {e}")),
                                    warning: None,
                                },
                            );
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
                    Ok(_) => {
                        // `path` is the manifest file being validated.
                        println!("{}", tokio::fs::read_to_string(&path).await.unwrap());
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("{}: {e}", path.display());
                        ExitCode::FAILURE
                    }
                }
            }
        }
        PluginCommand::Show { plugin } => match plugins::find_plugin(sym, &plugin).await {
            // A plugin is identified by its id; render its effective (resolved)
            // configuration rather than re-reading a manifest file.
            Some(p) => {
                println!("# {}", p.canonical);
                match toml::to_string_pretty(&p.plugin) {
                    Ok(rendered) => {
                        println!();
                        print!("{rendered}");
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("cannot render `{}`: {e}", p.canonical);
                        ExitCode::FAILURE
                    }
                }
            }
            None => {
                eprintln!("Plugin not found: {plugin}");
                ExitCode::FAILURE
            }
        },
    }
}

fn emit_validation_results(r: &plugins::ValidationResult) -> usize {
    let mut errors = 0;
    match &r.result {
        Ok(()) => {
            tracing::info!(
                report = %report::ReportEvent::Validated {
                    path: r.id.clone(),
                    item_kind: r.kind.to_string(),
                    valid: true,
                    error: None,
                    warning: r.warning.clone(),
                },
            );
        }
        Err(e) => {
            tracing::info!(
                report = %report::ReportEvent::Validated {
                    path: r.id.clone(),
                    item_kind: r.kind.to_string(),
                    valid: false,
                    error: Some(e.to_string()),
                    warning: None,
                },
            );
            errors += 1;
        }
    }
    for child in &r.children {
        errors += emit_validation_results(child);
    }
    errors
}
