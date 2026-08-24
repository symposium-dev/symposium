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
use symposium_install::UpdateLevel;

use crate::agent_plugin::Scope;
use crate::agents::Agent;
use crate::config::Symposium;
use crate::output::{Output, display_path};
use crate::plugins;
use crate::pm::WorkspaceDeps;
use crate::skills;
use std::sync::Arc;

/// Marker file written into every skill directory symposium installs.
///
/// Cleanup walks each agent's skills parent dir and removes any subdir
/// containing this marker that isn't in the freshly-installed set, leaving
/// user-managed skill directories (which lack the marker) untouched.
pub(crate) const MARKER_FILE: &str = ".symposium";

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

/// Where individually-installed skills go for one sync: under the project when
/// there is one, and otherwise under the user's home, which is the only place a
/// globally-enabled plugin's skills can land for an agent that has no plugin
/// unit to receive instead.
#[derive(Clone, Copy)]
enum SkillHome<'a> {
    Project(&'a Path),
    Global(&'a Path),
}

impl<'a> SkillHome<'a> {
    /// The directory this agent should hold `skill_name` in. `None` when the
    /// agent has no such location at all — Copilot has no global skills path.
    fn dir_for(&self, agent: Agent, skill_name: &str) -> Option<PathBuf> {
        match self {
            SkillHome::Project(root) => Some(agent.project_skill_dir(root, skill_name)),
            SkillHome::Global(home) => agent.global_skill_dir(home, skill_name),
        }
    }

    /// The boundary `create_managed_dir_all` may not walk above.
    fn boundary(&self) -> &'a Path {
        match self {
            SkillHome::Project(root) => root,
            SkillHome::Global(home) => home,
        }
    }

    /// The shared parent those directories sit in, for stale cleanup.
    fn parent_for(&self, agent: Agent) -> Option<PathBuf> {
        Some(
            self.dir_for(agent, "_")?
                .parent()
                .expect("skill dir must have parent")
                .to_path_buf(),
        )
    }
}

/// Whether a managed directory also needs its own `.gitignore`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Marking {
    /// Marker plus a `.gitignore` containing `*`. For a directory installed into
    /// agent-owned territory such as `.claude/skills/`, where the parent holds
    /// user content and so cannot be ignored wholesale.
    MarkerAndGitignore,
    /// Marker only. For a directory under `.symposium/`, which symposium owns
    /// entirely and covers with one `.gitignore` at its root.
    MarkerOnly,
}

/// Mark a directory as symposium-managed: drop the `.symposium` marker so the
/// directory is recognized on future syncs, and, per `marking`, a `.gitignore`
/// containing `*` to keep it out of version control.
///
/// Idempotent: overwrites any pre-existing marker or `.gitignore` in `dir`.
fn mark_managed_dir(dir: &Path, marking: Marking) -> Result<()> {
    fs::write(dir.join(MARKER_FILE), "")
        .with_context(|| format!("write marker in {}", dir.display()))?;
    if marking == Marking::MarkerAndGitignore {
        fs::write(dir.join(".gitignore"), "*\n")
            .with_context(|| format!("write .gitignore in {}", dir.display()))?;
    }
    Ok(())
}

/// Write the single `.gitignore` covering the project directory symposium owns.
fn ignore_owned_dir(owned: &Path, project_root: &Path) -> Result<()> {
    create_managed_dir_all(owned, project_root)?;
    fs::write(owned.join(".gitignore"), "*\n")
        .with_context(|| format!("write .gitignore in {}", owned.display()))
}

/// Does `dir` contain the `.symposium` marker, i.e. is it a symposium-managed
/// skill directory? Returns `false` for user-authored skills and for any
/// directory symposium did not create.
pub(crate) fn has_symposium_marker(dir: &Path) -> bool {
    dir.join(MARKER_FILE).exists()
}

/// Recursively copy the contents of `src` into `dst`. Creates `dst` if
/// missing. Regular files are copied with `fs::copy`; subdirectories are
/// walked. Symlinks and other special files are ignored.
pub(crate) fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
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

/// Synchronize a symposium-managed directory from `source_dir` into `dest_dir`.
///
/// Used by every install path that copies a directory symposium owns. It:
/// 1. Checks whether `dest_dir` is debounce-fresh (marker mtime < `debounce`)
///    — if so, skips entirely.
/// 2. Compares source and dest content — if identical, touches the marker
///    to reset the debounce window and returns without modifying content.
/// 3. Otherwise removes `dest_dir`, re-creates it with the source content,
///    and writes the marker + gitignore.
///
/// Returns `Ok(true)` if the destination was created or updated (callers
/// record it as installed). Returns `Ok(false)` if skipped (no-op).
pub(crate) fn sync_managed_dir(
    source_dir: &Path,
    dest_dir: &Path,
    boundary: &Path,
    debounce: Duration,
    marking: Marking,
) -> Result<bool> {
    if dest_dir == source_dir {
        return Ok(false);
    }

    // If the destination doesn't exist yet, do a fresh install.
    if !dest_dir.exists() {
        create_managed_dir_all(dest_dir, boundary)?;
        copy_dir_recursive(source_dir, dest_dir)?;
        mark_managed_dir(dest_dir, marking)?;
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
    create_managed_dir_all(dest_dir, boundary)?;
    copy_dir_recursive(source_dir, dest_dir)?;
    mark_managed_dir(dest_dir, marking)?;
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
    update: UpdateLevel,
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
            match crate::installation::acquire_installation(sym, install, None, None, update).await
            {
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

/// Whether a sync may skip a directory it synced very recently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Debounce {
    /// Honor `sync-debounce-secs`. For the per-event hook path, which runs on
    /// every tool call and has to stay near free.
    Recent,
    /// Compare content regardless of how recently we last looked. For anything a
    /// person triggered, and for the `SessionStart` catch-up pass — otherwise
    /// editing a skill and re-running `sync` appears to do nothing.
    Always,
}

/// The project-scoped paths sync writes into, when there is a workspace at all.
struct ProjectPaths {
    root: PathBuf,
    /// The directory symposium owns outright, carrying the one `.gitignore`.
    owned: PathBuf,
    /// Staging root for project-scoped compiled plugins.
    staging: PathBuf,
}

impl ProjectPaths {
    fn under(root: &Path) -> Self {
        let owned = root.join(crate::agent_plugin::PROJECT_OWNED_DIR);
        Self {
            root: root.to_path_buf(),
            staging: owned.join(crate::agent_plugin::PROJECT_STAGING_SUBDIR),
            owned,
        }
    }
}

/// The staging roots to consider, paired with the scope each one holds. The
/// project root drops out when there is no workspace.
fn staging_roots<'a>(
    project: &'a Option<ProjectPaths>,
    global: &'a Path,
) -> Vec<(Scope, &'a Path)> {
    let mut roots = Vec::new();
    if let Some(p) = project {
        roots.push((Scope::Project, p.staging.as_path()));
    }
    roots.push((Scope::Global, global));
    roots
}

/// One skill selected for installation, with the plugin it came from.
struct PendingSkill<'a> {
    name: String,
    origin_hash: String,
    plugin: String,
    plugin_id: crate::pm::PackageId,
    source: &'a Path,
}

/// Run the full sync: discover applicable skills, install into agent dirs,
/// clean up stale installations.
pub async fn sync(
    sym: &Symposium,
    deps: &Arc<WorkspaceDeps>,
    update: UpdateLevel,
    debounce: Debounce,
) -> Result<()> {
    let out = &Output::quiet();
    // A workspace is optional. Without one there is nothing project-scoped to
    // install, but globally-enabled plugins still apply, so the global half of
    // the sync runs regardless.
    let loaded = deps.load().cloned();
    let project = loaded.as_ref().map(|l| ProjectPaths::under(&l.root));
    let workspace_deps_count = loaded.as_ref().map_or(0, |l| l.crates.len());
    let debounce = match debounce {
        Debounce::Recent => Duration::from_secs(sym.config.sync_debounce_secs),
        Debounce::Always => Duration::ZERO,
    };
    match &project {
        Some(p) => tracing::debug!(root = %p.root.display(), "resolved workspace root"),
        None => tracing::debug!("no workspace; syncing globally-enabled plugins only"),
    }

    // Load plugin registry (registry sources + workspace plugins)
    let registry = plugins::load_registry_with_workspace(sym, loaded.as_deref()).await;

    for warning in &registry.warnings {
        tracing::info!(
            report = %crate::report::ReportEvent::Warning {
                message: format!("skipping {}: {}", display_path(&warning.path), warning.message),
            },
        );
    }

    // Removing anything means knowing the complete set of what should exist,
    // and an unreadable source means we do not. An unmounted registry path
    // would otherwise read as "these plugins no longer apply" and uninstall
    // them from every agent. A single skipped *entry* is not this: it loses one
    // plugin, which genuinely should then be removed.
    let degraded = !registry.sources_readable;

    tracing::info!(
        report = %crate::report::ReportEvent::Info {
            message: format!("scanning {workspace_deps_count} workspace dependencies"),
        },
    );

    // Resolve custom predicate installations.
    let custom_entries = resolve_custom_predicate_entries(sym, &registry, update).await;

    // Resolve the workspace once and build the predicate context shared by
    // skill resolution and MCP-server filtering. Attach the on-disk cache so
    // custom predicate results survive across sync runs; results are persisted
    // at the end of this evaluation pass.
    let dep_ids = crate::pm::workspace_dep_ids(sym, deps).await;
    let used_names = match &project {
        Some(p) => sym.config.plugins.used_names_in(&p.root),
        None => sym.config.plugins.global_used_names(),
    };
    // The predicate cache is keyed on a workspace, so there is nothing to cache
    // against without one.
    let predicate_cache_path = project.as_ref().map(|p| {
        crate::predicate_cache::PredicateCache::path_for_workspace(sym.cache_dir(), &p.root)
    });
    let mut ctx =
        crate::predicate::PredicateContext::with_custom_predicates(&dep_ids, custom_entries)
            .with_used_names(&used_names);
    if let Some(path) = &predicate_cache_path {
        ctx = ctx.with_disk_cache(path);
    }

    // The active plugin set: registry plugins plus the crate-sourced plugins
    // reached through `[[plugins]]` chained references and dependency
    // enablement. Every facet resolves over this one set, so a crate plugin's
    // skills and MCP servers install exactly like a registry plugin's.
    let pms = sym.package_managers(deps);
    let active = plugins::active_plugins(
        sym,
        &registry,
        &pms,
        project.as_ref().map(|p| p.root.as_path()),
        &mut ctx,
    )
    .await;

    // Find all applicable skills.
    let applicable = skills::collect_skills(sym, &active, &mut ctx, update).await;

    // Dedup by `(skill_name, origin_hash)`: two crate origins with the same
    // (name, version, skill-path-within-crate) collapse (the same skill bytes
    // reached through two plugins); skills from genuinely different locations —
    // including two skills at different paths within one crate — survive
    // independently. Skills that survive dedup are recorded with both their
    // plain name and their origin hash so we can decide later whether each one
    // needs an `<name>-<hash>` suffix to avoid collisions.
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    let mut to_install: Vec<PendingSkill<'_>> = Vec::new();
    let mut name_counts: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();

    for entry in &applicable {
        let name = entry.skill.name().to_string();
        if seen.insert((name.clone(), entry.origin_hash.clone())) {
            *name_counts.entry(name.clone()).or_default() += 1;
            to_install.push(PendingSkill {
                name,
                origin_hash: entry.origin_hash.clone(),
                plugin: entry.plugin.clone(),
                plugin_id: entry.plugin_id.clone(),
                source: &entry.skill.path,
            });
        }
    }

    // Only compile for a scope some configured agent can actually take. With
    // none, the directory would sit unread and the skills still arrive through
    // the per-skill path.
    let configured: Vec<Agent> = sym
        .config
        .agents
        .iter()
        .filter_map(|a| Agent::from_config_name(&a.name).ok())
        .collect();
    let compiled: Vec<crate::agent_plugin::CompiledPlugin> =
        crate::agent_plugin::compile(&active, &applicable, &sym.config.plugins)
            .into_iter()
            .filter(|plugin| {
                configured
                    .iter()
                    .any(|agent| agent.accepts_plugin_scope(plugin.scope))
            })
            .collect();
    let global_staging = sym
        .config_dir()
        .join(crate::agent_plugin::GLOBAL_STAGING_DIR);
    let mut staged_project: BTreeSet<PathBuf> = BTreeSet::new();
    let mut staged_global: BTreeSet<PathBuf> = BTreeSet::new();

    if let Some(p) = &project
        && compiled.iter().any(|c| c.scope == Scope::Project)
        && let Err(e) = ignore_owned_dir(&p.owned, &p.root)
    {
        tracing::info!(
            report = %crate::report::ReportEvent::Warning {
                message: format!("failed to prepare {}: {e}", display_path(&p.owned)),
            },
        );
    }

    for plugin in &compiled {
        let target = match (plugin.scope, &project) {
            (Scope::Project, Some(p)) => Some((&p.staging, p.root.as_path(), &mut staged_project)),
            // Nowhere to put a project-scoped plugin without a project. Its
            // skills still reach the agents that read them individually.
            (Scope::Project, None) => None,
            (Scope::Global, _) => Some((&global_staging, sym.config_dir(), &mut staged_global)),
        };
        let Some((root, boundary, staged)) = target else {
            continue;
        };
        match crate::agent_plugin::write(plugin, root, boundary, debounce) {
            Ok(dest) => {
                tracing::info!(
                    report = %crate::report::ReportEvent::PluginCompiled {
                        plugin: plugin.dir_name.clone(),
                        scope: plugin.scope.as_str().to_string(),
                        skills: plugin.skills.len(),
                        dest: display_path(&dest),
                    },
                );
                staged.insert(dest);
            }
            Err(e) => tracing::info!(
                report = %crate::report::ReportEvent::Warning {
                    message: format!("failed to compile plugin {}: {e}", plugin.dir_name),
                },
            ),
        }
    }

    // Reaping the global root from a project sync is only sound because
    // `Scope::of` keeps the global set a function of user config alone.
    if !degraded {
        if let Some(p) = &project {
            crate::agent_plugin::reap(&p.staging, &staged_project);
        }
        crate::agent_plugin::reap(&global_staging, &staged_global);
    }

    for (scope, root) in staging_roots(&project, &global_staging) {
        let in_root: Vec<&crate::agent_plugin::CompiledPlugin> =
            compiled.iter().filter(|p| p.scope == scope).collect();
        if in_root.is_empty() && (degraded || !root.exists()) {
            continue;
        }
        let name = crate::agent_plugin::marketplace_name(
            scope,
            project.as_ref().map(|p| p.root.as_path()),
        );
        if let Err(e) = crate::agent_plugin::write_marketplace(root, &name, &in_root) {
            tracing::info!(
                report = %crate::report::ReportEvent::Warning {
                    message: format!("failed to index {}: {e}", display_path(root)),
                },
            );
        }
    }

    // Collect MCP servers from the same active plugin set.
    let mut mcp_servers: Vec<sacp::schema::McpServer> = Vec::new();
    for p in &active {
        if p.applies(&mut ctx) {
            mcp_servers.extend(p.plugin.applicable_mcp_servers(&mut ctx));
        }
    }
    if let Some(path) = &predicate_cache_path
        && let Err(e) = ctx.persist_disk_cache(path)
    {
        tracing::warn!(
            path = %path.display(),
            error = %e,
            "failed to persist predicate cache"
        );
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
        workspace_deps = workspace_deps_count,
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

    // Individually-installed skills go under the project when there is one. A
    // no-workspace sync still has to deliver a globally-enabled plugin's skills
    // to agents that cannot take the compiled directory, so they land under the
    // user's home instead.
    let skill_home = match &project {
        Some(p) => SkillHome::Project(&p.root),
        None => SkillHome::Global(sym.home_dir()),
    };

    // Track every skill directory we (re)install during this sync. Anything
    // we find later that has the marker file but isn't in this set is stale.
    let mut installed_dirs: BTreeSet<PathBuf> = BTreeSet::new();

    // Plugin copies each agent now owns, so stale ones can be reaped below.
    let mut agent_copies: std::collections::BTreeMap<Agent, BTreeSet<PathBuf>> =
        std::collections::BTreeMap::new();

    for agent_name in &agent_names {
        let agent = Agent::from_config_name(agent_name)?;

        // Hand over the compiled directories this agent can take, and note
        // which plugins that covers so their skills are not also installed
        // individually.
        let mut delivered: BTreeSet<crate::pm::PackageId> = BTreeSet::new();
        for (scope, root) in staging_roots(&project, &global_staging) {
            let in_scope: Vec<&crate::agent_plugin::CompiledPlugin> = compiled
                .iter()
                .filter(|p| p.scope == scope && agent.accepts_plugin_scope(scope))
                .collect();
            if in_scope.is_empty() && (degraded || !root.exists()) {
                continue;
            }
            let marketplace = crate::agent_plugin::marketplace_name(
                scope,
                project.as_ref().map(|p| p.root.as_path()),
            );
            let registration = crate::agents::Registration {
                marketplace: &marketplace,
                root,
                plugins: &in_scope,
                scope,
            };
            // Only a project-scoped registration needs the project path, and
            // that scope is unreachable without one.
            let enable_in = project
                .as_ref()
                .map_or(sym.home_dir(), |p| p.root.as_path());
            match agent.install_plugins(&registration, sym.home_dir(), enable_in, debounce) {
                Ok(copies) => {
                    agent_copies
                        .entry(agent)
                        .or_default()
                        .extend(copies.iter().cloned());
                    for plugin in &in_scope {
                        delivered.insert(plugin.source_id.clone());
                        tracing::info!(
                            report = %crate::report::ReportEvent::PluginDelivered {
                                plugin: plugin.dir_name.clone(),
                                agent: agent_name.clone(),
                                scope: scope.as_str().to_string(),
                                dest: display_path(
                                    copies
                                        .iter()
                                        .find(|c| c.ends_with(&plugin.dir_name))
                                        .map(PathBuf::as_path)
                                        .unwrap_or(root)
                                ),
                            },
                        );
                    }
                }
                Err(e) => tracing::info!(
                    report = %crate::report::ReportEvent::Warning {
                        message: format!("failed to install plugins for {agent_name}: {e}"),
                    },
                ),
            }
        }

        let hook_root = match (sym.config.hook_scope, &project) {
            (crate::config::HookScope::Project, Some(p)) => p.root.clone(),
            // Project hook scope has nowhere to write without a project, so the
            // user-level registration stands in.
            _ => sym.home_dir().to_path_buf(),
        };

        // Register hooks and MCP servers
        agent
            .register_hooks(&hook_root, sym, out)
            .context("failed to register hooks")?;
        agent
            .register_global_mcp_servers(&hook_root, &mcp_servers, out)
            .context("failed to register MCP servers")?;

        for pending in &to_install {
            let PendingSkill {
                name: skill_name,
                origin_hash,
                plugin,
                plugin_id,
                source,
            } = pending;

            // Already delivered to this agent as a plugin directory, which is
            // the whole point of compiling one. Agents that cannot take the
            // plugin still get the skill the old way.
            if delivered.contains(plugin_id) {
                continue;
            }
            // `source` is the path to the SKILL.md file; the skill directory
            // is its parent.
            let source_dir = match source.parent() {
                Some(p) => p,
                None => {
                    out.warn(format!(
                        "skill {skill_name}: cannot determine source directory"
                    ));
                    continue;
                }
            };

            // The skill's source already sits at this agent's install slot
            // (a workspace `.agents/skills/` skill, on an agent that reads
            // that same directory) — it is in place as user content, not
            // something to copy.
            let Some(plain_dir) = skill_home.dir_for(agent, skill_name) else {
                continue;
            };
            let in_place = match (source_dir.canonicalize(), plain_dir.canonicalize()) {
                (Ok(a), Ok(b)) => a == b,
                _ => false,
            };
            if in_place {
                continue;
            }

            // Pick the install dir name for this skill on *this* agent:
            // - If exactly one origin claims the name and the un-suffixed
            //   slot is "available" (nonexistent or symposium-managed),
            //   use the plain `<skill-name>/`.
            // - Otherwise fall back to `<skill-name>-<origin-hash>/` so
            //   distinct origins coexist and we never clobber a
            //   user-managed directory.
            let unique_name = name_counts.get(skill_name).copied().unwrap_or(0) == 1;
            let plain_available = !plain_dir.exists() || has_symposium_marker(&plain_dir);
            let dir_name = if unique_name && plain_available {
                skill_name.clone()
            } else {
                format!("{skill_name}-{}", origin_hash)
            };
            let Some(dest_dir) = skill_home.dir_for(agent, &dir_name) else {
                continue;
            };

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

            match sync_managed_dir(
                source_dir,
                &dest_dir,
                skill_home.boundary(),
                debounce,
                Marking::MarkerAndGitignore,
            ) {
                Ok(true) => {
                    installed_dirs.insert(dest_dir.clone());
                    tracing::info!(
                        report = %crate::report::ReportEvent::SkillInstalled {
                            skill: dir_name.clone(),
                            plugin: plugin.clone(),
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

    // Stale-skill cleanup: scan every agent's skills parent directory (across
    // all known agents, so we also clean up after agents removed from config)
    // and remove subdirs containing the marker that we didn't just install.
    let mut scanned: BTreeSet<PathBuf> = BTreeSet::new();
    for &agent in Agent::all() {
        let Some(parent) = skill_home.parent_for(agent) else {
            continue;
        };
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

    // Reap plugin copies we no longer own, across every known agent so an agent
    // dropped from the config is cleaned up too.
    if !degraded {
        for &agent in Agent::all() {
            let written = agent_copies.get(&agent).cloned().unwrap_or_default();
            for root in agent.plugin_reap_roots(sym.home_dir()) {
                crate::agent_plugin::reap_to_depth(
                    &root,
                    crate::agent_plugin::AGENT_COPY_DEPTH,
                    &written,
                );
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
pub async fn register_hooks(sym: &Symposium, out: &Output) -> Result<()> {
    let registry = plugins::load_registry(sym).await;
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
