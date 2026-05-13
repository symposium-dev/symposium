//! Sync command: `symposium sync`.
//!
//! Scans workspace dependencies, finds applicable skills from plugin sources,
//! installs them into each configured agent's skill directory, and cleans up
//! stale skills by looking for a `.symposium` marker file in each skill dir.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::agents::Agent;
use crate::config::Symposium;
use crate::output::{Output, display_path};
use crate::plugins;
use crate::skills;

/// Marker file written into every skill directory symposium installs.
///
/// Cleanup walks each agent's skills parent dir and removes any subdir
/// containing this marker that isn't in the freshly-installed set, leaving
/// user-managed skill directories (which lack the marker) untouched.
const MARKER_FILE: &str = ".symposium";

/// Create `path` (and any missing ancestors up to `boundary`), writing a
/// `.gitignore` file containing `*` into each directory we newly create.
///
/// `boundary` is the workspace root — we never walk above it, and we do not
/// write a `.gitignore` there (it already exists and isn't ours to manage).
/// If `path` or an ancestor already exists, no `.gitignore` is added — we
/// only annotate directories we actually create.
pub(crate) fn create_managed_dir_all(path: &Path, boundary: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    if path == boundary {
        fs::create_dir_all(path).with_context(|| format!("create {}", path.display()))?;
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        create_managed_dir_all(parent, boundary)?;
    }
    fs::create_dir(path).with_context(|| format!("create {}", path.display()))?;
    let gi = path.join(".gitignore");
    if !gi.exists() {
        fs::write(&gi, "*\n").with_context(|| format!("write {}", gi.display()))?;
    }
    Ok(())
}

/// Skills parent directory for an agent (e.g. `.claude/skills/` or
/// `.agents/skills/`), derived from `Agent::project_skill_dir`.
fn skills_parent_dir(agent: Agent, project_root: &Path) -> PathBuf {
    agent
        .project_skill_dir(project_root, "_")
        .parent()
        .expect("skill dir must have parent")
        .to_path_buf()
}

/// Mark a directory as symposium-generated: drop the `.symposium` marker
/// and a `.gitignore` containing `*` so the directory is recognized on
/// future syncs and kept out of version control.
///
/// Idempotent — overwrites any pre-existing marker or `.gitignore` in
/// `dir`. Callers use this both for freshly-installed plugin skills and
/// for skills propagated by the agents-syncing feature.
fn mark_generated_skill_directory(dir: &Path) -> Result<()> {
    fs::write(dir.join(MARKER_FILE), "")
        .with_context(|| format!("write marker in {}", dir.display()))?;
    fs::write(dir.join(".gitignore"), "*\n")
        .with_context(|| format!("write .gitignore in {}", dir.display()))?;
    Ok(())
}

/// Does `dir` contain the `.symposium` marker, i.e. is it a symposium-managed
/// skill directory? Returns `false` for user-authored skills and for any
/// directory symposium did not create.
fn has_symposium_marker(dir: &Path) -> bool {
    dir.join(MARKER_FILE).exists()
}

/// Discover user-authored skills in `<project_root>/.agents/skills/`.
///
/// A skill is user-authored iff its directory contains `SKILL.md` and does
/// *not* contain the `.symposium` marker. Symposium never writes markers
/// into source skills, so this unambiguously separates user content from
/// copies symposium put there itself.
fn discover_user_authored_skills(project_root: &Path) -> Vec<PathBuf> {
    let agents_skills_dir = project_root.join(".agents").join("skills");
    let Ok(entries) = fs::read_dir(&agents_skills_dir) else {
        return Vec::new();
    };

    let mut skills: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter(|p| p.join("SKILL.md").is_file())
        .filter(|p| !has_symposium_marker(p))
        .collect();
    skills.sort();
    skills
}

/// Recursively copy the contents of `src` into `dst`. Creates `dst` if
/// missing. Regular files are copied with `fs::copy`; subdirectories are
/// walked. Symlinks and other special files are ignored.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst).with_context(|| format!("create {}", dst.display()))?;
    for entry in fs::read_dir(src).with_context(|| format!("read {}", src.display()))? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if file_type.is_file() {
            fs::copy(&src_path, &dst_path)
                .with_context(|| format!("copy {} → {}", src_path.display(), dst_path.display()))?;
        }
    }
    Ok(())
}

/// Propagate a user-authored skill from `.agents/skills/<name>/` to
/// `dest_dir`, returning `Ok(true)` if propagation happened (so the caller
/// should record `dest_dir` as installed).
///
/// Leaves `dest_dir` alone if it exists and lacks the `.symposium` marker —
/// the user put something there by hand and we must not clobber it.
fn propagate_user_skill(
    source_dir: &Path,
    dest_dir: &Path,
    project_root: &Path,
    out: &Output,
) -> Result<bool> {
    if dest_dir == source_dir {
        // Agent reads from the same directory as the source — nothing to do.
        return Ok(false);
    }

    let target_is_managed = has_symposium_marker(dest_dir);
    if dest_dir.exists() && !target_is_managed {
        out.warn(format!(
            "skipping propagation to {}: user-managed skill already present",
            display_path(dest_dir)
        ));
        return Ok(false);
    }

    // Clear any prior symposium-managed copy so removed files don't linger.
    if dest_dir.exists() {
        fs::remove_dir_all(dest_dir).with_context(|| format!("remove {}", dest_dir.display()))?;
    }

    create_managed_dir_all(dest_dir, project_root)?;
    copy_dir_recursive(source_dir, dest_dir)?;

    // The copy may have clobbered any marker or `.gitignore` written by
    // `create_managed_dir_all`, so re-apply them. This also guarantees a
    // uniform "symposium-managed" shape regardless of what the source
    // skill directory happened to contain.
    mark_generated_skill_directory(dest_dir)?;
    Ok(true)
}

/// Run the full sync: discover applicable skills, install into agent dirs,
/// clean up stale installations.
pub async fn sync(sym: &Symposium, cwd: &Path, out: &Output) -> Result<()> {
    let project_root = crate::init::find_workspace_root(cwd)?;
    tracing::debug!(root = %project_root.display(), "resolved workspace root");

    // Load plugin registry and workspace deps
    let registry = plugins::load_registry(sym);
    let workspace = crate::crate_sources::workspace_crates(&project_root);

    for warning in &registry.warnings {
        out.warn(format!(
            "skipping {}: {}",
            display_path(&warning.path),
            warning.message
        ));
    }

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
    let semver_pairs: Vec<(String, semver::Version)> = workspace
        .iter()
        .map(|wc| (wc.name.clone(), wc.version.clone()))
        .collect();
    let mcp_servers: Vec<sacp::schema::McpServer> = registry
        .plugins
        .iter()
        .filter(|p| p.plugin.applies_to_crates(&semver_pairs))
        .flat_map(|p| p.plugin.applicable_mcp_servers(&semver_pairs))
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

    // Track every skill directory we (re)install during this sync. Anything
    // we find later that has the marker file but isn't in this set is stale.
    let mut installed_dirs: BTreeSet<PathBuf> = BTreeSet::new();

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

        for &(skill_name, skill_source) in &to_install {
            let dest_dir = agent.project_skill_dir(&project_root, skill_name);

            // Create the destination (and any missing parents) with a `*` gitignore
            // in each new directory.
            if let Err(e) = create_managed_dir_all(&dest_dir, &project_root) {
                out.warn(format!("failed to create {}: {e}", display_path(&dest_dir)));
                continue;
            }

            match agent.install_skill(skill_source, &dest_dir) {
                Ok(()) => {
                    // Mark the directory as symposium-managed (marker +
                    // wildcard .gitignore). Kept as a warning on failure so
                    // a broken install doesn't halt the whole sync.
                    if let Err(e) = mark_generated_skill_directory(&dest_dir) {
                        out.warn(format!("failed to mark {}: {e}", display_path(&dest_dir)));
                    }
                    installed_dirs.insert(dest_dir.clone());
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
    }

    // Propagate user-authored skills from `.agents/skills/` into every
    // configured agent that reads skills from a different directory. Skills
    // are "user-authored" when they lack the `.symposium` marker — symposium
    // never writes that marker into a source, so this never re-propagates
    // symposium's own installs. See the agents-syncing feature docs.
    if sym.config.agents_syncing {
        let user_authored = discover_user_authored_skills(&project_root);
        if !user_authored.is_empty() {
            tracing::debug!(
                count = user_authored.len(),
                "propagating user-authored skills from .agents/skills/"
            );
            for agent_name in &agent_names {
                let agent = Agent::from_config_name(agent_name)?;
                for source_dir in &user_authored {
                    let name = match source_dir.file_name().and_then(|n| n.to_str()) {
                        Some(n) => n,
                        None => continue,
                    };
                    let dest_dir = agent.project_skill_dir(&project_root, name);
                    match propagate_user_skill(source_dir, &dest_dir, &project_root, out) {
                        Ok(true) => {
                            installed_dirs.insert(dest_dir.clone());
                            tracing::info!(
                                skill = %name,
                                agent = %agent_name,
                                dest = %dest_dir.display(),
                                "propagated skill from .agents/skills/"
                            );
                            out.done(format!(
                                "propagated skill {name} → {}",
                                display_path(&dest_dir)
                            ));
                        }
                        Ok(false) => {}
                        Err(e) => {
                            out.warn(format!(
                                "failed to propagate skill {name} to {}: {e}",
                                display_path(&dest_dir)
                            ));
                        }
                    }
                }
            }
        }
    }

    // Stale-skill cleanup: scan every agent's skills parent directory (across
    // all known agents, so we also clean up after agents removed from config)
    // and remove subdirs containing the marker that we didn't just install.
    let mut scanned: BTreeSet<PathBuf> = BTreeSet::new();
    for &agent in Agent::all() {
        let parent = skills_parent_dir(agent, &project_root);
        if !scanned.insert(parent.clone()) {
            continue;
        }
        let Ok(entries) = fs::read_dir(&parent) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() || installed_dirs.contains(&path) {
                continue;
            }
            if !has_symposium_marker(&path) {
                continue;
            }
            match fs::remove_dir_all(&path) {
                Ok(()) => {
                    tracing::info!(path = %path.display(), "removed stale skill");
                    out.removed(format!("removed {}", display_path(&path)));
                }
                Err(e) => {
                    out.warn(format!(
                        "failed to remove stale {}: {e}",
                        display_path(&path)
                    ));
                }
            }
        }
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
