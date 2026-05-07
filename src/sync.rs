//! Sync command: `symposium sync`.
//!
//! Scans workspace dependencies, finds applicable skills from plugin sources,
//! installs them into each configured agent's skill directory, and cleans up
//! stale skills using a per-agent manifest.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::agents::Agent;
use crate::config::Symposium;
use crate::output::{Output, display_path};
use crate::plugins;
use crate::skills;

/// Manifest tracking which skills symposium installed for a given agent.
/// Stored at e.g. `.agents/skills/.symposium.toml` or `.claude/skills/.symposium.toml`.
#[derive(Debug, Default, Deserialize, Serialize)]
struct SkillManifest {
    /// Skill names installed by symposium.
    #[serde(default)]
    installed: BTreeSet<String>,
}

impl SkillManifest {
    fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let contents = toml::to_string_pretty(self)?;
        fs::write(path, contents)?;
        Ok(())
    }
}

/// Returns the manifest path for a given agent's skill directory.
fn manifest_path(agent: Agent, project_root: &Path) -> std::path::PathBuf {
    // Use the agent's skill dir parent (e.g. `.agents/skills/`, `.claude/skills/`)
    // and put the manifest there.
    let dummy_dir = agent.project_skill_dir(project_root, ".symposium-probe");
    // dummy_dir is e.g. `.agents/skills/.symposium-probe`
    // parent is `.agents/skills/`
    dummy_dir
        .parent()
        .expect("skill dir must have parent")
        .join(".symposium.toml")
}

/// Run the full sync: discover applicable skills, install into agent dirs,
/// clean up stale skills.
pub async fn sync(sym: &Symposium, cwd: &Path, out: &Output) -> Result<()> {
    let project_root = crate::init::find_workspace_root(cwd)?;
    tracing::debug!(root = %project_root.display(), "resolved workspace root");

    // Load plugin registry and workspace deps
    let registry = plugins::load_registry(sym);
    let workspace = crate::crate_sources::workspace_semver_pairs(&project_root);

    out.info(format!(
        "scanning {} workspace dependencies",
        workspace.len()
    ));

    // Find all applicable skills
    let applicable = skills::skills_applicable_to(sym, &registry, &workspace).await;

    let mut skill_names: BTreeSet<String> = BTreeSet::new();

    // Build a map of skill_name -> skill for installation
    let mut to_install: Vec<(&str, &std::path::Path)> = Vec::new();

    for entry in &applicable {
        let name = entry.skill.name();
        if skill_names.insert(name.to_string()) {
            to_install.push((name, &entry.skill.path));
        }
    }

    // Collect MCP servers from applicable plugins, filtered by workspace deps
    let mcp_servers: Vec<sacp::schema::McpServer> = registry
        .plugins
        .iter()
        .filter(|p| p.plugin.applies_to_crates(&workspace))
        .flat_map(|p| p.plugin.applicable_mcp_servers(&workspace))
        .collect();

    let server_names: Vec<&str> = mcp_servers
        .iter()
        .map(|s| match s {
            sacp::schema::McpServer::Stdio(s) => s.name.as_str(),
            sacp::schema::McpServer::Http(s) => s.name.as_str(),
            sacp::schema::McpServer::Sse(s) => s.name.as_str(),
            _ => panic!("unsupported McpServer variant"),
        })
        .collect();

    // Sync each configured agent
    let agent_names: Vec<String> = sym.config.agents.iter().map(|a| a.name.clone()).collect();

    tracing::info!(
        workspace_deps = workspace.len(),
        agents = agent_names.len(),
        skills = to_install.len(),
        "sync started"
    );

    if agent_names.is_empty() {
        out.info("no agents configured, run `cargo agents init` to add one");
        return Ok(());
    }

    for agent_name in &agent_names {
        let agent = Agent::from_config_name(agent_name)?;

        let hook_root = match sym.config.hook_scope {
            crate::config::HookScope::Global => sym.home_dir().to_path_buf(),
            crate::config::HookScope::Project => project_root.clone(),
        };

        // Register hooks and MCP servers
        agent
            .register_hooks(&hook_root, sym, out)
            .context("failed to register hooks")?;
        agent
            .register_global_mcp_servers(&hook_root, &mcp_servers, out)
            .context("failed to register MCP servers")?;

        // Install skills and manage manifest
        let manifest_file = manifest_path(agent, &project_root);
        let old_manifest = SkillManifest::load(&manifest_file);

        let mut new_manifest = SkillManifest::default();

        for &(skill_name, skill_source) in &to_install {
            let dest_dir = agent.project_skill_dir(&project_root, skill_name);
            match agent.install_skill(skill_source, &dest_dir) {
                Ok(()) => {
                    new_manifest.installed.insert(skill_name.to_string());
                    tracing::info!(%skill_name, agent = %agent_name, dest = %dest_dir.display(), "installed skill");
                    out.done(format!(
                        "installed skill {skill_name} → {}",
                        display_path(&dest_dir)
                    ));
                }
                Err(e) => {
                    out.warn(format!("failed to install skill {skill_name}: {e}"));
                }
            }
        }

        // Remove stale skills (in old manifest but not in new)
        for stale in old_manifest.installed.difference(&new_manifest.installed) {
            let dest_dir = agent.project_skill_dir(&project_root, stale);
            if dest_dir.exists() {
                let _ = fs::remove_dir_all(&dest_dir);
                tracing::info!(%stale, agent = %agent_name, "removed stale skill");
                out.removed(format!(
                    "removed skill {stale} from {}",
                    display_path(&dest_dir)
                ));
            }
        }

        // Write updated manifest
        new_manifest
            .save(&manifest_file)
            .context("failed to write skill manifest")?;
    }

    // Unregister hooks/MCP for agents no longer configured
    for &agent in Agent::all() {
        if !agent_names.contains(&agent.config_name().to_string()) {
            agent.unregister_hooks(sym.home_dir(), sym, out);
            let _ = agent.unregister_global_mcp_servers(sym.home_dir(), &server_names, out);
        }
    }

    if to_install.is_empty() {
        tracing::debug!("no applicable skills for workspace dependencies");
        out.info("no applicable skills found for workspace dependencies");
    }

    Ok(())
}

/// Register global hooks for all configured agents.
/// Register hooks for all configured agents. Uses `home_dir` (global scope).
/// Called from `init` after writing the user config.
pub fn register_hooks(sym: &Symposium, out: &Output) -> Result<()> {
    let registry = plugins::load_registry(sym);
    let mcp_servers: Vec<sacp::schema::McpServer> = registry
        .plugins
        .iter()
        .flat_map(|p| p.plugin.mcp_servers.iter().map(|s| s.server.clone()))
        .collect();

    let server_names: Vec<&str> = mcp_servers
        .iter()
        .map(|s| match s {
            sacp::schema::McpServer::Stdio(s) => s.name.as_str(),
            sacp::schema::McpServer::Http(s) => s.name.as_str(),
            sacp::schema::McpServer::Sse(s) => s.name.as_str(),
            _ => panic!("unsupported McpServer variant"),
        })
        .collect();

    let agent_names: Vec<String> = sym.config.agents.iter().map(|a| a.name.clone()).collect();

    for agent_name in &agent_names {
        let agent = Agent::from_config_name(agent_name)?;
        agent.register_hooks(sym.home_dir(), sym, out)?;
        agent.register_global_mcp_servers(sym.home_dir(), &mcp_servers, out)?;
    }

    // Unregister hooks for agents no longer configured
    for &agent in Agent::all() {
        if !agent_names.contains(&agent.config_name().to_string()) {
            agent.unregister_hooks(sym.home_dir(), sym, out);
            let _ = agent.unregister_global_mcp_servers(sym.home_dir(), &server_names, out);
        }
    }

    Ok(())
}
