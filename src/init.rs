//! Init commands: `init --user` and `init --project`.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use dialoguer::{Confirm, MultiSelect};

use crate::agents::Agent;
use crate::config::{AgentEntry, ProjectConfig, Symposium};
use crate::output::{Output, display_path};

const RTK_PLUGIN: &str = include_str!("../resources/plugins/rtk.toml");

/// Options that can be provided on the command line to skip interactive prompts.
#[derive(Debug, Default)]
pub struct InitOpts {
    /// Agent names provided via `--add-agent`. If non-empty, skips the agent prompt.
    pub agents: Vec<String>,
    /// Agent names to remove via `--remove-agent`.
    pub remove_agents: Vec<String>,
    /// Whether to add default plugins.
    pub no_default_plugins: bool,
}

/// Whether we can prompt the user interactively.
fn interactive() -> bool {
    std::io::stdin().is_terminal()
}

/// Resolve which agents to configure. Priority:
/// 1. Explicit `--add-agent` / `--remove-agent` flags (applied to existing set)
/// 2. Interactive multi-select (if terminal), pre-selecting existing agents
/// 3. Default to first agent (Claude) in non-interactive mode
fn resolve_user_agents(opts: &InitOpts, existing: &[AgentEntry]) -> Result<Vec<Agent>> {
    if !opts.agents.is_empty() || !opts.remove_agents.is_empty() {
        // Start from existing agents
        let mut names: Vec<String> = existing.iter().map(|e| e.name.clone()).collect();
        // Add new ones
        for name in &opts.agents {
            Agent::from_config_name(name)?; // validate
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
        // Remove specified ones
        for name in &opts.remove_agents {
            Agent::from_config_name(name)?; // validate
            names.retain(|n| n != name);
        }
        return names.iter().map(|n| Agent::from_config_name(n)).collect();
    }
    if interactive() {
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

/// Resolve which agents to add at the project level. Priority:
/// 1. Explicit `--add-agent` / `--remove-agent` flags (applied to existing set)
/// 2. Interactive prompt (if terminal)
/// 3. No project agents in non-interactive mode
fn resolve_project_agents(opts: &InitOpts, existing: &[AgentEntry]) -> Result<Vec<Agent>> {
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
    if interactive() {
        return prompt_for_project_agents();
    }
    Ok(Vec::new())
}

/// Run user-wide initialization.
///
/// Prompts for agents (unless provided), writes
/// `~/.symposium/config.toml`, and registers global hooks.
pub async fn init_user(sym: &mut Symposium, out: &Output, opts: &InitOpts) -> Result<()> {
    out.println("Setting up symposium for your user account.\n");

    if !opts.no_default_plugins {
        tokio::fs::write(sym.config_dir().join("plugins").join("rtk.toml"), RTK_PLUGIN).await?;
    }

    let agents = resolve_user_agents(opts, &sym.config.agents)?;

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
    crate::sync::sync_agent(sym, None, out)
        .await
        .context("failed to register global hooks")?;

    Ok(())
}

/// Run project-level initialization.
///
/// Finds workspace root from `cwd`, optionally prompts for project agents,
/// creates `.symposium/config.toml`, and runs sync.
pub async fn init_project(
    sym: &Symposium,
    cwd: &Path,
    out: &Output,
    opts: &InitOpts,
) -> Result<()> {
    let workspace_root = find_workspace_root(cwd)?;
    out.println(format!(
        "Setting up symposium for project at {}.\n",
        workspace_root.display()
    ));

    // Check if already initialized
    let config_dir = workspace_root.join(".symposium");
    if config_dir.join("config.toml").exists() {
        out.already_ok(".symposium/config.toml already exists, syncing");
    } else {
        let project_agents = resolve_project_agents(opts, &[])?;

        let config = ProjectConfig {
            agents: project_agents
                .iter()
                .map(|a| AgentEntry {
                    name: a.config_name().to_string(),
                })
                .collect(),
            ..Default::default()
        };
        config
            .save(&workspace_root)
            .context("failed to write project config")?;

        out.done("created .symposium/config.toml");
    }

    // Run sync --workspace to discover extensions
    crate::sync::sync_workspace(sym, &workspace_root, out)
        .await
        .context("sync --workspace failed")?;

    // Run sync --agent to install extensions
    crate::sync::sync_agent(sym, Some(&workspace_root), out)
        .await
        .context("sync --agent failed")?;

    out.blank();
    out.println("Project setup complete. Consider checking .symposium/ into version control.");
    Ok(())
}

/// Run the default init (both user and project as needed).
pub async fn init_default(
    sym: &mut Symposium,
    cwd: &Path,
    out: &Output,
    opts: &InitOpts,
) -> Result<()> {
    let user_config_exists =
        sym.config_dir().join("config.toml").exists() && !sym.config.agents.is_empty();

    if !user_config_exists {
        init_user(sym, out, opts).await?;
        out.blank();
    }

    // Check if we're in a Rust workspace
    if let Ok(workspace_root) = find_workspace_root(cwd) {
        let project_config_exists = workspace_root
            .join(".symposium")
            .join("config.toml")
            .exists();

        if !project_config_exists {
            let setup = if interactive() {
                Confirm::new()
                    .with_prompt("Set up this project?")
                    .default(true)
                    .interact()?
            } else {
                true
            };

            if setup {
                init_project(sym, cwd, out, opts).await?;
            }
        } else {
            out.info("user and project already configured, syncing");
            out.blank();
            crate::sync::sync_workspace(sym, &workspace_root, out).await?;
            crate::sync::sync_agent(sym, Some(&workspace_root), out).await?;
        }
    } else if user_config_exists {
        out.info("user already configured — run from a Rust workspace to set up a project");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Interactive prompts
// ---------------------------------------------------------------------------

fn prompt_for_agents(existing: &[AgentEntry]) -> Result<Vec<Agent>> {
    let agents = Agent::all();
    let items: Vec<&str> = agents.iter().map(|a| a.display_name()).collect();

    // Pre-select agents that are already configured
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

fn prompt_for_project_agents() -> Result<Vec<Agent>> {
    let add_agents = Confirm::new()
        .with_prompt("Add project-level agents? (default: each developer uses their own)")
        .default(false)
        .interact()?;

    if add_agents {
        prompt_for_agents(&[])
    } else {
        Ok(Vec::new())
    }
}

// ---------------------------------------------------------------------------
// Workspace detection
// ---------------------------------------------------------------------------

/// Find the workspace root using `cargo metadata`, run from the given directory.
pub fn find_workspace_root(cwd: &Path) -> Result<PathBuf> {
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
