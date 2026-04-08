//! Init commands: `init --user` and `init --project`.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use dialoguer::{Confirm, Select};

use crate::agents::Agent;
use crate::config::{AgentConfig, ProjectConfig, Symposium};
use crate::output::{Output, display_path};

/// Options that can be provided on the command line to skip interactive prompts.
#[derive(Debug, Default)]
pub struct InitOpts {
    /// Agent name provided via `--agent`. If set, skips the agent prompt.
    pub agent: Option<String>,
}

/// Whether we can prompt the user interactively.
fn interactive() -> bool {
    std::io::stdin().is_terminal()
}

/// Resolve which agent to use. Priority:
/// 1. Explicit `--agent` flag
/// 2. Interactive prompt (if terminal)
/// 3. Default to first agent (Claude) in non-interactive mode
fn resolve_agent(opts: &InitOpts) -> Result<Agent> {
    if let Some(ref name) = opts.agent {
        return Agent::from_config_name(name);
    }
    if interactive() {
        return prompt_for_agent();
    }
    // Non-interactive default
    Ok(Agent::all()[0])
}

/// Resolve whether to set a project-level agent override. Priority:
/// 1. Explicit `--agent` flag → set override to that agent
/// 2. Interactive prompt (if terminal)
/// 3. No override in non-interactive mode
fn resolve_project_agent(opts: &InitOpts) -> Result<Option<Agent>> {
    if let Some(ref name) = opts.agent {
        return Ok(Some(Agent::from_config_name(name)?));
    }
    if interactive() {
        return prompt_for_project_agent();
    }
    Ok(None)
}

/// Run user-wide initialization.
///
/// Prompts for agent preference (unless provided), writes
/// `~/.symposium/config.toml`, and registers global hooks.
pub async fn init_user(sym: &mut Symposium, out: &Output, opts: &InitOpts) -> Result<()> {
    out.println("Setting up symposium for your user account.\n");

    let agent = resolve_agent(opts)?;

    // Write agent config
    sym.config.agent = AgentConfig {
        name: Some(agent.config_name().to_string()),
        sync_default: true,
        auto_sync: false,
    };
    sym.save_config()
        .context("failed to write user config")?;

    let config_path = sym.config_dir().join("config.toml");
    out.done(format!(
        "{}: wrote user config (agent: {})",
        display_path(&config_path),
        agent.display_name()
    ));

    // Register global hooks
    crate::sync::sync_agent(sym, None, out).await
        .context("failed to register global hooks")?;

    Ok(())
}

/// Run project-level initialization.
///
/// Finds workspace root from `cwd`, optionally prompts for agent override,
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
        let agent_override = resolve_project_agent(opts)?;

        // Create project config
        let config = ProjectConfig {
            agent: agent_override.map(|a| AgentConfig {
                name: Some(a.config_name().to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        config.save(&workspace_root)
            .context("failed to write project config")?;

        out.done("created .symposium/config.toml");
    }

    // Run sync --workspace to discover extensions
    crate::sync::sync_workspace(sym, &workspace_root, out).await
        .context("sync --workspace failed")?;

    // Run sync --agent to install extensions
    crate::sync::sync_agent(sym, Some(&workspace_root), out).await
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
    let user_config_exists = sym.config_dir().join("config.toml").exists()
        && sym.config.agent.name.is_some();

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

fn prompt_for_agent() -> Result<Agent> {
    let agents = Agent::all();
    let items: Vec<&str> = agents.iter().map(|a| a.display_name()).collect();

    let selection = Select::new()
        .with_prompt("Which agent do you use?")
        .items(&items)
        .default(0)
        .interact()?;

    Ok(agents[selection])
}

fn prompt_for_project_agent() -> Result<Option<Agent>> {
    let override_agent = Confirm::new()
        .with_prompt("Set a project-level agent override? (default: each developer uses their own)")
        .default(false)
        .interact()?;

    if override_agent {
        let agent = prompt_for_agent()?;
        Ok(Some(agent))
    } else {
        Ok(None)
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

    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)
        .context("failed to parse cargo metadata output")?;

    let workspace_root = metadata
        .get("workspace_root")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("cargo metadata missing workspace_root"))?;

    Ok(PathBuf::from(workspace_root))
}
