//! Init command: `symposium init`.

use std::io::IsTerminal;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
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
/// 2. Interactive multi-select (if terminal), pre-selecting existing agents
/// 3. Default to first agent (Claude) in non-interactive mode
fn resolve_agents(opts: &InitOpts, existing: &[AgentEntry], out: &Output) -> Result<Vec<Agent>> {
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
    if interactive(out) {
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

    let agents = resolve_agents(opts, &sym.config.agents, out)?;
    tracing::debug!(agents = ?agents.iter().map(|a| a.config_name()).collect::<Vec<_>>(), "resolved agents");

    sym.config.agents = agents
        .iter()
        .map(|a| AgentEntry {
            name: a.config_name().to_string(),
        })
        .collect();

    if let Some(scope) = opts.hook_scope {
        sym.config.hook_scope = scope;
    } else if interactive(out) {
        sym.config.hook_scope = prompt_for_hook_scope(sym.config.hook_scope)?;
    }
    tracing::debug!(scope = ?sym.config.hook_scope, "hook scope");

    sym.save_config().context("failed to write user config")?;
    tracing::info!(
        agents = ?agents.iter().map(|a| a.config_name()).collect::<Vec<_>>(),
        scope = ?sym.config.hook_scope,
        "config written"
    );

    let config_path = sym.config_dir().join("config.toml");
    let agent_names: Vec<_> = agents.iter().map(|a| a.display_name()).collect();
    out.done(format!(
        "{}: wrote user config (agents: {})",
        display_path(&config_path),
        agent_names.join(", ")
    ));

    // Register global hooks (project-scope hooks are installed by `sync`).
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
        .items(&items)
        .default(default)
        .interact()?;

    Ok(match selection {
        0 => HookScope::Global,
        _ => HookScope::Project,
    })
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
        .items(&items)
        .defaults(&defaults)
        .interact()?;

    if selections.is_empty() {
        bail!("at least one agent must be selected");
    }

    Ok(selections.into_iter().map(|i| agents[i]).collect())
}

// ---------------------------------------------------------------------------
// Workspace detection
// ---------------------------------------------------------------------------

/// Find the workspace root using `cargo metadata`, run from the given directory.
pub fn find_workspace_root(cwd: &std::path::Path) -> Result<PathBuf> {
    let output = std::process::Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version=1"])
        .current_dir(cwd)
        .output()
        .context("failed to run cargo metadata")?;

    if !output.status.success() {
        bail!("not in a Rust workspace (cargo metadata failed)");
    }

    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("failed to parse cargo metadata output")?;

    let workspace_root = metadata
        .get("workspace_root")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("cargo metadata missing workspace_root"))?;

    Ok(PathBuf::from(workspace_root))
}
