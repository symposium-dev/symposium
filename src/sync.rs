//! Sync command: `symposium sync`.
//!
//! Scans workspace dependencies, finds applicable skills from plugin sources,
//! installs them into each configured agent's skill directory, and cleans up
//! stale skills by looking for a `.symposium` marker file in each skill dir.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};

use crate::agents::Agent;
use crate::config::Symposium;
use crate::output::{Output, display_path};
use crate::plugins;
use crate::skills;
use symposium_sdk::workspace::WorkspaceDeps;

/// Marker file written into every skill directory symposium installs.
///
/// Cleanup walks each agent's skills parent dir and removes any subdir
/// containing this marker that isn't in the freshly-installed set, leaving
/// user-managed skill directories (which lack the marker) untouched.
const MARKER_FILE: &str = ".symposium";

/// Create `path` and any missing ancestors up to `boundary`.
///
/// `boundary` is the workspace root — we never walk above it.
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

/// Collect all regular files in `dir` recursively, returning paths relative
/// to `dir` paired with their contents. Skips the `.symposium` marker and
/// `.gitignore` since those are managed metadata, not skill content.
fn collect_dir_contents(dir: &Path) -> Result<Vec<(PathBuf, Vec<u8>)>> {
    let mut result = Vec::new();
    collect_dir_contents_inner(dir, dir, &mut result)?;
    result.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(result)
}

fn collect_dir_contents_inner(
    base: &Path,
    dir: &Path,
    out: &mut Vec<(PathBuf, Vec<u8>)>,
) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e).with_context(|| format!("read {}", dir.display())),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_dir_contents_inner(base, &path, out)?;
        } else if file_type.is_file() {
            let rel = path.strip_prefix(base).unwrap_or(&path).to_path_buf();
            let name = rel.to_string_lossy();
            if name == MARKER_FILE || name == ".gitignore" {
                continue;
            }
            let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
            out.push((rel, bytes));
        }
    }
    Ok(())
}

/// Returns true if the source directory's content differs from the
/// destination's content (ignoring managed metadata files).
fn dir_contents_differ(source_dir: &Path, dest_dir: &Path) -> Result<bool> {
    let src = collect_dir_contents(source_dir)?;
    let dst = collect_dir_contents(dest_dir)?;
    Ok(src != dst)
}

/// Synchronize a skill directory from `source_dir` into `dest_dir`.
///
/// This is the single function used by both the plugin-skill and
/// user-authored-skill code paths. It:
/// 1. Checks whether `dest_dir` is debounce-fresh (marker mtime < `debounce`)
///    — if so, skips entirely.
/// 2. Compares source and dest content — if identical, touches the marker
///    to reset the debounce window and returns without modifying content.
/// 3. Otherwise removes `dest_dir`, re-creates it with the source content,
///    and writes the marker + gitignore.
///
/// Returns `Ok(true)` if the destination was created or updated (callers
/// record it as installed). Returns `Ok(false)` if skipped (no-op).
fn sync_skill_dir(
    source_dir: &Path,
    dest_dir: &Path,
    project_root: &Path,
    debounce: Duration,
) -> Result<bool> {
    if dest_dir == source_dir {
        return Ok(false);
    }

    // If the destination doesn't exist yet, do a fresh install.
    if !dest_dir.exists() {
        create_managed_dir_all(dest_dir, project_root)?;
        copy_dir_recursive(source_dir, dest_dir)?;
        mark_generated_skill_directory(dest_dir)?;
        return Ok(true);
    }

    // Debounce: if we synced recently, skip the content comparison.
    let marker_path = dest_dir.join(MARKER_FILE);
    if !debounce.is_zero()
        && let Ok(meta) = fs::metadata(&marker_path)
        && let Ok(mtime) = meta.modified()
        && let Ok(elapsed) = SystemTime::now().duration_since(mtime)
        && elapsed < debounce
    {
        tracing::debug!(dest = %dest_dir.display(), "skill sync debounced");
        return Ok(false);
    }

    // Compare content (excluding managed metadata).
    if !dir_contents_differ(source_dir, dest_dir)? {
        // Content is identical — just touch the marker to reset debounce.
        touch_marker(&marker_path)?;
        return Ok(false);
    }

    // Content changed: replace entirely.
    fs::remove_dir_all(dest_dir).with_context(|| format!("remove {}", dest_dir.display()))?;
    create_managed_dir_all(dest_dir, project_root)?;
    copy_dir_recursive(source_dir, dest_dir)?;
    mark_generated_skill_directory(dest_dir)?;
    Ok(true)
}

/// Update the marker file's mtime to now without changing content.
fn touch_marker(marker_path: &Path) -> Result<()> {
    fs::write(marker_path, "")
        .with_context(|| format!("touch marker {}", marker_path.display()))?;
    Ok(())
}

/// Resolve custom predicate installations from the registry into entries
/// suitable for [`PredicateContext::with_custom_predicates`].
async fn resolve_custom_predicate_entries(
    sym: &Symposium,
    registry: &plugins::PluginRegistry,
) -> std::collections::HashMap<String, crate::predicate::ResolvedPredicateEntry> {
    use crate::predicate::ResolvedPredicateEntry;

    let mut entries = std::collections::HashMap::new();

    for (name, resolved) in registry.custom_predicates.iter() {
        let plugin = &registry.plugins[resolved.plugin_index];
        let Some(install) = plugin.plugin.get_installation(&resolved.command) else {
            tracing::warn!(
                predicate = name,
                command = &resolved.command,
                "custom predicate references unknown installation"
            );
            continue;
        };

        let acquired =
            match crate::installation::acquire_installation(sym, install, None, None).await {
                Ok(a) => a,
                Err(e) => {
                    tracing::warn!(
                        predicate = name,
                        error = %e,
                        "failed to acquire custom predicate installation"
                    );
                    continue;
                }
            };

        let runnable =
            match crate::installation::resolve_runnable(acquired, &format!("predicate `{name}`")) {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(
                        predicate = name,
                        error = %e,
                        "failed to resolve custom predicate runnable"
                    );
                    continue;
                }
            };

        entries.insert(
            name.clone(),
            ResolvedPredicateEntry {
                runnable,
                args: resolved.args.clone(),
            },
        );
    }

    entries
}

/// Run the full sync: discover applicable skills, install into agent dirs,
/// clean up stale installations.
pub async fn sync(sym: &Symposium, cwd: &Path) -> Result<()> {
    let mut deps = sym.workspace_deps(cwd);
    sync_with_deps(sym, &mut deps).await
}

/// Sync variant that shares a `WorkspaceDeps` cache with the caller.
/// Used by `execute_hook` so that the auto-sync load is reused by later
/// hook stages.
pub async fn sync_with_deps(sym: &Symposium, deps: &mut WorkspaceDeps) -> Result<()> {
    let out = &Output::quiet();
    let loaded = deps
        .load()
        .ok_or_else(|| anyhow::anyhow!("not in a Rust workspace"))?;
    let project_root = loaded.root.clone();
    let workspace: Vec<_> = loaded.crates.clone();
    let debounce = Duration::from_secs(sym.config.sync_debounce_secs);
    tracing::debug!(root = %project_root.display(), "resolved workspace root");

    // Load plugin registry
    let registry = plugins::load_registry(sym);

    for warning in &registry.warnings {
        tracing::info!(
            report = %crate::report::ReportEvent::Warning {
                message: format!("skipping {}: {}", display_path(&warning.path), warning.message),
            },
        );
    }

    tracing::info!(
        report = %crate::report::ReportEvent::Info {
            message: format!("scanning {} workspace dependencies", workspace.len()),
        },
    );

    // Resolve custom predicate installations.
    let custom_entries = resolve_custom_predicate_entries(sym, &registry).await;

    // Find all applicable skills
    let applicable = skills::skills_applicable_to(sym, &registry, &workspace, custom_entries).await;

    // Dedup by `(skill_name, SkillOrigin)`: two `Crate` origins with the
    // same (name, version) collapse (the skills are the same logical bytes
    // from the same crate source); two `Plugin` origins always survive
    // independently. Skills that survive dedup are recorded with both
    // their plain name and their origin so we can decide later whether
    // each one needs an `<name>-<hash>` suffix to avoid collisions.
    let mut seen: BTreeSet<(String, skills::SkillOrigin)> = BTreeSet::new();
    let mut to_install: Vec<(String, skills::SkillOrigin, &std::path::Path)> = Vec::new();
    let mut name_counts: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();

    for entry in &applicable {
        let name = entry.skill.name().to_string();
        if seen.insert((name.clone(), entry.origin.clone())) {
            *name_counts.entry(name.clone()).or_default() += 1;
            to_install.push((name, entry.origin.clone(), &entry.skill.path));
        }
    }

    // Collect MCP servers from applicable plugins, filtered by workspace deps
    let semver_pairs = crate::crate_sources::crate_pairs(&workspace);
    let mut ctx = crate::predicate::PredicateContext::new(&semver_pairs);
    let mut mcp_servers: Vec<sacp::schema::McpServer> = Vec::new();
    for p in &registry.plugins {
        if p.plugin.applies(&mut ctx) {
            mcp_servers.extend(p.plugin.applicable_mcp_servers(&mut ctx));
        }
    }

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
        tracing::info!(
            report = %crate::report::ReportEvent::Info {
                message: "no agents configured, run `cargo agents init` to add one".into(),
            },
        );
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

        for (skill_name, origin, skill_source) in &to_install {
            // `skill_source` is the path to the SKILL.md file; the skill
            // directory is its parent.
            let source_dir = match skill_source.parent() {
                Some(p) => p,
                None => {
                    out.warn(format!(
                        "skill {skill_name}: cannot determine source directory"
                    ));
                    continue;
                }
            };

            // Pick the install dir name for this skill on *this* agent:
            // - If exactly one origin claims the name and the un-suffixed
            //   slot is "available" (nonexistent or symposium-managed),
            //   use the plain `<skill-name>/`.
            // - Otherwise fall back to `<skill-name>-<origin-hash>/` so
            //   distinct origins coexist and we never clobber a
            //   user-managed directory.
            let unique_name = name_counts.get(skill_name).copied().unwrap_or(0) == 1;
            let plain_dir = agent.project_skill_dir(&project_root, skill_name);
            let plain_available = !plain_dir.exists() || has_symposium_marker(&plain_dir);
            let dir_name = if unique_name && plain_available {
                skill_name.clone()
            } else {
                format!("{skill_name}-{}", origin.short_hash())
            };
            let dest_dir = agent.project_skill_dir(&project_root, &dir_name);

            // If the dest exists but is user-managed, skip it.
            if dest_dir.exists() && !has_symposium_marker(&dest_dir) {
                tracing::info!(
                    report = %crate::report::ReportEvent::Warning {
                        message: format!(
                            "skipping {}: user-managed skill already present",
                            display_path(&dest_dir)
                        ),
                    },
                );
                continue;
            }

            match sync_skill_dir(source_dir, &dest_dir, &project_root, debounce) {
                Ok(true) => {
                    installed_dirs.insert(dest_dir.clone());
                    tracing::info!(
                        report = %crate::report::ReportEvent::SkillInstalled {
                            skill: dir_name.clone(),
                            agent: agent_name.clone(),
                            dest: display_path(&dest_dir),
                        },
                    );
                }
                Ok(false) => {
                    // Debounced or unchanged — still record as installed
                    // so stale-cleanup doesn't remove it.
                    installed_dirs.insert(dest_dir.clone());
                }
                Err(e) => {
                    tracing::info!(
                        report = %crate::report::ReportEvent::Warning {
                            message: format!("failed to install skill {dir_name}: {e}"),
                        },
                    );
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

                    if dest_dir == *source_dir {
                        continue;
                    }
                    if dest_dir.exists() && !has_symposium_marker(&dest_dir) {
                        tracing::info!(
                            report = %crate::report::ReportEvent::Warning {
                                message: format!(
                                    "skipping propagation to {}: user-managed skill already present",
                                    display_path(&dest_dir)
                                ),
                            },
                        );
                        continue;
                    }

                    match sync_skill_dir(source_dir, &dest_dir, &project_root, debounce) {
                        Ok(true) => {
                            installed_dirs.insert(dest_dir.clone());
                            tracing::info!(
                                report = %crate::report::ReportEvent::SkillPropagated {
                                    skill: name.to_string(),
                                    agent: agent_name.clone(),
                                    dest: display_path(&dest_dir),
                                },
                            );
                        }
                        Ok(false) => {
                            // Debounced or unchanged — still record as
                            // installed so stale-cleanup doesn't remove it.
                            if dest_dir.exists() {
                                installed_dirs.insert(dest_dir.clone());
                            }
                        }
                        Err(e) => {
                            tracing::info!(
                                report = %crate::report::ReportEvent::Warning {
                                    message: format!("failed to propagate skill {name} to {}: {e}", display_path(&dest_dir)),
                                },
                            );
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
                    tracing::info!(
                        report = %crate::report::ReportEvent::SkillRemoved {
                            path: display_path(&path),
                        },
                    );
                }
                Err(e) => {
                    tracing::info!(
                        report = %crate::report::ReportEvent::Warning {
                            message: format!("failed to remove stale {}: {e}", display_path(&path)),
                        },
                    );
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
        tracing::info!(
            report = %crate::report::ReportEvent::Info {
                message: "no applicable skills found for workspace dependencies".into(),
            },
        );
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
