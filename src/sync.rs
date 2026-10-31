//! Sync command: `symposium sync`.
//!
//! Scans workspace dependencies, finds applicable skills from plugin sources,
//! installs them into each configured agent's skill directory, and cleans up
//! stale skills by looking for a `.symposium` marker file in each skill dir.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use symposium_install::UpdateLevel;

use crate::agents::Agent;
use crate::config::Symposium;
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

/// Private result of the one-pass resolution phase.
///
/// This value may retain raw package provenance and filesystem paths. It has
/// neither `Debug` nor serialization; the next layer projects it into the
/// telemetry-safe structured report.
struct ResolvedSync {
    packages: Vec<crate::pm::SourcedPackage>,
    plugins: usize,
    plugin_evidence: Vec<crate::telemetry::ResolutionEvidence>,
    skills: Vec<crate::skills::SkillWithGroupContext>,
    mcp_servers: Vec<sacp::schema::McpServer>,
    complete: bool,
}

/// One deduplicated skill ready for installation.
///
/// The first route supplies the safe witness. Later duplicate routes union
/// only their package contributors so package extension matching remains
/// complete without changing which concrete route selected the installation.
struct ResolvedSkillInstall {
    name: String,
    origin_hash: String,
    source: PathBuf,
    evidence: Option<crate::telemetry::ResolutionEvidence>,
}

struct DeduplicatedSkills {
    skills: Vec<ResolvedSkillInstall>,
    name_counts: std::collections::BTreeMap<String, usize>,
}

/// Observable result of synchronizing one managed skill directory.
///
/// Declaration order is the fold precedence used when several agents share
/// one destination: a result that changed content outranks a later no-op.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SkillSyncStatus {
    Debounced,
    Unchanged,
    Updated,
    Installed,
}

impl SkillSyncStatus {
    fn changed(self) -> bool {
        matches!(self, Self::Installed | Self::Updated)
    }
}

/// One managed installation retained for cleanup and the attribution index.
///
/// This type deliberately has no `Debug`: `destination` and the surrounding
/// layout contain project-local paths.
struct InstalledSkill {
    slot: String,
    skill_index: usize,
    /// `None` retains an existing managed slot after a failed refresh without
    /// counting the failed attempt as an installation or update.
    status: Option<SkillSyncStatus>,
}

/// Private filesystem result kept separate from the telemetry-safe report.
///
/// Cleanup scans every known agent parent, while indexing must touch only
/// parents that actually hold managed content. Keeping both sets prevents a
/// Codex-only sync from creating an empty Claude index.
struct InstalledLayout {
    skills: BTreeMap<PathBuf, InstalledSkill>,
    cleanup_parents: BTreeSet<PathBuf>,
    managed_parents: BTreeSet<PathBuf>,
}

impl InstalledLayout {
    fn new() -> Self {
        Self {
            skills: BTreeMap::new(),
            cleanup_parents: BTreeSet::new(),
            managed_parents: BTreeSet::new(),
        }
    }

    fn record(
        &mut self,
        destination: PathBuf,
        slot: String,
        skill_index: usize,
        status: SkillSyncStatus,
    ) {
        self.record_present(destination, slot, skill_index, Some(status));
    }

    fn preserve(&mut self, destination: PathBuf, slot: String, skill_index: usize) {
        self.record_present(destination, slot, skill_index, None);
    }

    fn record_present(
        &mut self,
        destination: PathBuf,
        slot: String,
        skill_index: usize,
        status: Option<SkillSyncStatus>,
    ) {
        let parent = destination
            .parent()
            .expect("BUG: an installed skill directory must have a parent")
            .to_path_buf();
        self.managed_parents.insert(parent);

        self.skills
            .entry(destination)
            .and_modify(|installed| {
                debug_assert_eq!(
                    installed.skill_index, skill_index,
                    "one installed slot must identify one resolved skill"
                );
                debug_assert_eq!(
                    installed.slot, slot,
                    "one installed directory must retain one slot name"
                );
                // Shared parents are visited once per configured agent. A
                // fresh install therefore commonly appears as Installed and
                // then Debounced; preserve the strongest result for counts
                // and for the next attribution-index slice.
                if let Some(status) = status {
                    installed.status = Some(installed.status.map_or(status, |old| old.max(status)));
                }
            })
            .or_insert(InstalledSkill {
                slot,
                skill_index,
                status,
            });
    }
}

/// Complete internal result; only its safe report leaves this module.
struct CompletedSync {
    report: crate::telemetry::SyncReport,
    layout: InstalledLayout,
}

/// Hard sync failure paired with the safe progress report accumulated so far.
#[derive(Debug)]
pub(crate) struct SyncFailure {
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "resolution recording consumes failed reports in the activation stack"
        )
    )]
    report: Box<crate::telemetry::SyncReport>,
    source: anyhow::Error,
}

impl fmt::Display for SyncFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.source.fmt(formatter)
    }
}

impl SyncFailure {
    /// Return the safe report accumulated before the hard failure.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "resolution recording consumes failed reports in the activation stack"
        )
    )]
    pub(crate) fn report(&self) -> &crate::telemetry::SyncReport {
        &self.report
    }

    /// Recover the original command error for the compatibility entry point.
    fn into_source(self) -> anyhow::Error {
        self.source
    }
}

impl std::error::Error for SyncFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

/// Mutable report state for one operation.
///
/// This value has no `Debug`: before final projection it owns raw package
/// provenance and resolved skill paths.
struct SyncProgress {
    started: Instant,
    retain_telemetry: bool,
    outcome: crate::telemetry::SyncOutcome,
    packages: Vec<crate::pm::SourcedPackage>,
    plugins: usize,
    plugin_evidence: Vec<crate::telemetry::ResolutionEvidence>,
    skills: Vec<ResolvedSkillInstall>,
    name_counts: BTreeMap<String, usize>,
    layout: InstalledLayout,
    reaped: usize,
}

impl SyncProgress {
    fn new(retain_telemetry: bool) -> Self {
        Self {
            started: Instant::now(),
            retain_telemetry,
            outcome: crate::telemetry::SyncOutcome::Ok,
            packages: Vec::new(),
            plugins: 0,
            plugin_evidence: Vec::new(),
            skills: Vec::new(),
            name_counts: BTreeMap::new(),
            layout: InstalledLayout::new(),
            reaped: 0,
        }
    }

    fn mark_partial(&mut self) {
        promote_to_partial(&mut self.outcome);
    }

    fn set_resolution(
        &mut self,
        packages: Vec<crate::pm::SourcedPackage>,
        plugins: usize,
        plugin_evidence: Vec<crate::telemetry::ResolutionEvidence>,
        deduplicated: DeduplicatedSkills,
        complete: bool,
    ) {
        self.packages = packages;
        self.plugins = plugins;
        self.plugin_evidence = plugin_evidence;
        self.skills = deduplicated.skills;
        self.name_counts = deduplicated.name_counts;
        if !complete {
            self.mark_partial();
        }
    }

    fn report(&self, outcome: crate::telemetry::SyncOutcome) -> crate::telemetry::SyncReport {
        let evidence = self.resolved_extension_evidence();

        let installed = self
            .layout
            .skills
            .values()
            .filter(|skill| skill.status == Some(SkillSyncStatus::Installed))
            .count();
        let updated = self
            .layout
            .skills
            .values()
            .filter(|skill| skill.status == Some(SkillSyncStatus::Updated))
            .count();

        crate::telemetry::SyncReport::project(crate::telemetry::SyncReportInput {
            outcome,
            duration: self.started.elapsed(),
            packages: if self.retain_telemetry {
                &self.packages
            } else {
                &[]
            },
            extensions: evidence,
            plugins: self.plugins,
            skills: self.skills.len(),
            installed,
            updated,
            reaped: self.reaped,
        })
    }

    /// Return evidence for every resolved plugin and skill.
    ///
    /// Filesystem installation status affects the change counters and the
    /// private attribution index, not whether an extension resolved.
    fn resolved_extension_evidence(&self) -> Vec<&crate::telemetry::ResolutionEvidence> {
        if !self.retain_telemetry {
            return Vec::new();
        }

        self.plugin_evidence
            .iter()
            .chain(
                self.skills
                    .iter()
                    .filter_map(|skill| skill.evidence.as_ref()),
            )
            .collect()
    }

    fn finish(self) -> CompletedSync {
        let report = self.report(self.outcome);
        CompletedSync {
            report,
            layout: self.layout,
        }
    }

    fn fail(self, source: anyhow::Error) -> SyncFailure {
        let report = self.report(crate::telemetry::SyncOutcome::Error);
        // Hard failures are exceptional; boxing their report keeps every
        // `Result<_, SyncFailure>` compact without adding work to success.
        SyncFailure {
            report: Box::new(report),
            source,
        }
    }
}

fn promote_to_partial(outcome: &mut crate::telemetry::SyncOutcome) {
    if *outcome == crate::telemetry::SyncOutcome::Ok {
        *outcome = crate::telemetry::SyncOutcome::Partial;
    }
}

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
    fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir()) && path.join(MARKER_FILE).exists()
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
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    let (entries, skipped) = walk_tree(src)?;
    for message in skipped {
        tracing::info!(report = %crate::report::ReportEvent::Warning { message });
    }
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

/// Collect `dir`'s tree for comparison: every directory and regular file under
/// it, as a path relative to `dir` paired with the file's contents (`None` for
/// a directory). Symlinks resolve exactly as [`copy_dir_recursive`] resolves
/// them, so the source of an installed skill and its destination compare equal.
///
/// Skips the `.symposium` marker and `.gitignore`, which are managed metadata
/// rather than skill content. Entries the walk refuses are dropped silently:
/// the copy this comparison guards reports them, and reporting them here too
/// would say the same thing twice per sync.
fn collect_dir_contents(dir: &Path) -> Result<Vec<(PathBuf, Option<Vec<u8>>)>> {
    let (entries, _skipped) = walk_tree(dir)?;
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
    Ok(out)
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
/// 3. Otherwise builds a complete, marked copy beside `dest_dir` and swaps it
///    in, so a failed copy leaves the slot as it was.
///
/// Returns the exact filesystem outcome so the structured sync report can
/// distinguish new installs, updates, content-identical checks, and debounce
/// skips without inspecting paths or mtimes a second time.
fn sync_skill_dir(
    source_dir: &Path,
    dest_dir: &Path,
    project_root: &Path,
    debounce: Duration,
) -> Result<SkillSyncStatus> {
    if dest_dir == source_dir {
        return Ok(SkillSyncStatus::Unchanged);
    }

    // If the destination doesn't exist yet, do a fresh install.
    if !slot_taken(dest_dir) {
        install_skill_dir(source_dir, dest_dir, project_root)?;
        return Ok(SkillSyncStatus::Installed);
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
        return Ok(SkillSyncStatus::Debounced);
    }

    // Compare content (excluding managed metadata).
    if !dir_contents_differ(source_dir, dest_dir)? {
        // Content is identical — just touch the marker to reset debounce.
        touch_marker(&marker_path)?;
        return Ok(SkillSyncStatus::Unchanged);
    }

    // Content changed: replace entirely.
    install_skill_dir(source_dir, dest_dir, project_root)?;
    Ok(SkillSyncStatus::Updated)
}

fn install_skill_dir(source_dir: &Path, dest_dir: &Path, project_root: &Path) -> Result<()> {
    let parent = dest_dir.parent().expect("skill dir must have parent");
    create_managed_dir_all(parent, project_root)?;
    let mut staging = tempfile::Builder::new()
        .prefix(".symposium-tmp-")
        .tempdir_in(parent)
        .with_context(|| format!("create staging dir in {}", parent.display()))?;
    mark_generated_skill_directory(staging.path())?;
    copy_dir_recursive(source_dir, staging.path())?;
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
) -> ResolvedCustomPredicates {
    use crate::predicate::ResolvedPredicateEntry;

    let mut entries = std::collections::HashMap::new();
    let mut complete = true;

    for (name, resolved) in registry.custom_predicates.iter() {
        let plugin = &registry.plugins[resolved.plugin_index];
        let Some(install) = plugin.manifest.get_installation(&resolved.command) else {
            tracing::warn!(
                predicate = name,
                command = &resolved.command,
                "custom predicate references unknown installation"
            );
            complete = false;
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
                    complete = false;
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
                    complete = false;
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

    ResolvedCustomPredicates { entries, complete }
}

struct ResolvedCustomPredicates {
    entries: std::collections::HashMap<String, crate::predicate::ResolvedPredicateEntry>,
    complete: bool,
}

async fn resolve_sync(
    sym: &Symposium,
    deps: &Arc<WorkspaceDeps>,
    registry: &plugins::PluginRegistry,
    project_root: &Path,
    custom_entries: std::collections::HashMap<String, crate::predicate::ResolvedPredicateEntry>,
    predicate_cache_path: &Path,
    update: UpdateLevel,
) -> ResolvedSync {
    let pms = sym.package_managers(deps);
    let (packages, dependencies_complete) = match pms.list_deps().await {
        Ok(packages) => (packages, true),
        Err(error) => {
            tracing::debug!(%error, "failed to list sourced dependencies for sync");
            (Vec::new(), false)
        }
    };
    let used = crate::discovery::used_plugins(sym, &pms, &registry.plugins, project_root);
    let mut ctx = crate::predicate::SourcedPredicateContext::with_custom_predicates(
        &packages,
        custom_entries,
    )
    .with_used(&used)
    .with_disk_cache(predicate_cache_path);
    let retain_evidence = crate::telemetry::is_enabled(sym);

    let active = plugins::resolve_active_plugins(
        sym,
        registry,
        &pms,
        Some(project_root),
        &mut ctx,
        update,
        retain_evidence,
    )
    .await;
    let resolved_skills =
        skills::resolve_skills(sym, &active.plugins, &mut ctx, update, retain_evidence).await;

    let mcp_servers = collect_mcp_servers(&active.plugins, &mut ctx);
    let plugins = active.plugins.len();
    let plugin_evidence = active
        .plugins
        .into_iter()
        .filter_map(|plugin| plugin.evidence)
        .collect();

    if let Err(error) = ctx.persist_disk_cache(predicate_cache_path) {
        // Cache persistence does not make resolution partial: it affects only
        // whether a later run may reuse a boolean result.
        tracing::warn!(
            path = %predicate_cache_path.display(),
            error = %error,
            "failed to persist predicate cache"
        );
    }

    ResolvedSync {
        packages,
        plugins,
        plugin_evidence,
        skills: resolved_skills.skills,
        mcp_servers,
        complete: dependencies_complete && active.complete && resolved_skills.complete,
    }
}

/// Collect nested MCP servers without re-evaluating each plugin's selection
/// gate.
///
/// The plugin walk has already admitted every item in `active`. Re-stamping
/// its provenance here is still required because `workspace-member()` inside
/// an MCP predicate belongs to that plugin, not to the plugin visited before
/// it.
fn collect_mcp_servers<'a>(
    active: &[crate::plugins::ResolvedPlugin],
    ctx: &mut crate::predicate::SourcedPredicateContext<'a>,
) -> Vec<sacp::schema::McpServer> {
    let mut servers = Vec::new();
    for resolved in active {
        ctx.set_workspace_member(resolved.plugin.is_workspace_member());
        servers.extend(resolved.plugin.manifest.applicable_mcp_servers(ctx));
    }
    servers
}

fn deduplicate_skills(applicable: Vec<crate::skills::SkillWithGroupContext>) -> DeduplicatedSkills {
    let mut by_key: std::collections::BTreeMap<(String, String), usize> =
        std::collections::BTreeMap::new();
    let mut skills: Vec<ResolvedSkillInstall> = Vec::new();
    let mut name_counts = std::collections::BTreeMap::new();

    for entry in applicable {
        let name = entry.skill.name().to_owned();
        let key = (name.clone(), entry.origin_hash.clone());
        if let Some(&index) = by_key.get(&key) {
            if let (Some(existing), Some(duplicate)) =
                (&mut skills[index].evidence, &entry.evidence)
            {
                existing.union_contributors(duplicate);
            }
            continue;
        }

        by_key.insert(key, skills.len());
        *name_counts.entry(name.clone()).or_default() += 1;
        skills.push(ResolvedSkillInstall {
            name,
            origin_hash: entry.origin_hash,
            source: entry.skill.path,
            evidence: entry.evidence,
        });
    }

    DeduplicatedSkills {
        skills,
        name_counts,
    }
}

/// Run the full sync while preserving the established public `Result<()>` API.
pub async fn sync(sym: &Symposium, deps: &Arc<WorkspaceDeps>, update: UpdateLevel) -> Result<()> {
    sync_with_report(sym, deps, update)
        .await
        .map(drop)
        .map_err(SyncFailure::into_source)
}

/// Run a full sync and return its telemetry-safe structured report.
pub(crate) async fn sync_with_report(
    sym: &Symposium,
    deps: &Arc<WorkspaceDeps>,
    update: UpdateLevel,
) -> std::result::Result<crate::telemetry::SyncReport, SyncFailure> {
    let CompletedSync { report, layout } = run_sync(sym, deps, update).await?;
    // The following stack consumes the private layout to maintain attribution
    // indexes. It is deliberately dropped before the safe report leaves sync.
    drop(layout);
    Ok(report)
}

/// Resolve, install, and clean up while retaining the private installed layout.
async fn run_sync(
    sym: &Symposium,
    deps: &Arc<WorkspaceDeps>,
    update: UpdateLevel,
) -> std::result::Result<CompletedSync, SyncFailure> {
    let mut progress = SyncProgress::new(crate::telemetry::is_enabled(sym));
    if let Some(error) = sym.config_error() {
        return Err(progress.fail(anyhow::anyhow!(
            "not syncing: {} could not be read; fix or remove it, then run the command again\n{error}",
            display_path(&sym.config_dir().join("config.toml")),
        )));
    }
    let out = &Output::quiet();
    let loaded = match deps.require() {
        Ok(loaded) => loaded,
        Err(error) => return Err(progress.fail(error)),
    };
    let project_root = loaded.root.clone();
    let workspace_count = loaded.crates.len();
    let loaded = loaded.clone();
    let debounce = Duration::from_secs(sym.config.sync_debounce_secs);
    tracing::debug!(root = %project_root.display(), "resolved workspace root");

    // Load plugin registry (registry sources + workspace plugins)
    let registry = plugins::load_registry_with_workspace(sym, Some(&loaded)).await;
    if !registry.warnings.is_empty() {
        progress.mark_partial();
    }

    for warning in &registry.warnings {
        tracing::info!(
            report = %crate::report::ReportEvent::Warning {
                message: format!("skipping {}: {}", display_path(&warning.path), warning.message),
            },
        );
    }

    tracing::info!(
        report = %crate::report::ReportEvent::Info {
            message: format!("scanning {workspace_count} workspace dependencies"),
        },
    );

    // Resolve custom predicate installations.
    let custom = resolve_custom_predicate_entries(sym, &registry, update).await;
    if !custom.complete {
        progress.mark_partial();
    }

    // Resolve sourced packages, plugins, skills, and MCP servers in one pass.
    // Raw provenance stays inside the private result.
    let predicate_cache_path =
        crate::predicate_cache::PredicateCache::path_for_workspace(sym.cache_dir(), &project_root);
    let resolved = resolve_sync(
        sym,
        deps,
        &registry,
        &project_root,
        custom.entries,
        &predicate_cache_path,
        update,
    )
    .await;
    let ResolvedSync {
        packages,
        plugins,
        plugin_evidence,
        skills,
        mcp_servers,
        complete,
    } = resolved;
    // The first concrete route survives deduplication; later routes contribute
    // only their package set for package-level extension matching.
    let deduplicated = deduplicate_skills(skills);
    progress.set_resolution(packages, plugins, plugin_evidence, deduplicated, complete);

    // Sync each configured agent
    let agent_names: Vec<String> = sym.config.agents.iter().map(|a| a.name.clone()).collect();

    tracing::info!(
        workspace_deps = workspace_count,
        agents = agent_names.len(),
        skills = progress.skills.len(),
        "sync started"
    );

    let _lock = match lock_sync(sym) {
        Ok(lock) => lock,
        Err(error) => return Err(progress.fail(error)),
    };

    if agent_names.is_empty() {
        tracing::info!(
            report = %crate::report::ReportEvent::Info {
                message: "no agents configured, run `cargo agents init` to add one".into(),
            },
        );
    }

    let skip_cleanup = !registry.unreadable_registries.is_empty();
    for agent_name in &agent_names {
        let Ok(agent) = Agent::from_config_name(agent_name) else {
            progress.mark_partial();
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
            progress.mark_partial();
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
                progress.mark_partial();
                tracing::info!(
                    report = %crate::report::ReportEvent::Warning {
                        message: format!("failed to register MCP servers for {agent_name}: {e:#}"),
                    },
                );
            }
        }

        let skills = &progress.skills;
        let name_counts = &progress.name_counts;
        let layout = &mut progress.layout;
        let outcome = &mut progress.outcome;
        for (skill_index, skill) in skills.iter().enumerate() {
            let skill_name = &skill.name;
            let origin_hash = &skill.origin_hash;
            let skill_source = &skill.source;
            // `skill_source` is the path to the SKILL.md file; the skill
            // directory is its parent.
            let source_dir = match skill_source.parent() {
                Some(p) => p,
                None => {
                    out.warn(format!(
                        "skill {skill_name}: cannot determine source directory"
                    ));
                    promote_to_partial(outcome);
                    continue;
                }
            };

            // The skill's source already sits at this agent's install slot
            // (a workspace `.agents/skills/` skill, on an agent that reads
            // that same directory) — it is in place as user content, not
            // something to copy.
            let plain_dir = agent.project_skill_dir(&project_root, skill_name);
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
                promote_to_partial(outcome);
                continue;
            }

            match sync_skill_dir(source_dir, &dest_dir, &project_root, debounce) {
                Ok(status) => {
                    layout.record(dest_dir.clone(), dir_name.clone(), skill_index, status);
                    if status.changed() {
                        tracing::info!(
                            report = %crate::report::ReportEvent::SkillInstalled {
                                skill: dir_name.clone(),
                                agent: agent_name.clone(),
                                dest: display_path(&dest_dir),
                            },
                        );
                    }
                }
                Err(e) => {
                    if is_managed_skill_dir(&dest_dir) {
                        layout.preserve(dest_dir.clone(), dir_name.clone(), skill_index);
                    }
                    promote_to_partial(outcome);
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
    let installed: BTreeSet<PathBuf> = progress
        .layout
        .skills
        .keys()
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
        progress.layout.cleanup_parents.insert(parent.clone());
        let entries = match fs::read_dir(&parent) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                progress.mark_partial();
                tracing::info!(
                    report = %crate::report::ReportEvent::Warning {
                        message: format!(
                            "failed to scan skills directory {}: {error}",
                            display_path(&parent),
                        ),
                    },
                );
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    progress.mark_partial();
                    tracing::info!(
                        report = %crate::report::ReportEvent::Warning {
                            message: format!(
                                "failed to inspect an entry in {}: {error}",
                                display_path(&parent),
                            ),
                        },
                    );
                    continue;
                }
            };
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    progress.mark_partial();
                    tracing::info!(
                        report = %crate::report::ReportEvent::Warning {
                            message: format!(
                                "failed to inspect {}: {error}",
                                display_path(&entry.path()),
                            ),
                        },
                    );
                    continue;
                }
            };
            if !file_type.is_dir() {
                continue;
            }
            let path = entry.path();
            if installed.contains(&path) || !is_managed_skill_dir(&path) {
                continue;
            }
            match fs::remove_dir_all(&path) {
                Ok(()) => {
                    tracing::info!(
                        report = %crate::report::ReportEvent::SkillRemoved {
                            path: display_path(&path),
                        },
                    );
                    progress.reaped = progress
                        .reaped
                        .checked_add(1)
                        .expect("BUG: reaped directory count must fit in usize");
                }
                Err(e) => {
                    progress.mark_partial();
                    tracing::info!(
                        report = %crate::report::ReportEvent::Warning {
                            message: format!("failed to remove stale {}: {e}", display_path(&path)),
                        },
                    );
                }
            }
        }
    }
    for name in &registry.unreadable_registries {
        tracing::info!(
            report = %crate::report::ReportEvent::Warning {
                message: format!(
                    "registry `{name}` could not be fully read; skipped cleanup of installed skills"
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

    if progress.skills.is_empty() {
        tracing::info!(
            report = %crate::report::ReportEvent::Info {
                message: "no applicable skills found for workspace dependencies".into(),
            },
        );
    }

    Ok(progress.finish())
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

    fn write_skill(directory: &Path, body: &str) {
        fs::create_dir_all(directory).unwrap();
        fs::write(directory.join("SKILL.md"), body).unwrap();
    }

    #[test]
    fn skill_sync_reports_each_filesystem_outcome() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let destination = temp.path().join("project/.agents/skills/example");
        write_skill(&source, "first");

        assert_eq!(
            sync_skill_dir(&source, &destination, temp.path(), Duration::ZERO).unwrap(),
            SkillSyncStatus::Installed
        );
        assert_eq!(
            sync_skill_dir(&source, &destination, temp.path(), Duration::MAX).unwrap(),
            SkillSyncStatus::Debounced
        );
        assert_eq!(
            sync_skill_dir(&source, &destination, temp.path(), Duration::ZERO).unwrap(),
            SkillSyncStatus::Unchanged
        );

        fs::write(source.join("SKILL.md"), "second").unwrap();
        assert_eq!(
            sync_skill_dir(&source, &destination, temp.path(), Duration::ZERO).unwrap(),
            SkillSyncStatus::Updated
        );
        assert_eq!(
            fs::read_to_string(destination.join("SKILL.md")).unwrap(),
            "second"
        );
    }

    #[test]
    fn shared_parent_keeps_the_strongest_slot_status() {
        let destination = PathBuf::from("project/.agents/skills/example");
        let parent = destination.parent().unwrap().to_path_buf();
        let mut layout = InstalledLayout::new();

        layout.record(
            destination.clone(),
            "example".to_owned(),
            2,
            SkillSyncStatus::Installed,
        );
        layout.record(
            destination.clone(),
            "example".to_owned(),
            2,
            SkillSyncStatus::Debounced,
        );

        assert_eq!(layout.skills.len(), 1);
        assert_eq!(
            layout.skills[&destination].status,
            Some(SkillSyncStatus::Installed),
            "a later agent sharing the parent must not hide this run's install"
        );
        assert_eq!(layout.managed_parents, BTreeSet::from([parent]));
    }

    #[test]
    fn failed_refresh_preserves_the_slot_without_a_success_status() {
        let destination = PathBuf::from("project/.agents/skills/example");
        let parent = destination.parent().unwrap().to_path_buf();
        let mut layout = InstalledLayout::new();

        layout.preserve(destination.clone(), "example".to_owned(), 2);

        assert_eq!(layout.skills[&destination].status, None);
        assert_eq!(layout.managed_parents, BTreeSet::from([parent]));
    }

    #[test]
    fn hard_failure_preserves_the_safe_progress_report() {
        use std::error::Error as _;

        let failure = SyncProgress::new(true).fail(anyhow::anyhow!("sentinel failure"));

        assert_eq!(
            failure.report().outcome(),
            crate::telemetry::SyncOutcome::Error
        );
        assert_eq!(failure.to_string(), "sentinel failure");
        assert_eq!(failure.source().unwrap().to_string(), "sentinel failure");

        let source = SyncProgress::new(true)
            .fail(anyhow::anyhow!("original failure"))
            .into_source();
        assert_eq!(format!("{source:#}"), "original failure");
    }

    #[test]
    fn resolved_skill_evidence_does_not_depend_on_installation() {
        use crate::pm::{CRATES_IO_REGISTRY_URL, PackageProvenance};
        use crate::predicate::{PredicateSet, SourcedPredicateContext};

        let mut context = SourcedPredicateContext::new(&[]);
        let route = crate::telemetry::evaluate_resolution_predicates(
            &PredicateSet::default(),
            &mut context,
            &mut crate::telemetry::OptionalWitnessMode::new(true),
        )
        .unwrap();
        let evidence = crate::telemetry::skill_resolution_evidence(
            route,
            None,
            None,
            &PackageProvenance::Registry {
                url: CRATES_IO_REGISTRY_URL.to_owned(),
            },
            "resolved-skill",
        )
        .unwrap();
        let mut progress = SyncProgress::new(true);
        progress.skills.push(ResolvedSkillInstall {
            name: "resolved-skill".to_owned(),
            origin_hash: "origin".to_owned(),
            source: PathBuf::from("source/SKILL.md"),
            evidence: Some(evidence),
        });

        assert!(progress.layout.skills.is_empty());
        assert_eq!(progress.resolved_extension_evidence().len(), 1);
    }

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

        copy_dir_recursive(&src, &dst).unwrap();

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
        copy_dir_recursive(&src, &dst).unwrap();

        fs::write(tmp.path().join("outside/lib.txt"), "edited").unwrap();

        assert!(dir_contents_differ(&src, &dst).unwrap());
    }

    #[test]
    fn a_plain_tree_copies_and_compares_equal() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("SKILL.md"), "skill").unwrap();
        fs::write(src.join("sub/a.txt"), "a").unwrap();
        let dst = tmp.path().join("dst");

        copy_dir_recursive(&src, &dst).unwrap();

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

        copy_dir_recursive(&src, &dst).unwrap();
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
        let _ = sync_skill_dir(&src, &skills.join("alpha"), tmp.path(), Duration::ZERO);

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

        assert!(sync_skill_dir(&src, &dst, tmp.path(), Duration::ZERO).is_err());
        assert_eq!(fs::read_dir(&skills).unwrap().count(), 0);

        fs::set_permissions(&notes, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            sync_skill_dir(&src, &dst, tmp.path(), Duration::ZERO).unwrap(),
            SkillSyncStatus::Installed,
        );
        assert!(dst.join(MARKER_FILE).exists());
        assert_eq!(fs::read_to_string(dst.join("notes.txt")).unwrap(), "notes");
    }

    #[test]
    fn deduplicated_routes_union_their_package_contributors() {
        use crate::pm::{CARGO_PM, CRATES_IO_REGISTRY_URL, PackageId, PackageProvenance};
        use crate::predicate::{Predicate, PredicateSet, SourcedPredicateContext};

        let packages = ["first", "second"].map(|name| {
            crate::pm::SourcedPackage::new(
                PackageId::new(CARGO_PM, name, "1.0.0"),
                PackageProvenance::Registry {
                    url: CRATES_IO_REGISTRY_URL.to_owned(),
                },
            )
        });
        let evidence = packages.iter().map(|package| {
            let mut context = SourcedPredicateContext::new(&packages);
            crate::telemetry::evaluate_resolution_predicates(
                &PredicateSet {
                    predicates: vec![Predicate::DependsOn(package.id.name.clone(), None)],
                },
                &mut context,
                &mut crate::telemetry::OptionalWitnessMode::new(true),
            )
            .unwrap()
            .unwrap()
        });
        let source = PathBuf::from("shared/SKILL.md");
        let skills = evidence
            .map(|evidence| {
                let mut frontmatter = std::collections::BTreeMap::new();
                frontmatter.insert("name".to_owned(), "shared".to_owned());
                crate::skills::SkillWithGroupContext {
                    skill: crate::skills::Skill {
                        frontmatter,
                        predicates: PredicateSet::default(),
                        path: source.clone(),
                    },
                    origin_hash: "same-origin".to_owned(),
                    evidence: Some(evidence),
                }
            })
            .collect();

        let deduplicated = deduplicate_skills(skills);

        assert_eq!(deduplicated.skills.len(), 1);
        let contributors = deduplicated.skills[0]
            .evidence
            .as_ref()
            .unwrap()
            .contributors();
        assert_eq!(contributors.len(), 2);
        assert!(
            packages
                .iter()
                .all(|package| contributors.contains(package))
        );
    }

    #[test]
    fn evidence_retention_does_not_change_the_installed_slot_or_status() {
        use crate::pm::{CARGO_PM, CRATES_IO_REGISTRY_URL, PackageId, PackageProvenance};
        use crate::predicate::{Predicate, PredicateSet, SourcedPredicateContext};

        let temp = tempfile::tempdir().unwrap();
        let source_dir = temp.path().join("source");
        write_skill(&source_dir, "skill");
        let source = source_dir.join("SKILL.md");
        let package = crate::pm::SourcedPackage::new(
            PackageId::new(CARGO_PM, "example-runtime", "1.0.0"),
            PackageProvenance::Registry {
                url: CRATES_IO_REGISTRY_URL.to_owned(),
            },
        );
        let packages = vec![package.clone()];
        let mut context = SourcedPredicateContext::new(&packages);
        let evidence = crate::telemetry::evaluate_resolution_predicates(
            &PredicateSet {
                predicates: vec![Predicate::DependsOn(package.id.name.clone(), None)],
            },
            &mut context,
            &mut crate::telemetry::OptionalWitnessMode::new(true),
        )
        .unwrap()
        .unwrap();
        let entry = |evidence| crate::skills::SkillWithGroupContext {
            skill: crate::skills::Skill {
                frontmatter: BTreeMap::from([("name".to_owned(), "example".to_owned())]),
                predicates: PredicateSet::default(),
                path: source.clone(),
            },
            origin_hash: "same-origin".to_owned(),
            evidence,
        };

        let without_evidence = deduplicate_skills(vec![entry(None)]);
        let with_evidence = deduplicate_skills(vec![entry(Some(evidence))]);
        let plain = &without_evidence.skills[0];
        let retained = &with_evidence.skills[0];
        assert_eq!(
            (&plain.name, &plain.origin_hash, &plain.source),
            (&retained.name, &retained.origin_hash, &retained.source)
        );
        assert_eq!(without_evidence.name_counts, with_evidence.name_counts);

        let plain_destination = temp.path().join("plain/.agents/skills/example");
        let retained_destination = temp.path().join("retained/.agents/skills/example");
        let plain_status = sync_skill_dir(
            plain.source.parent().unwrap(),
            &plain_destination,
            temp.path(),
            Duration::ZERO,
        )
        .unwrap();
        let retained_status = sync_skill_dir(
            retained.source.parent().unwrap(),
            &retained_destination,
            temp.path(),
            Duration::ZERO,
        )
        .unwrap();

        assert_eq!(plain_status, retained_status);
        assert_eq!(plain_status, SkillSyncStatus::Installed);
    }

    #[test]
    fn mcp_predicates_use_their_own_plugins_provenance() {
        use crate::plugins::{Plugin, PluginManifest, PluginMcpServer, ResolvedPlugin};
        use crate::pm::{ANY_VERSION, PackageId, PackageProvenance, SourcedPackage};
        use crate::predicate::{Predicate, PredicateSet, SourcedPredicateContext};

        fn plugin(name: &str, provenance: PackageProvenance) -> Plugin {
            Plugin {
                manifest: PluginManifest {
                    name: name.to_owned(),
                    predicates: PredicateSet::default(),
                    installations: Vec::new(),
                    hooks: Vec::new(),
                    skills: Vec::new(),
                    mcp_servers: Vec::new(),
                    subcommands: std::collections::BTreeMap::new(),
                    custom_predicates: Vec::new(),
                    chained: Vec::new(),
                    requires_use: false,
                },
                package: SourcedPackage::new(PackageId::new("test", name, ANY_VERSION), provenance),
            }
        }

        let workspace = ResolvedPlugin {
            plugin: plugin("workspace", PackageProvenance::Workspace),
            evidence: None,
        };
        let server = toml::from_str(
            r#"
                name = "private-server"
                command = "server"
                args = []
                env = []
            "#,
        )
        .unwrap();
        let mut private = plugin("private", PackageProvenance::Path);
        private.manifest.mcp_servers.push(PluginMcpServer {
            predicates: PredicateSet {
                predicates: vec![Predicate::WorkspaceMember],
            },
            server,
        });
        let private = ResolvedPlugin {
            plugin: private,
            evidence: None,
        };
        let packages = Vec::new();
        let mut context = SourcedPredicateContext::new(&packages);

        let servers = collect_mcp_servers(&[workspace, private], &mut context);

        assert!(servers.is_empty());
    }
}
