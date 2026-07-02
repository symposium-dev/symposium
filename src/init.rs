//! Init command: `cargo agents init`.

use std::io::IsTerminal;

use anyhow::{Context, Result};
use dialoguer::MultiSelect;

use crate::agents::Agent;
use crate::config::{AgentEntry, Symposium};
use crate::output::{Output, display_path};

/// Options that can be provided on the command line to skip interactive prompts.
#[derive(Debug, Default)]
pub struct InitOpts {
    /// Agent names provided via `--add-agent`. If non-empty, skips the agent prompt.
    pub agents: Vec<String>,
    /// Agent names to remove via `--remove-agent`.
    pub remove_agents: Vec<String>,
    /// Explicit hook scope override.
    pub hook_scope: Option<crate::config::HookScope>,
}

/// Whether we can prompt the user interactively.
fn interactive(out: &Output) -> bool {
    !out.is_quiet() && std::io::stdin().is_terminal()
}

/// Resolve which agents to configure. Priority:
/// 1. Explicit `--add-agent` / `--remove-agent` flags (applied to existing set)
/// 2. Interactive multi-select (if `should_prompt`), pre-selecting existing agents
/// 3. Default to first agent (Claude) in non-interactive mode
fn resolve_agents(
    opts: &InitOpts,
    existing: &[AgentEntry],
    should_prompt: bool,
) -> Result<Vec<Agent>> {
    if !opts.agents.is_empty() || !opts.remove_agents.is_empty() {
        let mut names: Vec<String> = existing.iter().map(|e| e.name.clone()).collect();
        for name in &opts.agents {
            Agent::from_config_name(name)?;
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
        for name in &opts.remove_agents {
            Agent::from_config_name(name)?;
            names.retain(|n| n != name);
        }
        return names.iter().map(|n| Agent::from_config_name(n)).collect();
    }
    if should_prompt {
        return prompt_for_agents(existing);
    }
    if !existing.is_empty() {
        return existing
            .iter()
            .map(|e| Agent::from_config_name(&e.name))
            .collect();
    }
    Ok(vec![Agent::all()[0]])
}

/// Run user-wide initialization.
///
/// Prompts for agents (unless provided), writes
/// `~/.symposium/config.toml`, and registers global hooks.
pub async fn init(sym: &mut Symposium, out: &Output, opts: &InitOpts) -> Result<()> {
    tracing::info!("init started");
    out.println("Setting up symposium for your user account.\n");

    // CLI flags signal non-interactive intent — skip all prompts.
    let cli_driven = !opts.agents.is_empty() || !opts.remove_agents.is_empty();
    let should_prompt = !cli_driven && interactive(out);

    // Resolve each setting: CLI flag > interactive prompt > keep existing.
    let agents = resolve_agents(opts, &sym.config.agents, should_prompt)?;

    sym.config.agents = agents
        .iter()
        .map(|a| AgentEntry {
            name: a.config_name().to_string(),
        })
        .collect();

    sym.config.hook_scope = match opts.hook_scope {
        Some(scope) => scope,
        None if should_prompt && !agents.is_empty() => {
            prompt_for_hook_scope(sym.config.hook_scope)?
        }
        None => sym.config.hook_scope,
    };

    if should_prompt && !agents.is_empty() {
        sym.config.auto_update = prompt_for_auto_update(sym.config.auto_update)?;
        sym.config.telemetry.enabled = prompt_for_telemetry(sym.config.telemetry.enabled)?;
    }

    tracing::debug!(
        agents = ?agents.iter().map(|a| a.config_name()).collect::<Vec<_>>(),
        scope = ?sym.config.hook_scope,
        auto_update = ?sym.config.auto_update,
        "resolved config"
    );

    // Persist and apply.
    sym.save_config().context("failed to write user config")?;

    let config_path = sym.config_dir().join("config.toml");

    if agents.is_empty() {
        // Uninstall: unregister all hooks and MCP servers for every agent.
        crate::sync::register_hooks(sym, out).context("failed to unregister hooks")?;
        out.done(format!(
            "{}: wrote user config (no agents — symposium uninstalled)",
            display_path(&config_path),
        ));
        return Ok(());
    }

    let agent_names: Vec<_> = agents.iter().map(|a| a.display_name()).collect();
    out.done(format!(
        "{}: wrote user config (agents: {})",
        display_path(&config_path),
        agent_names.join(", ")
    ));

    if sym.config.hook_scope == crate::config::HookScope::Global {
        crate::sync::register_hooks(sym, out).context("failed to register global hooks")?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Interactive prompts
// ---------------------------------------------------------------------------

fn prompt_for_hook_scope(current: crate::config::HookScope) -> Result<crate::config::HookScope> {
    use crate::config::HookScope;

    let items = ["Globally (recommended)", "Per project"];
    let default = match current {
        HookScope::Global => 0,
        HookScope::Project => 1,
    };

    let selection = dialoguer::Select::new()
        .with_prompt("Install hooks and agent configuration")
        .items(items)
        .default(default)
        .interact()?;

    Ok(match selection {
        0 => HookScope::Global,
        _ => HookScope::Project,
    })
}

fn prompt_for_auto_update(current: crate::config::AutoUpdate) -> Result<crate::config::AutoUpdate> {
    use crate::config::AutoUpdate;

    let items = [
        "Auto-update (recommended)",
        "Warn when updates are available",
        "Off",
    ];
    let default = match current {
        AutoUpdate::On => 0,
        AutoUpdate::Warn => 1,
        AutoUpdate::Off => 2,
    };

    let selection = dialoguer::Select::new()
        .with_prompt("Automatic updates")
        .items(items)
        .default(default)
        .interact()?;

    Ok(match selection {
        0 => AutoUpdate::On,
        1 => AutoUpdate::Warn,
        _ => AutoUpdate::Off,
    })
}

fn prompt_for_telemetry(current: bool) -> Result<bool> {
    Ok(dialoguer::Confirm::new()
        .with_prompt(
            "Enable anonymous usage telemetry? It is stored locally under \
             ~/.symposium/telemetry/ and never uploaded automatically — you can review \
             it with `cargo agents telemetry show` and share it yourself",
        )
        .default(current)
        .interact()?)
}

fn prompt_for_agents(existing: &[AgentEntry]) -> Result<Vec<Agent>> {
    let agents = Agent::all();
    let items: Vec<&str> = agents.iter().map(|a| a.display_name()).collect();

    let defaults: Vec<bool> = agents
        .iter()
        .map(|a| existing.iter().any(|e| e.name == a.config_name()))
        .collect();

    let selections = MultiSelect::new()
        .with_prompt("Which agents do you use? (space to select, enter to confirm)")
        .items(items)
        .defaults(&defaults)
        .interact()?;

    Ok(selections.into_iter().map(|i| agents[i]).collect())
}
