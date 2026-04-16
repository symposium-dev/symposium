//! Sync commands: `sync --workspace` and `sync --agent`.
//!
//! - `sync --workspace` updates `.symposium/config.toml` to reflect workspace deps.
//! - `sync --agent` installs enabled extensions into the agent's expected locations.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::agents::Agent;
use crate::config::{ProjectConfig, Symposium, resolve_agents, resolve_sync_default};
use crate::output::{Output, display_path};
use crate::plugins;
use crate::skills;

// ---------------------------------------------------------------------------
// sync --workspace
// ---------------------------------------------------------------------------

/// Update `.symposium/config.toml` to match current workspace dependencies.
///
/// Returns the updated project config.
pub async fn sync_workspace(
    sym: &Symposium,
    project_root: &Path,
    out: &Output,
) -> Result<ProjectConfig> {
    let lock_path = project_root.join("Cargo.lock");
    let mtime_cache_path = sym
        .cache_dir()
        .join("cargo-lock-mtime")
        .join(cache_key_for_path(project_root));

    // Load existing project config (used for mtime check, source resolution, and merge)
    let existing = ProjectConfig::load(project_root).unwrap_or_default();

    // Step 1: Check Cargo.lock mtime
    if existing.skills.len() > 0 && is_mtime_unchanged(&lock_path, &mtime_cache_path) {
        out.already_ok("Cargo.lock unchanged, skipping workspace sync");
        return Ok(existing);
    }

    // Step 2: Read workspace dependencies
    let workspace = crate::crate_sources::workspace_semver_pairs(project_root);
    let dep_names: std::collections::BTreeSet<String> =
        workspace.iter().map(|(name, _)| name.clone()).collect();

    out.info(format!(
        "scanning {} workspace dependencies",
        dep_names.len()
    ));

    // Step 3: Load plugin sources and discover skills (project-aware)
    let registry = plugins::load_registry_with(sym, Some(&existing), Some(project_root));
    let applicable = skills::skills_applicable_to(sym, &registry, &workspace).await;

    // Collect applicable crate names from skills
    let mut available_skills: BTreeMap<String, bool> = BTreeMap::new();
    let sync_default = resolve_sync_default(&sym.config, Some(&existing));

    for entry in &applicable {
        for crate_name in entry.effective_crate_names() {
            if dep_names.contains(&crate_name) {
                available_skills.entry(crate_name).or_insert(sync_default);
            }
        }
    }

    // Step 5: Merge with existing config
    // (reuses `existing` loaded at the top)

    // Build the merged skills map: preserve existing choices, add new, drop stale
    let mut merged_skills: BTreeMap<String, bool> = BTreeMap::new();
    let mut added_skills = Vec::new();
    let mut removed_skills = Vec::new();

    for (name, default) in &available_skills {
        if let Some(&existing_val) = existing.skills.get(name) {
            // Preserve user's existing on/off choice
            merged_skills.insert(name.clone(), existing_val);
        } else {
            // New skill
            merged_skills.insert(name.clone(), *default);
            added_skills.push(name.clone());
        }
    }
    for key in existing.skills.keys() {
        if !available_skills.contains_key(key) {
            removed_skills.push(key.clone());
        }
    }

    // Step 6: Write config (format-preserving)
    let config_path = ProjectConfig::path(project_root);
    if !config_path.exists() {
        // First time: create the file with serde so we get a clean starting point
        let config = ProjectConfig {
            agents: existing.agents.clone(),
            skills: merged_skills.clone(),
            workflows: existing.workflows.clone(),
            ..Default::default()
        };
        config.save(project_root)?;
    } else {
        ProjectConfig::update_skills(project_root, &merged_skills)?;
    }

    // Step 7: Cache mtime
    cache_mtime(&lock_path, &mtime_cache_path);

    // Report what happened
    for name in &added_skills {
        let status = if merged_skills[name] { "on" } else { "off" };
        out.added(format!("skill {name} ({status})"));
    }
    for name in &removed_skills {
        out.removed(format!("skill {name} (dependency removed)"));
    }
    if added_skills.is_empty() && removed_skills.is_empty() {
        out.already_ok(format!(
            "{}: {} skills unchanged",
            display_path(&config_path),
            merged_skills.len()
        ));
    } else {
        out.done(format!(
            "{}: {} skills ({} added, {} removed)",
            display_path(&config_path),
            merged_skills.len(),
            added_skills.len(),
            removed_skills.len()
        ));
    }

    // Return the merged view
    Ok(ProjectConfig {
        agents: existing.agents,
        skills: merged_skills,
        workflows: existing.workflows,
        sync_default: existing.sync_default,
        self_contained: existing.self_contained,
        defaults: existing.defaults,
        plugin_source: existing.plugin_source,
    })
}

// ---------------------------------------------------------------------------
// sync --agent
// ---------------------------------------------------------------------------

/// Install enabled extensions and register hooks for all configured agents.
/// Also removes hooks for agents that are no longer configured.
pub async fn sync_agent(sym: &Symposium, project_root: Option<&Path>, out: &Output) -> Result<()> {
    let project_config = project_root.and_then(ProjectConfig::load);
    let agent_names = resolve_agents(&sym.config, project_config.as_ref());

    // Collect MCP servers from all plugins
    let registry = plugins::load_registry_with(sym, project_config.as_ref(), project_root);
    let mcp_servers: Vec<sacp::schema::McpServer> = registry
        .plugins
        .iter()
        .flat_map(|p| p.plugin.mcp_servers.iter().cloned())
        .collect();

    // Register hooks and MCP servers for configured agents
    for agent_name in &agent_names {
        let agent = Agent::from_config_name(agent_name)?;

        if let Some(root) = project_root {
            let project_config = project_config.as_ref();
            let is_project_agent =
                project_config.is_some_and(|c| c.agents.iter().any(|a| a.name == *agent_name));

            if is_project_agent {
                agent
                    .register_project_hooks(root, sym, out)
                    .context("failed to register project hooks")?;
                agent
                    .register_project_mcp_servers(root, &mcp_servers, out)
                    .context("failed to register project MCP servers")?;
            } else {
                agent
                    .register_global_hooks(sym.home_dir(), sym, out)
                    .context("failed to register global hooks")?;
                agent
                    .register_global_mcp_servers(sym.home_dir(), &mcp_servers, out)
                    .context("failed to register global MCP servers")?;
            }

            if let Some(ref config) = project_config {
                install_skills(sym, agent, root, config, out).await?;
            }
        } else {
            agent
                .register_global_hooks(sym.home_dir(), sym, out)
                .context("failed to register global hooks")?;
            agent
                .register_global_mcp_servers(sym.home_dir(), &mcp_servers, out)
                .context("failed to register global MCP servers")?;
        }
    }

    // Collect server names for unregistration
    let server_names: Vec<&str> = mcp_servers
        .iter()
        .map(|s| match s {
            sacp::schema::McpServer::Stdio(s) => s.name.as_str(),
            sacp::schema::McpServer::Http(s) => s.name.as_str(),
            sacp::schema::McpServer::Sse(s) => s.name.as_str(),
            _ => "unknown",
        })
        .collect();

    // Remove hooks for agents that are no longer configured
    for &agent in Agent::all() {
        if !agent_names.contains(&agent.config_name().to_string()) {
            if let Some(root) = project_root {
                agent.unregister_project_hooks(root, sym, out);
                let _ = agent.unregister_project_mcp_servers(root, &server_names, out);
            }
            agent.unregister_global_hooks(sym.home_dir(), sym, out);
            let _ = agent.unregister_global_mcp_servers(sym.home_dir(), &server_names, out);
        }
    }

    if agent_names.is_empty() {
        out.info("no agents configured — run `symposium init --user` to add one");
    }

    Ok(())
}

/// Add an agent to the project config.
pub fn add_agent(project_root: &Path, agent_name: &str, out: &Output) -> Result<()> {
    let agent = Agent::from_config_name(agent_name)?;
    ProjectConfig::add_agent(project_root, agent_name)?;
    out.done(format!(
        "added agent {} ({})",
        agent_name,
        agent.display_name()
    ));
    Ok(())
}

/// Remove an agent from the project config.
pub fn remove_agent(project_root: &Path, agent_name: &str, out: &Output) -> Result<()> {
    let agent = Agent::from_config_name(agent_name)?;
    ProjectConfig::remove_agent(project_root, agent_name)?;
    out.removed(format!(
        "removed agent {} ({})",
        agent_name,
        agent.display_name()
    ));
    Ok(())
}

// ---------------------------------------------------------------------------
// Skill installation
// ---------------------------------------------------------------------------

async fn install_skills(
    sym: &Symposium,
    agent: Agent,
    project_root: &Path,
    config: &ProjectConfig,
    out: &Output,
) -> Result<()> {
    let workspace = crate::crate_sources::workspace_semver_pairs(project_root);
    let registry = plugins::load_registry(sym);
    let applicable = skills::skills_applicable_to(sym, &registry, &workspace).await;

    let mut installed = Vec::new();

    for entry in &applicable {
        for crate_name in entry.effective_crate_names() {
            let enabled = config.skills.get(&crate_name).copied().unwrap_or(false);
            if !enabled {
                continue;
            }

            let skill_path = &entry.skill.path;
            let skill_name = entry.skill.name();
            let dest_dir = agent.project_skill_dir(project_root, skill_name);

            match agent.install_skill(skill_path, &dest_dir) {
                Ok(()) => {
                    installed.push(skill_name.to_string());
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
    }

    if installed.is_empty() {
        out.info("no enabled skills to install");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Cargo.lock mtime caching
// ---------------------------------------------------------------------------

fn cache_key_for_path(path: &Path) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn is_mtime_unchanged(lock_path: &Path, cache_path: &Path) -> bool {
    let Ok(lock_meta) = fs::metadata(lock_path) else {
        return false;
    };
    let Ok(cached) = fs::read_to_string(cache_path) else {
        return false;
    };
    let Ok(lock_mtime) = lock_meta.modified() else {
        return false;
    };
    let mtime_str = format!("{:?}", lock_mtime);
    cached.trim() == mtime_str
}

fn cache_mtime(lock_path: &Path, cache_path: &Path) {
    let Ok(lock_meta) = fs::metadata(lock_path) else {
        return;
    };
    let Ok(lock_mtime) = lock_meta.modified() else {
        return;
    };
    if let Some(parent) = cache_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(cache_path, format!("{:?}", lock_mtime));
}
