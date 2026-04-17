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
    out.println("Setting up symposium for your user account.\n");

    let agents = resolve_agents(opts, &sym.config.agents, out)?;

    sym.config.agents = agents
        .iter()
        .map(|a| AgentEntry {
            name: a.config_name().to_string(),
        })
        .collect();
    sym.save_config().context("failed to write user config")?;

    let config_path = sym.config_dir().join("config.toml");
    let agent_names: Vec<_> = agents.iter().map(|a| a.display_name()).collect();
    out.done(format!(
        "{}: wrote user config (agents: {})",
        display_path(&config_path),
        agent_names.join(", ")
    ));

    // Register global hooks
    crate::sync::register_hooks(sym, out)
        .context("failed to register global hooks")?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Interactive prompts
// ---------------------------------------------------------------------------

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
