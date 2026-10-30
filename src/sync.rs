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

use crate::agents::Agent;
use crate::compile;
use crate::config::{INSTALLED_PLUGINS_SUBDIR, Symposium};
use crate::dir_walk::{self, EntryKind};
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

fn slot_taken(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// Is `path` a real directory holding the `.symposium` marker, i.e. a
/// symposium-managed skill directory? Returns `false` for user-authored skills,
/// for any directory symposium did not create, and for any symlink.
fn is_managed_skill_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir()) && has_symposium_marker(path)
}

pub(crate) fn has_symposium_marker(dir: &Path) -> bool {
    dir.join(MARKER_FILE).is_file()
}

pub(crate) fn ignore_in_git(dir: &Path) -> Result<()> {
    let path = dir.join(".gitignore");
    if path.is_file() {
        return Ok(());
    }
    fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    fs::write(&path, "*\n").with_context(|| format!("write {}", path.display()))
}

/// One entry of a source tree as the installer sees it: a path relative to the
/// tree root, with symlinks already resolved.
type TreeEntry = (PathBuf, EntryKind);

/// Walk `root` depth-first with symlinks followed, returning every directory
/// and regular file under it as a path relative to `root`, sorted, together
/// with one message per entry the walk refused to follow.
///
/// Copying and comparing both go through this one walk, because they have to
/// agree entry for entry. A comparison scan that omitted something the copier
/// writes would see the destination differ from the source on every run, and
/// every sync would become a full delete-and-recopy.
///
/// Sorting by relative path puts each directory before its children, so a
/// caller may create or read the entries in order.
fn walk_tree(root: &Path) -> Result<(Vec<TreeEntry>, Vec<String>)> {
    let mut entries = Vec::new();
    let mut skipped = Vec::new();
    let mut ancestors = dir_walk::Ancestors::rooted_at(root);
    walk_tree_inner(root, root, &mut ancestors, &mut entries, &mut skipped)?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok((entries, skipped))
}

fn walk_tree_inner(
    root: &Path,
    dir: &Path,
    ancestors: &mut dir_walk::Ancestors,
    entries: &mut Vec<TreeEntry>,
    skipped: &mut Vec<String>,
) -> Result<()> {
    let read = match fs::read_dir(dir) {
        Ok(read) => read,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e).with_context(|| format!("read {}", dir.display())),
    };
    for entry in read {
        let entry = entry?;
        let path = entry.path();
        let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        let file_type = entry.file_type()?;
        match dir_walk::resolved_kind(&path, file_type) {
            Ok(Some(EntryKind::Dir)) => {
                if !ancestors.enter(&path) {
                    skipped.push(format!(
                        "skipping {}: symlink re-enters a directory it is inside",
                        display_path(&path)
                    ));
                    continue;
                }
                entries.push((rel, EntryKind::Dir));
                walk_tree_inner(root, &path, ancestors, entries, skipped)?;
                ancestors.leave();
            }
            Ok(Some(EntryKind::File)) => entries.push((rel, EntryKind::File)),
            // A socket or a fifo. Never skill content, so never worth a word.
            Ok(None) => {}
            Err(e) => skipped.push(format!(
                "skipping {}: broken symlink ({e})",
                display_path(&path)
            )),
        }
    }
    Ok(())
}

/// Recursively copy the contents of `src` into `dst`. Creates `dst` if
/// missing. Symlinks are followed: a link to a file lands as a real file, a
/// link to a directory as a real directory holding a copy of its contents.
///
/// Entries the walk cannot follow are reported and skipped, so a single broken
/// link leaves the rest of the skill installed instead of failing the sync.
fn copy_dir_recursive(src: &Path, dst: &Path, warned: &mut BTreeSet<String>) -> Result<()> {
    let (entries, skipped) = walk_tree(src)?;
    report_skipped(skipped, warned);
    fs::create_dir_all(dst).with_context(|| format!("create {}", dst.display()))?;
    for (rel, kind) in entries {
        let dst_path = dst.join(&rel);
        match kind {
            EntryKind::Dir => {
                fs::create_dir_all(&dst_path)
                    .with_context(|| format!("create {}", dst_path.display()))?;
            }
            EntryKind::File => {
                let src_path = src.join(&rel);
                fs::copy(&src_path, &dst_path).with_context(|| {
                    format!("copy {} → {}", src_path.display(), dst_path.display())
                })?;
            }
        }
    }
    Ok(())
}

pub(crate) type TreeContents = Vec<(PathBuf, Option<Vec<u8>>)>;

/// Collect `dir`'s tree for comparison: every directory and regular file under
/// it, as a path relative to `dir` paired with the file's contents (`None` for
/// a directory). Symlinks resolve exactly as [`copy_dir_recursive`] resolves
/// them, so the source of an installed skill and its destination compare equal.
///
/// Skips the `.symposium` marker and `.gitignore`, which are managed metadata
/// rather than skill content.
pub(crate) fn collect_dir_contents(dir: &Path) -> Result<(TreeContents, Vec<String>)> {
    let (entries, skipped) = walk_tree(dir)?;
    let mut out = Vec::with_capacity(entries.len());
    for (rel, kind) in entries {
        let name = rel.to_string_lossy();
        if name == MARKER_FILE || name == ".gitignore" {
            continue;
        }
        let contents = match kind {
            EntryKind::Dir => None,
            EntryKind::File => {
                let path = dir.join(&rel);
                Some(fs::read(&path).with_context(|| format!("read {}", path.display()))?)
            }
        };
        out.push((rel, contents));
    }
    Ok((out, skipped))
}

pub(crate) fn report_skipped(skipped: Vec<String>, warned: &mut BTreeSet<String>) {
    for message in skipped {
        if warned.insert(message.clone()) {
            tracing::info!(report = %crate::report::ReportEvent::Warning { message });
        }
    }
}

pub(crate) fn recently_synced(dir: &Path, debounce: Duration) -> bool {
    !debounce.is_zero()
        && fs::metadata(dir.join(MARKER_FILE))
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|mtime| SystemTime::now().duration_since(mtime).ok())
            .is_some_and(|elapsed| elapsed < debounce)
}

pub(crate) fn sync_tree(dest: &Path, sorted: &TreeContents) -> Result<bool> {
    if dest.exists() && !is_managed_skill_dir(dest) {
        anyhow::bail!("{} is not managed by symposium", display_path(dest));
    }
    if dest.is_dir() && collect_dir_contents(dest)?.0 == *sorted {
        touch_marker(&dest.join(MARKER_FILE))?;
        return Ok(false);
    }
    if dest.exists() {
        fs::remove_dir_all(dest).with_context(|| format!("remove {}", dest.display()))?;
    }
    fs::create_dir_all(dest).with_context(|| format!("create {}", dest.display()))?;
    touch_marker(&dest.join(MARKER_FILE))?;
    for (rel, data) in sorted {
        let path = dest.join(rel);
        match data {
            None => fs::create_dir_all(&path),
            Some(bytes) => fs::write(&path, bytes),
        }
        .with_context(|| format!("write {}", path.display()))?;
    }
    Ok(true)
}

pub(crate) fn sync_plugin_dir(source: &Path, dest: &Path, debounce: Duration) -> Result<bool> {
    if recently_synced(dest, debounce) {
        return Ok(false);
    }
    sync_tree(dest, &collect_dir_contents(source)?.0)
}

pub(crate) fn reap_unlisted(parent: &Path, keep: impl Fn(&Path) -> bool) {
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !is_managed_skill_dir(&path) || keep(&path) {
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

/// Returns true if the source directory's content differs from the
/// destination's content (ignoring managed metadata files).
fn dir_contents_differ(source_dir: &Path, dest_dir: &Path) -> Result<bool> {
    let (src, _) = collect_dir_contents(source_dir)?;
    let (dst, _) = collect_dir_contents(dest_dir)?;
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
/// 3. Otherwise builds a complete, marked copy beside `dest_dir` and swaps it
///    in, so a failed copy leaves the slot as it was.
///
/// Returns `Ok(true)` if the destination was created or updated (callers
/// record it as installed). Returns `Ok(false)` if skipped (no-op).
fn sync_skill_dir(
    source_dir: &Path,
    dest_dir: &Path,
    project_root: &Path,
    debounce: Duration,
    warned: &mut BTreeSet<String>,
) -> Result<bool> {
    if dest_dir == source_dir {
        return Ok(false);
    }

    // If the destination doesn't exist yet, do a fresh install.
    if !slot_taken(dest_dir) {
        install_skill_dir(source_dir, dest_dir, project_root, warned)?;
        return Ok(true);
    }

    // Debounce: if we synced recently, skip the content comparison.
    if recently_synced(dest_dir, debounce) {
        tracing::debug!(dest = %dest_dir.display(), "skill sync debounced");
        return Ok(false);
    }

    // Compare content (excluding managed metadata).
    let marker_path = dest_dir.join(MARKER_FILE);
    if !dir_contents_differ(source_dir, dest_dir)? {
        // Content is identical — just touch the marker to reset debounce.
        touch_marker(&marker_path)?;
        return Ok(false);
    }

    // Content changed: replace entirely.
    install_skill_dir(source_dir, dest_dir, project_root, warned)?;
    Ok(true)
}

fn install_skill_dir(
    source_dir: &Path,
    dest_dir: &Path,
    project_root: &Path,
    warned: &mut BTreeSet<String>,
) -> Result<()> {
    let parent = dest_dir.parent().expect("skill dir must have parent");
    create_managed_dir_all(parent, project_root)?;
    let mut staging = tempfile::Builder::new()
        .prefix(".symposium-tmp-")
        .tempdir_in(parent)
        .with_context(|| format!("create staging dir in {}", parent.display()))?;
    mark_generated_skill_directory(staging.path())?;
    copy_dir_recursive(source_dir, staging.path(), warned)?;
    mark_generated_skill_directory(staging.path())?;
    if slot_taken(dest_dir) {
        fs::remove_dir_all(dest_dir).with_context(|| format!("remove {}", dest_dir.display()))?;
    }
    fs::rename(staging.path(), dest_dir).with_context(|| {
        format!(
            "rename {} -> {}",
            staging.path().display(),
            dest_dir.display()
        )
    })?;
    staging.disable_cleanup(true);
    Ok(())
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
        let Some(install) = plugin.manifest.get_installation(&resolved.command) else {
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

/// Run the full sync: discover applicable skills, install into agent dirs,
/// clean up stale installations.
pub async fn sync(
    sym: &Symposium,
    deps: &Arc<WorkspaceDeps>,
    update: UpdateLevel,
) -> Result<Vec<(Agent, Vec<String>)>> {
    if let Some(error) = sym.config_error() {
        anyhow::bail!(
            "not syncing: {} could not be read; fix or remove it, then run the command again\n{error}",
            display_path(&sym.config_dir().join("config.toml")),
        );
    }
    let out = &Output::quiet();
    let loaded = deps.require()?;
    let project_root = loaded.root.clone();
    let workspace: Vec<_> = loaded.crates.clone();
    let loaded = loaded.clone();
    let debounce = Duration::from_secs(sym.config.sync_debounce_secs);
    tracing::debug!(root = %project_root.display(), "resolved workspace root");

    // Load plugin registry (registry sources + workspace plugins)
    let registry = plugins::load_registry_with_workspace(sym, Some(&loaded)).await;

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
    let custom_entries = resolve_custom_predicate_entries(sym, &registry, update).await;

    // Resolve the workspace once and build the predicate context shared by
    // skill resolution and MCP-server filtering. Attach the on-disk cache so
    // custom predicate results survive across sync runs; results are persisted
    // at the end of this evaluation pass.
    let dep_ids = crate::pm::workspace_dep_ids(sym, deps).await;
    let pms = sym.package_managers(deps);
    let used = crate::discovery::used_plugins(sym, &pms, &registry.plugins, &project_root);
    let predicate_cache_path =
        crate::predicate_cache::PredicateCache::path_for_workspace(sym.cache_dir(), &project_root);
    let mut ctx =
        crate::predicate::PredicateContext::with_custom_predicates(&dep_ids, custom_entries)
            .with_used(&used)
            .with_disk_cache(&predicate_cache_path);

    // The active plugin set: registry plugins plus the crate-sourced plugins
    // reached through `[[plugins]]` chained references and dependency
    // enablement. Every facet resolves over this one set, so a crate plugin's
    // skills and MCP servers install exactly like a registry plugin's.
    let active = plugins::active_plugins(
        sym,
        &registry,
        &pms,
        Some(&project_root),
        &mut ctx,
        UpdateLevel::Check,
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
    let mut to_install: Vec<(String, String, &std::path::Path)> = Vec::new();

    for entry in &applicable {
        let name = entry.skill.name().to_string();
        if seen.insert((name.clone(), entry.origin_hash.clone())) {
            to_install.push((name, entry.origin_hash.clone(), &entry.skill.path));
        }
    }

    // Collect MCP servers from the same active plugin set.
    let mut mcp_servers: Vec<sacp::schema::McpServer> = Vec::new();
    for p in &active {
        if p.applies(&mut ctx) {
            mcp_servers.extend(p.manifest.applicable_mcp_servers(&mut ctx));
        }
    }
    let global = compile::global_plugins(sym, &pms, &registry, &active, &mut ctx, update).await;
    let project = if agent_names_include(sym, Agent::Claude) {
        compile::project_plugins(&active, &applicable, &global)
    } else {
        Vec::new()
    };
    if let Err(e) = ctx.persist_disk_cache(&predicate_cache_path) {
        tracing::warn!(
            path = %predicate_cache_path.display(),
            error = %e,
            "failed to persist predicate cache"
        );
    }

    // Sync each configured agent
    let agent_names: Vec<String> = sym.config.agents.iter().map(|a| a.name.clone()).collect();

    tracing::info!(
        workspace_deps = workspace.len(),
        agents = agent_names.len(),
        skills = to_install.len(),
        "sync started"
    );

    let _lock = lock_sync(sym)?;

    if agent_names.is_empty() {
        tracing::info!(
            report = %crate::report::ReportEvent::Info {
                message: "no agents configured, run `cargo agents init` to add one".into(),
            },
        );
    }

    // Track every skill directory this sync wants, even one whose refresh
    // failed. Anything we find later that has the marker file but isn't in
    // this set is stale.
    let mut installed_dirs: BTreeSet<PathBuf> = BTreeSet::new();
    let skip_cleanup = !registry.unreadable_registries.is_empty();

    let mut warned = BTreeSet::new();
    let plugins_root = sym.config_dir().join(INSTALLED_PLUGINS_SUBDIR);
    let compiled = compile::compile(&plugins_root, &global, debounce, &mut warned, skip_cleanup);
    crate::agents::sync_marketplace_index(sym, &compiled.plugins);
    let project_compiled =
        compile::compile_project(&project_root, &project, debounce, &mut warned, skip_cleanup);

    let mut delivered = BTreeSet::new();
    let mut project_delivered = BTreeSet::new();
    let mut plugin_changes = Vec::new();
    let mut copied: std::collections::BTreeMap<_, BTreeSet<_>> = std::collections::BTreeMap::new();
    for agent_name in &agent_names {
        let Ok(agent) = Agent::from_config_name(agent_name) else {
            continue;
        };
        let user_changes = agent.deliver_user_plugins(sym, &compiled.plugins);
        let project_changes =
            agent.deliver_project_plugins(sym, &project_root, &project_compiled.plugins);
        let takes_plugins = user_changes.is_some();
        let takes_project_plugins = project_changes.is_some();
        if takes_plugins {
            delivered.insert(agent_name);
        }
        if takes_project_plugins {
            project_delivered.insert(agent_name);
        }
        if takes_plugins || takes_project_plugins {
            let changed = user_changes.into_iter().chain(project_changes).flatten();
            plugin_changes.push((agent, changed.collect()));
        }
        let dir = copied
            .entry(skills_parent_dir(agent, &project_root))
            .or_default();
        for (skill_name, origin_hash, _) in &to_install {
            if !(takes_plugins && compiled.skill_origins.contains(origin_hash)
                || takes_project_plugins && project_compiled.skill_origins.contains(origin_hash))
            {
                dir.insert((skill_name, origin_hash));
            }
        }
    }

    for agent_name in &agent_names {
        let Ok(agent) = Agent::from_config_name(agent_name) else {
            tracing::info!(
                report = %crate::report::ReportEvent::Warning {
                    message: unknown_agent_warning(agent_name),
                },
            );
            continue;
        };

        // Hooks: the project and global locations are genuinely different files
        // for some agents - Antigravity writes `.agents/hooks.json` but
        // `~/.gemini/config/hooks.json`, and Copilot `.github/hooks/` but
        // `~/.copilot/settings.json` - so project scope cannot be produced by
        // rooting the global path at the workspace.
        let hooks = match sym.config.hook_scope {
            crate::config::HookScope::Global => agent.register_hooks(sym.home_dir(), sym, out),
            crate::config::HookScope::Project => {
                agent.register_project_hooks(&project_root, sym, out)
            }
        };
        if let Err(e) = hooks {
            tracing::info!(
                report = %crate::report::ReportEvent::Warning {
                    message: format!("failed to register hooks for {agent_name}: {e:#}"),
                },
            );
        }

        // MCP does not follow the hook scope: an agent's MCP file differs from
        // its hooks file, so the target is resolved per agent instead.
        let mcp_scope = match sym.config.hook_scope {
            crate::config::HookScope::Global => crate::agents::McpScope::User,
            crate::config::HookScope::Project => crate::agents::McpScope::Project,
        };
        // Reported, not logged: the user asked for project scope and is getting a
        // machine-wide entry.
        if mcp_scope == crate::agents::McpScope::Project
            && !agent.supports_project_mcp_scope()
            && !mcp_servers.is_empty()
        {
            tracing::info!(
                report = %crate::report::ReportEvent::Info {
                    message: format!(
                        "{} has no project-level MCP config; registering its servers at user level",
                        agent.display_name(),
                    ),
                },
            );
        }
        match agent.register_mcp_servers(mcp_scope, &project_root, sym, &mcp_servers, out) {
            Ok(names) => {
                let file = agent.mcp_config_path(mcp_scope, &project_root, sym);
                let project_file =
                    agent.mcp_config_path(crate::agents::McpScope::Project, &project_root, sym);
                let offered: &[String] = if file == project_file { &names } else { &[] };
                if agent.supports_project_mcp_scope() && !skip_cleanup {
                    unregister_recorded_mcp_servers(
                        sym,
                        |_, registration| {
                            registration.agent == *agent_name
                                && registration.file == project_file
                                && !offered.contains(&registration.name)
                        },
                        out,
                    );
                }
                record_mcp_registrations(sym, agent, &file, names);
            }
            Err(e) => {
                tracing::info!(
                    report = %crate::report::ReportEvent::Warning {
                        message: format!("failed to register MCP servers for {agent_name}: {e:#}"),
                    },
                );
            }
        }

        let mut name_counts = std::collections::BTreeMap::new();
        for (skill_name, _) in &copied[&skills_parent_dir(agent, &project_root)] {
            *name_counts.entry(*skill_name).or_default() += 1;
        }

        for (skill_name, origin_hash, skill_source) in &to_install {
            if delivered.contains(agent_name) && compiled.skill_origins.contains(origin_hash)
                || project_delivered.contains(agent_name)
                    && project_compiled.skill_origins.contains(origin_hash)
            {
                continue;
            }
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

            let plain_dir = agent.project_skill_dir(&project_root, skill_name);
            let shared_dir = project_root.join(".agents").join("skills").join(skill_name);
            let is_source = |dir: &Path| match (source_dir.canonicalize(), dir.canonicalize()) {
                (Ok(a), Ok(b)) => a == b,
                _ => false,
            };
            let already_in_place = is_source(&plain_dir)
                || (agent.reads_vendor_neutral_skills() && is_source(&shared_dir));
            if already_in_place {
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
            let plain_available = !slot_taken(&plain_dir) || is_managed_skill_dir(&plain_dir);
            let dir_name = if unique_name && plain_available {
                skill_name.clone()
            } else {
                format!("{skill_name}-{}", origin_hash)
            };
            let dest_dir = agent.project_skill_dir(&project_root, &dir_name);

            // If the dest exists but is user-managed, skip it.
            if slot_taken(&dest_dir) && !is_managed_skill_dir(&dest_dir) {
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

            installed_dirs.insert(dest_dir.clone());
            match sync_skill_dir(source_dir, &dest_dir, &project_root, debounce, &mut warned) {
                Ok(true) => {
                    tracing::info!(
                        report = %crate::report::ReportEvent::SkillInstalled {
                            skill: dir_name.clone(),
                            agent: agent_name.clone(),
                            dest: display_path(&dest_dir),
                            plugin: false,
                        },
                    );
                }
                Ok(false) => {}
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
    // Compared by resolved path, since one agent's skills dir can be a symlink
    // to another's. A symlink entry is user content and is never followed.
    let installed: BTreeSet<PathBuf> = installed_dirs
        .iter()
        .filter_map(|dir| dir.canonicalize().ok())
        .collect();
    let mut scanned: BTreeSet<PathBuf> = BTreeSet::new();
    for &agent in Agent::all() {
        let Ok(parent) = skills_parent_dir(agent, &project_root).canonicalize() else {
            continue;
        };
        if skip_cleanup || !scanned.insert(parent.clone()) {
            continue;
        }
        reap_unlisted(&parent, |dir| {
            installed.contains(dir) || compile::is_plugin_dir(dir)
        });
    }
    for name in &registry.unreadable_registries {
        tracing::info!(
            report = %crate::report::ReportEvent::Warning {
                message: format!(
                    "registry `{name}` could not be fully read; skipped cleanup of installed skills and plugins"
                ),
            },
        );
    }

    // Hooks come off at whichever scope they would have been written to. MCP
    // entries come off the user-level file and this workspace's files; another
    // workspace's project file waits for a sync there.
    for &agent in Agent::all() {
        if !agent_names.contains(&agent.config_name().to_string()) {
            match sym.config.hook_scope {
                crate::config::HookScope::Global => {
                    agent.unregister_hooks(sym.home_dir(), sym, out)
                }
                crate::config::HookScope::Project => {
                    agent.unregister_project_hooks(&project_root, sym, out)
                }
            }
        }
    }
    unregister_unconfigured_mcp_servers(
        sym,
        &agent_names,
        |agent, file| {
            file.starts_with(&project_root)
                || file == agent.mcp_config_path(crate::agents::McpScope::User, &project_root, sym)
        },
        out,
    );
    if !skip_cleanup {
        unregister_user_plugins(sym);
        unregister_project_plugins(sym, &project_root);
    }

    if to_install.is_empty() {
        tracing::info!(
            report = %crate::report::ReportEvent::Info {
                message: "no applicable skills found for workspace dependencies".into(),
            },
        );
    }

    Ok(plugin_changes)
}

fn lock_sync(sym: &Symposium) -> Result<fs::File> {
    let cache_dir = sym.cache_dir();
    fs::create_dir_all(cache_dir).with_context(|| format!("create {}", cache_dir.display()))?;
    let path = cache_dir.join("sync.lock");
    let file = fs::File::options()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    file.lock()
        .with_context(|| format!("lock {}", path.display()))?;
    Ok(file)
}

pub(crate) fn unregister_user_plugins(sym: &Symposium) {
    for &agent in Agent::all() {
        if !agent_names_include(sym, agent) {
            agent.deliver_user_plugins(sym, &[]);
        }
    }
}

fn unregister_project_plugins(sym: &Symposium, project_root: &Path) {
    for &agent in Agent::all() {
        if !agent_names_include(sym, agent) {
            agent.deliver_project_plugins(sym, project_root, &[]);
        }
    }
}

fn agent_names_include(sym: &Symposium, agent: Agent) -> bool {
    sym.config
        .agents
        .iter()
        .any(|entry| entry.name == agent.config_name())
}

/// Register global hooks for all configured agents.
/// Register hooks for all configured agents. Uses `home_dir` (global scope).
/// Called from `init` after writing the user config.
pub async fn register_hooks(sym: &Symposium, out: &Output) -> Result<()> {
    let registry = plugins::load_registry(sym).await;
    let mcp_servers: Vec<sacp::schema::McpServer> = registry
        .plugins
        .iter()
        .flat_map(|p| p.manifest.mcp_servers.iter().map(|s| s.server.clone()))
        .collect();

    let agent_names: Vec<String> = sym.config.agents.iter().map(|a| a.name.clone()).collect();

    // `init` has no workspace in hand, so this is the user-level pass only;
    // the project-level file is written by the first `sync` in a workspace.
    let home = sym.home_dir();
    for agent_name in &agent_names {
        let Ok(agent) = Agent::from_config_name(agent_name) else {
            out.warn(unknown_agent_warning(agent_name));
            continue;
        };
        agent.register_hooks(home, sym, out)?;
        let names = agent.register_mcp_servers(
            crate::agents::McpScope::User,
            home,
            sym,
            &mcp_servers,
            out,
        )?;
        record_mcp_registrations(
            sym,
            agent,
            &agent.mcp_config_path(crate::agents::McpScope::User, home, sym),
            names,
        );
    }

    // Unregister hooks for agents no longer configured
    for &agent in Agent::all() {
        if !agent_names.contains(&agent.config_name().to_string()) {
            agent.unregister_hooks(home, sym, out);
        }
    }
    unregister_unconfigured_mcp_servers(
        sym,
        &agent_names,
        |agent, file| file == agent.mcp_config_path(crate::agents::McpScope::User, home, sym),
        out,
    );

    Ok(())
}

fn record_mcp_registrations(sym: &Symposium, agent: Agent, file: &Path, names: Vec<String>) {
    crate::state::record_mcp_registrations(
        sym.config_dir(),
        names
            .into_iter()
            .map(|name| crate::state::McpRegistration {
                agent: agent.config_name().to_string(),
                file: file.to_path_buf(),
                name,
            })
            .collect(),
    );
}

fn unknown_agent_warning(agent_name: &str) -> String {
    format!(
        "unknown agent `{agent_name}` in config.toml, skipped; remove it with `cargo agents init --remove-agent {agent_name}`"
    )
}

/// Remove the MCP entries Symposium wrote for agents that are not configured,
/// in the files `in_reach` accepts.
fn unregister_unconfigured_mcp_servers(
    sym: &Symposium,
    agent_names: &[String],
    in_reach: impl Fn(Agent, &Path) -> bool,
    out: &Output,
) {
    unregister_recorded_mcp_servers(
        sym,
        |agent, registration| {
            !agent_names.contains(&registration.agent) && in_reach(agent, &registration.file)
        },
        out,
    );
}

/// An entry whose removal fails stays recorded, so the next run retries it.
fn unregister_recorded_mcp_servers(
    sym: &Symposium,
    stale: impl Fn(Agent, &crate::state::McpRegistration) -> bool,
    out: &Output,
) {
    let mut removed = Vec::new();
    for registration in crate::state::mcp_registrations(sym.config_dir()) {
        let Ok(agent) = Agent::from_config_name(&registration.agent) else {
            continue;
        };
        if stale(agent, &registration)
            && agent
                .unregister_mcp_servers(&registration.file, &[&registration.name], out)
                .is_ok()
        {
            removed.push(registration);
        }
    }
    crate::state::forget_mcp_registrations(sym.config_dir(), &removed);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A source tree holding, in order: a plain file, a nested directory, an
    /// empty directory, a link to a file outside the tree, a link to a
    /// directory outside the tree, a broken link, and a link back to the root.
    #[cfg(unix)]
    fn tree_with_symlinks(root: &Path) -> PathBuf {
        use std::os::unix::fs::symlink;

        let outside = root.join("outside");
        fs::create_dir_all(outside.join("nested")).unwrap();
        fs::write(outside.join("lib.txt"), "shared").unwrap();
        fs::write(outside.join("nested/deep.txt"), "deep").unwrap();

        let src = root.join("src");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::create_dir(src.join("empty")).unwrap();
        fs::write(src.join("SKILL.md"), "skill").unwrap();
        fs::write(src.join("sub/plain.txt"), "plain").unwrap();
        symlink("../outside/lib.txt", src.join("lib.txt")).unwrap();
        // A chain: the first link resolves to a second one, itself relative to
        // a third directory. Whole chains are followed, not just one hop.
        symlink("../outside/hop.txt", src.join("chain.txt")).unwrap();
        symlink("nested/../lib.txt", outside.join("hop.txt")).unwrap();
        symlink("../outside", src.join("vendor")).unwrap();
        symlink("gone.txt", src.join("broken.txt")).unwrap();
        symlink(".", src.join("loop")).unwrap();
        src
    }

    /// The installed copy is the content behind the links, as real files, and
    /// the comparison that guards the next sync agrees with what was written.
    #[cfg(unix)]
    #[test]
    fn copy_follows_symlinks_and_compares_equal_afterwards() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tree_with_symlinks(tmp.path());
        let dst = tmp.path().join("dst");

        copy_dir_recursive(&src, &dst, &mut BTreeSet::new()).unwrap();

        assert!(
            !fs::symlink_metadata(dst.join("lib.txt"))
                .unwrap()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(dst.join("lib.txt")).unwrap(), "shared");
        assert_eq!(
            fs::read_to_string(dst.join("chain.txt")).unwrap(),
            "shared",
            "a link chain resolves to its final target"
        );
        assert_eq!(
            fs::read_to_string(dst.join("vendor/lib.txt")).unwrap(),
            "shared"
        );
        assert_eq!(
            fs::read_to_string(dst.join("vendor/nested/deep.txt")).unwrap(),
            "deep"
        );
        assert_eq!(
            fs::read_to_string(dst.join("sub/plain.txt")).unwrap(),
            "plain"
        );
        assert!(
            dst.join("empty").is_dir(),
            "an empty directory still copies"
        );
        assert!(!dst.join("broken.txt").exists(), "a broken link is skipped");
        assert!(
            !dst.join("loop").exists(),
            "a link back to the root is skipped"
        );

        assert!(
            !dir_contents_differ(&src, &dst).unwrap(),
            "the source scan must see exactly what the copier wrote, or every \
             sync turns into a full recopy"
        );
    }

    #[cfg(unix)]
    #[test]
    fn unfollowable_entries_are_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tree_with_symlinks(tmp.path());

        let (_, skipped) = walk_tree(&src).unwrap();

        assert!(
            skipped
                .iter()
                .any(|m| m.contains("broken.txt") && m.contains("broken symlink")),
            "{skipped:?}"
        );
        assert!(
            skipped
                .iter()
                .any(|m| m.contains("loop") && m.contains("re-enters")),
            "{skipped:?}"
        );
    }

    /// A change behind a link is a change to the skill.
    #[cfg(unix)]
    #[test]
    fn editing_a_linked_file_is_seen_as_a_difference() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tree_with_symlinks(tmp.path());
        let dst = tmp.path().join("dst");
        copy_dir_recursive(&src, &dst, &mut BTreeSet::new()).unwrap();

        fs::write(tmp.path().join("outside/lib.txt"), "edited").unwrap();

        assert!(dir_contents_differ(&src, &dst).unwrap());
    }

    #[test]
    fn only_a_marker_file_marks_a_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let marked = tmp.path().join("marked");
        let project = tmp.path().join("project");
        let plain = tmp.path().join("plain");
        for dir in [&marked, &project, &plain] {
            fs::create_dir_all(dir.join("guide")).unwrap();
            fs::write(dir.join("guide/SKILL.md"), "").unwrap();
        }
        fs::write(marked.join(MARKER_FILE), "").unwrap();
        fs::create_dir(project.join(MARKER_FILE)).unwrap();

        for (dir, is_marked) in [(&marked, true), (&project, false), (&plain, false)] {
            assert_eq!(has_symposium_marker(dir), is_marked, "{}", dir.display());
            let mut found = Vec::new();
            skills::find_skill_files_recursive(dir, &mut found);
            assert_eq!(found.is_empty(), is_marked, "{}", dir.display());
        }
    }

    #[test]
    fn a_plain_tree_copies_and_compares_equal() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("SKILL.md"), "skill").unwrap();
        fs::write(src.join("sub/a.txt"), "a").unwrap();
        let dst = tmp.path().join("dst");

        copy_dir_recursive(&src, &dst, &mut BTreeSet::new()).unwrap();

        assert_eq!(fs::read_to_string(dst.join("sub/a.txt")).unwrap(), "a");
        assert!(!dir_contents_differ(&src, &dst).unwrap());
    }

    /// Managed metadata is excluded from the comparison, so writing the marker
    /// and gitignore into the destination does not make it look changed.
    #[test]
    fn managed_metadata_is_not_skill_content() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("SKILL.md"), "skill").unwrap();
        let dst = tmp.path().join("dst");

        copy_dir_recursive(&src, &dst, &mut BTreeSet::new()).unwrap();
        mark_generated_skill_directory(&dst).unwrap();

        assert!(!dir_contents_differ(&src, &dst).unwrap());
    }

    #[test]
    fn install_preserves_unmarked_directory_at_staging_path() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("SKILL.md"), "skill").unwrap();

        let skills = tmp.path().join("skills");
        let user_dir = skills.join(".symposium-tmp-alpha");
        fs::create_dir_all(&user_dir).unwrap();
        let notes = user_dir.join("notes.txt");
        fs::write(&notes, "user-owned notes").unwrap();
        assert!(!user_dir.join(MARKER_FILE).exists());

        // Either choose another staging path or fail safely. Neither outcome
        // may delete or adopt this pre-existing, unmarked directory.
        let _ = sync_skill_dir(
            &src,
            &skills.join("alpha"),
            tmp.path(),
            Duration::ZERO,
            &mut BTreeSet::new(),
        );

        assert_eq!(
            fs::read_to_string(&notes).expect("install must preserve user-owned files"),
            "user-owned notes"
        );
        assert!(!user_dir.join(MARKER_FILE).exists());
    }

    #[cfg(unix)]
    #[test]
    fn failed_install_leaves_the_slot_free() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("SKILL.md"), "skill").unwrap();
        let notes = src.join("notes.txt");
        fs::write(&notes, "notes").unwrap();
        fs::set_permissions(&notes, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read(&notes).is_ok() {
            return;
        }
        let skills = tmp.path().join("skills");
        let dst = skills.join("alpha");

        assert!(
            sync_skill_dir(&src, &dst, tmp.path(), Duration::ZERO, &mut BTreeSet::new()).is_err()
        );
        assert_eq!(fs::read_dir(&skills).unwrap().count(), 0);

        fs::set_permissions(&notes, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            sync_skill_dir(&src, &dst, tmp.path(), Duration::ZERO, &mut BTreeSet::new()).unwrap()
        );
        assert!(dst.join(MARKER_FILE).exists());
        assert_eq!(fs::read_to_string(dst.join("notes.txt")).unwrap(), "notes");
    }
}
