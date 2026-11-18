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
use crate::compile;
use crate::config::{INSTALLED_PLUGINS_SUBDIR, Symposium};
use crate::dir_walk::{self, EntryKind};
use crate::output::{Output, display_path};
use crate::plugins;
use crate::pm::Workspace;
use crate::skills;
use std::sync::Arc;

/// Marker file written into every skill directory symposium installs.
///
/// Cleanup walks each agent's skills parent dir and removes any subdir
/// containing this marker that isn't in the freshly-installed set, leaving
/// user-managed skill directories (which lack the marker) untouched.
pub(crate) use crate::telemetry::INSTALLATION_MARKER_FILE as MARKER_FILE;
const PLUGIN_DATA_DIR: &str = "plugin-data";

/// Private result of source-sensitive sync resolution.
///
/// This value deliberately has neither `Debug` nor serialization. Its admitted
/// packages and route evidence remain private working state until
/// [`SyncProgress`] projects a bounded [`telemetry`](crate::telemetry) report.
struct ResolvedSync {
    dependencies: crate::pm::AdmittedDependencies,
    active: Vec<crate::plugins::ResolvedPlugin>,
    revoked: Vec<crate::plugins::Plugin>,
    skills: Vec<crate::skills::SkillWithGroupContext>,
    mcp_servers: Vec<sacp::schema::McpServer>,
    global: Vec<crate::compile::PluginSkills>,
    project: Vec<crate::compile::PluginSkills>,
    complete: bool,
}

/// Inputs whose lifetime is limited to one sync-resolution pass.
struct SyncResolutionInputs<'a> {
    registry: &'a plugins::PluginRegistry,
    project_root: &'a Path,
    custom_predicates: std::collections::HashMap<String, crate::predicate::ResolvedPredicateEntry>,
    predicate_cache_path: &'a Path,
    update: UpdateLevel,
}

/// One deduplicated skill ready for installation.
///
/// The first concrete route supplies the witness. Later routes with the same
/// install identity contribute only their exact package handles.
struct ResolvedSkillInstall {
    name: String,
    origin_hash: String,
    source: PathBuf,
    package_root: Option<PathBuf>,
    evidence: Option<crate::telemetry::ResolutionEvidence>,
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

/// One managed installation retained for cleanup and attribution indexing.
///
/// This type deliberately has no `Debug`: its surrounding layout contains
/// project-local paths.
struct InstalledSkill {
    slot: String,
    skill_index: usize,
    /// `None` preserves a managed slot after a failed refresh without counting
    /// the failed attempt as an installation or update.
    status: Option<SkillSyncStatus>,
}

/// Private filesystem result kept separate from the telemetry-safe report.
struct InstalledLayout {
    skills: BTreeMap<PathBuf, InstalledSkill>,
    managed_parents: BTreeSet<PathBuf>,
}

impl InstalledLayout {
    fn new() -> Self {
        Self {
            skills: BTreeMap::new(),
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
                // Several agents share `.agents/skills`. A fresh install can
                // therefore be followed by a debounce hit in the same sync.
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

type PluginChanges = Vec<(Agent, Vec<String>)>;

/// Successful sync result exposed to the later recording adapter.
pub(crate) struct SyncCompletion {
    pub(crate) plugin_changes: PluginChanges,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "resolution recording consumes successful reports in the activation stack"
        )
    )]
    pub(crate) report: crate::telemetry::SyncReport,
}

/// Complete private result before filesystem layout is handed to indexing.
struct CompletedSync {
    completion: SyncCompletion,
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

    fn into_source(self) -> anyhow::Error {
        self.source
    }
}

impl std::error::Error for SyncFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

/// Mutable private state for one structured sync result.
struct SyncProgress {
    started: Instant,
    retain_telemetry: bool,
    outcome: crate::telemetry::SyncOutcome,
    dependencies: Option<crate::pm::AdmittedDependencies>,
    active: Vec<crate::plugins::ResolvedPlugin>,
    skills: Vec<ResolvedSkillInstall>,
    layout: InstalledLayout,
    compiled_installed: usize,
    compiled_updated: usize,
    reaped: usize,
}

impl SyncProgress {
    fn new(retain_telemetry: bool) -> Self {
        Self {
            started: Instant::now(),
            retain_telemetry,
            outcome: crate::telemetry::SyncOutcome::Ok,
            dependencies: None,
            active: Vec::new(),
            skills: Vec::new(),
            layout: InstalledLayout::new(),
            compiled_installed: 0,
            compiled_updated: 0,
            reaped: 0,
        }
    }

    fn mark_partial(&mut self) {
        promote_to_partial(&mut self.outcome);
    }

    fn set_resolution(
        &mut self,
        dependencies: crate::pm::AdmittedDependencies,
        active: Vec<crate::plugins::ResolvedPlugin>,
        skills: Vec<ResolvedSkillInstall>,
        complete: bool,
    ) {
        self.dependencies = Some(dependencies);
        self.active = active;
        self.skills = skills;
        if !complete {
            self.mark_partial();
        }
    }

    fn record_compilation(&mut self, outcome: crate::compile::CompilationOutcome) {
        self.compiled_installed = self
            .compiled_installed
            .checked_add(outcome.installed)
            .expect("BUG: installed artifact count must fit in usize");
        self.compiled_updated = self
            .compiled_updated
            .checked_add(outcome.updated)
            .expect("BUG: updated artifact count must fit in usize");
        self.reaped = self
            .reaped
            .checked_add(outcome.reaped)
            .expect("BUG: reaped artifact count must fit in usize");
        if !outcome.complete {
            self.mark_partial();
        }
    }

    fn report(&self, outcome: crate::telemetry::SyncOutcome) -> crate::telemetry::SyncReport {
        let extensions = self.resolved_extension_evidence();
        let installed_skills = self
            .layout
            .skills
            .values()
            .filter(|skill| skill.status == Some(SkillSyncStatus::Installed))
            .count();
        let updated_skills = self
            .layout
            .skills
            .values()
            .filter(|skill| skill.status == Some(SkillSyncStatus::Updated))
            .count();
        let installed = self
            .compiled_installed
            .checked_add(installed_skills)
            .expect("BUG: installed artifact count must fit in usize");
        let updated = self
            .compiled_updated
            .checked_add(updated_skills)
            .expect("BUG: updated artifact count must fit in usize");
        let packages = self
            .dependencies
            .as_ref()
            .filter(|_| self.retain_telemetry)
            .map_or(&[][..], crate::pm::AdmittedDependencies::packages);

        crate::telemetry::SyncReport::project(crate::telemetry::SyncReportInput {
            outcome,
            duration: self.started.elapsed(),
            packages,
            extensions,
            plugins: self.active.len(),
            skills: self.skills.len(),
            installed,
            updated,
            reaped: self.reaped,
        })
    }

    /// Return evidence for every resolved plugin and skill.
    ///
    /// Installation affects change counts and indexing, not whether an
    /// extension resolved.
    fn resolved_extension_evidence(&self) -> Vec<&crate::telemetry::ResolutionEvidence> {
        if !self.retain_telemetry {
            return Vec::new();
        }

        self.active
            .iter()
            .filter_map(|resolved| resolved.evidence.as_ref())
            .chain(
                self.skills
                    .iter()
                    .filter_map(|skill| skill.evidence.as_ref()),
            )
            .collect()
    }

    fn finish(self, plugin_changes: PluginChanges) -> CompletedSync {
        let report = self.report(self.outcome);
        CompletedSync {
            completion: SyncCompletion {
                plugin_changes,
                report,
            },
            layout: self.layout,
        }
    }

    fn fail(self, source: anyhow::Error) -> SyncFailure {
        SyncFailure {
            report: Box::new(self.report(crate::telemetry::SyncOutcome::Error)),
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

/// The install directory name a skill falls back to when its plain name is
/// unavailable: `<skill-name>-<origin-hash>`. The revocation cleanup rebuilds
/// the same names, so both go through here.
fn suffixed_skill_dir(skill_name: &str, origin_hash: &str) -> String {
    format!("{skill_name}-{origin_hash}")
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
pub(crate) fn unfollowed_entries(root: &Path, package_root: Option<&Path>) -> Vec<String> {
    walk_tree(root, package_root)
        .map(|(_, skipped)| skipped)
        .unwrap_or_default()
}

fn walk_tree(root: &Path, package_root: Option<&Path>) -> Result<(Vec<TreeEntry>, Vec<String>)> {
    let mut skipped = Vec::new();
    let tree = dir_walk::walk_where(root, &mut |path| {
        let outside = package_root.is_some_and(|package| !plugins::resolves_inside(package, path));
        if outside {
            skipped.push(format!(
                "skipping {}: resolves outside the package",
                display_path(path)
            ));
        }
        !outside
    })?;
    skipped.extend(
        tree.refused
            .into_iter()
            .filter_map(|refused| match refused {
                dir_walk::Refused::Loop(rel) => Some(format!(
                    "skipping {}: symlink re-enters a directory it is inside",
                    display_path(&root.join(rel))
                )),
                dir_walk::Refused::BrokenLink(rel, e) => Some(format!(
                    "skipping {}: broken symlink ({e})",
                    display_path(&root.join(rel))
                )),
                // A socket or a fifo. Never skill content, so never worth a word.
                dir_walk::Refused::Special(_) => None,
            }),
    );
    Ok((tree.entries, skipped))
}

/// Recursively copy the contents of `src` into `dst`. Creates `dst` if
/// missing. Symlinks are followed: a link to a file lands as a real file, a
/// link to a directory as a real directory holding a copy of its contents.
///
/// Entries the walk cannot follow are reported and skipped, so a single broken
/// link leaves the rest of the skill installed instead of failing the sync.
pub(crate) fn copy_dir_recursive(
    src: &Path,
    dst: &Path,
    package_root: Option<&Path>,
    warned: &mut BTreeSet<String>,
) -> Result<()> {
    let (entries, skipped) = walk_tree(src, package_root)?;
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
pub(crate) fn collect_dir_contents(
    dir: &Path,
    package_root: Option<&Path>,
) -> Result<(TreeContents, Vec<String>)> {
    let (entries, skipped) = walk_tree(dir, package_root)?;
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
    if slot_taken(dest) && !is_managed_skill_dir(dest) {
        anyhow::bail!("{} is not managed by symposium", display_path(dest));
    }
    if dest.is_dir() && collect_dir_contents(dest, None)?.0 == *sorted {
        touch_marker(&dest.join(MARKER_FILE))?;
        return Ok(false);
    }
    let parent = dest.parent().expect("plugin dir must have parent");
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    install_staged(dest, |staging| {
        touch_marker(&staging.join(MARKER_FILE))?;
        for (rel, data) in sorted {
            let path = staging.join(rel);
            match data {
                None => fs::create_dir_all(&path),
                Some(bytes) => fs::write(&path, bytes),
            }
            .with_context(|| format!("write {}", path.display()))?;
        }
        Ok(())
    })?;
    Ok(true)
}

pub(crate) fn sync_plugin_dir(source: &Path, dest: &Path, debounce: Duration) -> Result<bool> {
    if recently_synced(dest, debounce)
        && crate::compile::skill_dir_names(source) == crate::compile::skill_dir_names(dest)
    {
        return Ok(false);
    }
    sync_tree(dest, &collect_dir_contents(source, None)?.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReapReport {
    pub(crate) removed: usize,
    pub(crate) complete: bool,
}

pub(crate) fn reap_unlisted(parent: &Path, keep: impl Fn(&Path) -> bool) -> ReapReport {
    let entries = match fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return ReapReport {
                removed: 0,
                complete: true,
            };
        }
        Err(error) => {
            tracing::info!(
                report = %crate::report::ReportEvent::Warning {
                    message: format!(
                        "failed to scan skills directory {}: {error}",
                        display_path(parent),
                    ),
                },
            );
            return ReapReport {
                removed: 0,
                complete: false,
            };
        }
    };
    let mut report = ReapReport {
        removed: 0,
        complete: true,
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                report.complete = false;
                tracing::info!(
                    report = %crate::report::ReportEvent::Warning {
                        message: format!(
                            "failed to inspect an entry in {}: {error}",
                            display_path(parent),
                        ),
                    },
                );
                continue;
            }
        };
        let path = entry.path();
        if !is_managed_skill_dir(&path) || keep(&path) {
            continue;
        }
        match fs::remove_dir_all(&path) {
            Ok(()) => {
                report.removed += 1;
                tracing::info!(
                    report = %crate::report::ReportEvent::SkillRemoved {
                        path: display_path(&path),
                    },
                );
            }
            Err(e) => {
                report.complete = false;
                tracing::info!(
                    report = %crate::report::ReportEvent::Warning {
                        message: format!("failed to remove stale {}: {e}", display_path(&path)),
                    },
                );
            }
        }
    }
    report
}

/// Returns true if the source directory's content differs from the
/// destination's content (ignoring managed metadata files).
fn dir_contents_differ(
    source_dir: &Path,
    dest_dir: &Path,
    package_root: Option<&Path>,
) -> Result<bool> {
    let (src, _) = collect_dir_contents(source_dir, package_root)?;
    let (dst, _) = collect_dir_contents(dest_dir, None)?;
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
/// Returns the exact filesystem outcome so the safe report can distinguish a
/// new install from an update without inspecting paths or mtimes again.
fn sync_skill_dir(
    source_dir: &Path,
    dest_dir: &Path,
    project_root: &Path,
    debounce: Duration,
    warned: &mut BTreeSet<String>,
    package_root: Option<&Path>,
) -> Result<SkillSyncStatus> {
    if dest_dir == source_dir {
        return Ok(SkillSyncStatus::Unchanged);
    }

    // If the destination doesn't exist yet, do a fresh install.
    if !slot_taken(dest_dir) {
        install_skill_dir(source_dir, dest_dir, project_root, warned, package_root)?;
        return Ok(SkillSyncStatus::Installed);
    }

    // Debounce: if we synced recently, skip the content comparison.
    if recently_synced(dest_dir, debounce) {
        tracing::debug!(dest = %dest_dir.display(), "skill sync debounced");
        return Ok(SkillSyncStatus::Debounced);
    }

    // Compare content (excluding managed metadata).
    let marker_path = dest_dir.join(MARKER_FILE);
    if !dir_contents_differ(source_dir, dest_dir, package_root)? {
        // Content is identical — just touch the marker to reset debounce.
        touch_marker(&marker_path)?;
        return Ok(SkillSyncStatus::Unchanged);
    }

    // Content changed: replace entirely.
    install_skill_dir(source_dir, dest_dir, project_root, warned, package_root)?;
    Ok(SkillSyncStatus::Updated)
}

fn install_skill_dir(
    source_dir: &Path,
    dest_dir: &Path,
    project_root: &Path,
    warned: &mut BTreeSet<String>,
    package_root: Option<&Path>,
) -> Result<()> {
    let parent = dest_dir.parent().expect("skill dir must have parent");
    create_managed_dir_all(parent, project_root)?;
    install_staged(dest_dir, |staging| {
        mark_generated_skill_directory(staging)?;
        copy_dir_recursive(source_dir, staging, package_root, warned)?;
        mark_generated_skill_directory(staging)
    })
}

fn install_staged(dest: &Path, fill: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
    let parent = dest.parent().expect("install slot must have parent");
    let mut staging = tempfile::Builder::new()
        .prefix(".symposium-tmp-")
        .tempdir_in(parent)
        .with_context(|| format!("create staging dir in {}", parent.display()))?;
    fill(staging.path())?;
    if slot_taken(dest) {
        fs::remove_dir_all(dest).with_context(|| format!("remove {}", dest.display()))?;
    }
    fs::rename(staging.path(), dest)
        .with_context(|| format!("rename {} -> {}", staging.path().display(), dest.display()))?;
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
struct ResolvedCustomPredicates {
    entries: std::collections::HashMap<String, crate::predicate::ResolvedPredicateEntry>,
    complete: bool,
}

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
            match crate::installation::acquire_installation(sym, install, None, None, update, None)
                .await
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

async fn resolve_sync(
    sym: &Symposium,
    ws: &Arc<Workspace>,
    dependencies: crate::pm::AdmittedDependencies,
    inputs: SyncResolutionInputs<'_>,
) -> ResolvedSync {
    let SyncResolutionInputs {
        registry,
        project_root,
        custom_predicates,
        predicate_cache_path,
        update,
    } = inputs;
    let pms = ws.pms();
    let used = crate::discovery::used(
        sym,
        project_root,
        &crate::discovery::Names::new(registry.plugins.as_slice(), dependencies.package_ids(), pms),
    )
    .await;
    for (entry, pms) in &used.ambiguous {
        tracing::info!(
            report = %crate::report::ReportEvent::Warning {
                message: crate::status_command::ambiguous_use(entry, pms),
            },
        );
    }
    let use_resolution_complete = used.ambiguous.is_empty();

    let mut ctx = crate::predicate::ResolvedPredicateContext::new(dependencies.packages())
        .with_custom_predicates(custom_predicates)
        .with_used(&used.plugins)
        .with_cwd(ws.start())
        .with_disk_cache(predicate_cache_path);
    let retain_evidence = crate::telemetry::is_enabled(sym);
    let active = plugins::resolve_active_plugins_with_evidence(
        sym,
        registry,
        pms,
        Some(project_root),
        &dependencies,
        &mut ctx,
        plugins::ResolvedPluginOptions::new(UpdateLevel::Check, retain_evidence),
    )
    .await;
    let resolved_skills =
        skills::resolve_skills(sym, &active.active, &mut ctx, update, retain_evidence).await;

    let mcp_servers = collect_mcp_servers(sym, &active.active, &mut ctx);

    let global =
        compile::global_plugins(sym, pms, registry, &active.active, &mut ctx, update).await;
    let project = if Agent::Claude.is_configured(sym) {
        compile::project_plugins(&active.active, &resolved_skills.skills, &global)
    } else {
        Vec::new()
    };
    if let Err(error) = ctx.persist_disk_cache(predicate_cache_path) {
        tracing::warn!(
            path = %predicate_cache_path.display(),
            error = %error,
            "failed to persist predicate cache"
        );
    }
    let complete = use_resolution_complete && active.complete && resolved_skills.complete;

    ResolvedSync {
        dependencies,
        active: active.active,
        revoked: active.revoked,
        skills: resolved_skills.skills,
        mcp_servers,
        global,
        project,
        complete,
    }
}

/// Extract MCP servers from an already-active set.
///
/// The fixed point evaluated each plugin gate once. This pass evaluates only
/// the nested server predicates, re-stamping the owning plugin's workspace
/// provenance so one plugin cannot leak that state into the next.
fn collect_mcp_servers(
    sym: &Symposium,
    active: &[plugins::ResolvedPlugin],
    ctx: &mut crate::predicate::ResolvedPredicateContext<'_>,
) -> Vec<sacp::schema::McpServer> {
    let mut servers = Vec::new();
    for resolved in active {
        ctx.set_workspace_member(resolved.plugin.workspace_member);
        let applicable = resolved.plugin.manifest.applicable_mcp_servers(ctx);
        servers.extend(package_mcp_servers(sym, &resolved.plugin, applicable));
    }
    servers
}

fn deduplicate_skills(
    applicable: Vec<crate::skills::SkillWithGroupContext>,
) -> Vec<ResolvedSkillInstall> {
    let mut by_key: std::collections::BTreeMap<(String, String), usize> =
        std::collections::BTreeMap::new();
    let mut skills: Vec<ResolvedSkillInstall> = Vec::new();

    for entry in applicable {
        let name = entry.skill().name().to_owned();
        let origin_hash = entry.origin_hash().to_owned();
        let key = (name.clone(), origin_hash.clone());
        if let Some(&index) = by_key.get(&key) {
            if let (Some(existing), Some(duplicate)) =
                (&mut skills[index].evidence, &entry.evidence)
            {
                let _ = existing.union_contributors(duplicate);
            }
            continue;
        }

        by_key.insert(key, skills.len());
        skills.push(ResolvedSkillInstall {
            name,
            origin_hash,
            source: entry.skill().path.clone(),
            package_root: entry.agent_plugin_root.clone(),
            evidence: entry.evidence,
        });
    }

    skills
}

/// Run the full sync while preserving its established public result.
pub async fn sync(
    sym: &Symposium,
    ws: &Arc<Workspace>,
    update: UpdateLevel,
) -> Result<Vec<(Agent, Vec<String>)>> {
    sync_with_report(sym, ws, update)
        .await
        .map(|completion| completion.plugin_changes)
        .map_err(SyncFailure::into_source)
}

/// Run a full sync and return its telemetry-safe structured report.
pub(crate) async fn sync_with_report(
    sym: &Symposium,
    ws: &Arc<Workspace>,
    update: UpdateLevel,
) -> std::result::Result<SyncCompletion, SyncFailure> {
    let CompletedSync { completion, layout } = run_sync(sym, ws, update).await?;
    // Attribution indexing consumes the private layout in the next slice. It
    // is deliberately discarded before the safe completion leaves sync.
    drop(layout);
    Ok(completion)
}

/// Resolve, install, and clean up while retaining private filesystem layout.
async fn run_sync(
    sym: &Symposium,
    ws: &Arc<Workspace>,
    update: UpdateLevel,
) -> std::result::Result<CompletedSync, SyncFailure> {
    let mut progress = SyncProgress::new(crate::telemetry::is_enabled(sym));
    if let Some(error) = sym.config_error() {
        return Err(progress.fail(anyhow::anyhow!(
            "not syncing: {} could not be read; fix or remove it, then run the command again\n{error}",
            display_path(&sym.config_dir().join("config.toml")),
        )));
    }
    if let Err(error) = sym.config.plugins.validate_disable_managers(ws.pms()) {
        return Err(progress.fail(error));
    }
    let out = &Output::quiet();
    let loaded = match ws.require_info().await {
        Ok(loaded) => loaded.clone(),
        Err(error) => return Err(progress.fail(error)),
    };
    let project_root = loaded.root.clone();
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

    let dependencies = ws.admitted_dependencies().await;
    tracing::info!(
        report = %crate::report::ReportEvent::Info {
            message: format!(
                "scanning {} workspace dependencies",
                dependencies.package_ids().len()
            ),
        },
    );

    // Resolve custom predicate installations.
    let custom = resolve_custom_predicate_entries(sym, &registry, update).await;
    if !custom.complete {
        progress.mark_partial();
    }

    // Resolve admitted packages, plugins, skills, and MCP servers in one pass.
    // Private provenance stays inside this working result.
    let predicate_cache_path =
        crate::predicate_cache::PredicateCache::path_for_workspace(sym.cache_dir(), &project_root);
    let resolved = resolve_sync(
        sym,
        ws,
        dependencies,
        SyncResolutionInputs {
            registry: &registry,
            project_root: &project_root,
            custom_predicates: custom.entries,
            predicate_cache_path: &predicate_cache_path,
            update,
        },
    )
    .await;
    let ResolvedSync {
        dependencies,
        active,
        revoked,
        skills,
        mcp_servers,
        global,
        project,
        complete,
    } = resolved;
    // The install directory names (plain and suffixed) of the skills a revoked
    // package would install. A revocation overrides the cleanup safety net
    // below: these come off even when cleanup is otherwise skipped. The
    // listing runs nothing from the revoked package: none of its predicates
    // is evaluated, and a `source.git` group is read from the git cache only.
    let revoked_dirs: BTreeSet<String> = skills::skills_on_disk(sym, &revoked)
        .into_iter()
        .flat_map(|(name, origin_hash)| [suffixed_skill_dir(&name, &origin_hash), name])
        .collect();

    // Dedup by `(skill_name, origin_hash)`: two crate origins with the same
    // (name, version, skill-path-within-crate) collapse (the same skill bytes
    // reached through two plugins); skills from genuinely different locations —
    // including two skills at different paths within one crate — survive
    // independently. Skills that survive dedup are recorded with both their
    // plain name and their origin hash so we can decide later whether each one
    // needs an `<name>-<hash>` suffix to avoid collisions.
    let to_install = deduplicate_skills(skills);
    progress.set_resolution(dependencies, active, to_install, complete);

    // Sync each configured agent
    let agent_names: Vec<String> = sym.config.agents.iter().map(|a| a.name.clone()).collect();

    tracing::info!(
        workspace_deps = progress
            .dependencies
            .as_ref()
            .map_or(0, |dependencies| dependencies.package_ids().len()),
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

    // Track every skill directory this sync wants, even one whose refresh
    // failed. Anything we find later that has the marker file but isn't in
    // this set is stale.
    // A package manager that failed answered nothing, not nothing-is-offered,
    // so its silence must not retire what it installed before: neither a skill
    // nor an MCP entry. Both cleanups read this one decision.
    let failed_pms = ws.failed();
    let skip_cleanup = !registry.unreadable_registries.is_empty() || !failed_pms.is_empty();
    if skip_cleanup {
        progress.mark_partial();
    }

    let mut warned = BTreeSet::new();
    let plugins_root = sym.config_dir().join(INSTALLED_PLUGINS_SUBDIR);
    // A revoked package's staged plugin comes off first, so the hold-back
    // below cannot hand it to the agents again.
    if skip_cleanup && !revoked.is_empty() {
        let revoked_plugins: BTreeSet<String> = revoked
            .iter()
            .filter_map(|p| compile::normalize_name(&p.manifest.name))
            .collect();
        for root in [&plugins_root, &compile::project_plugins_root(&project_root)] {
            let reaped = reap_unlisted(root, |dir| {
                !dir.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| revoked_plugins.contains(name))
            });
            if !reaped.complete {
                progress.mark_partial();
            }
        }
    }
    let compiled = compile::compile(&plugins_root, &global, debounce, &mut warned, skip_cleanup);
    crate::agents::sync_marketplace_index(sym, &compiled.plugins);
    let project_compiled =
        compile::compile_project(&project_root, &project, debounce, &mut warned, skip_cleanup);
    progress.record_compilation(compiled.outcome);
    progress.record_compilation(project_compiled.outcome);

    let mut plugin_changes = Vec::new();
    let mut copied: std::collections::BTreeMap<_, BTreeSet<_>> = std::collections::BTreeMap::new();
    let mut copies: std::collections::BTreeMap<&str, Vec<_>> = std::collections::BTreeMap::new();
    for agent_name in &agent_names {
        let Ok(agent) = Agent::from_config_name(agent_name) else {
            continue;
        };
        let user = agent.deliver_user_plugins(sym, &compiled.plugins);
        let project = agent.deliver_project_plugins(sym, &project_root, &project_compiled.plugins);
        let mut as_plugins = BTreeSet::new();
        if let Some(delivery) = &user {
            as_plugins.extend(compiled.skill_origins_of(&delivery.delivered));
        }
        if let Some(delivery) = &project {
            as_plugins.extend(project_compiled.skill_origins_of(&delivery.delivered));
        }
        if user.is_some() || project.is_some() {
            let updated = user.into_iter().chain(project).flat_map(|d| d.updated);
            plugin_changes.push((agent, updated.collect()));
        }
        let agent_copies: Vec<_> = progress
            .skills
            .iter()
            .enumerate()
            .filter(|(_, skill)| !as_plugins.contains(&skill.origin_hash))
            .collect();
        copied
            .entry(skills_parent_dir(agent, &project_root))
            .or_default()
            .extend(
                agent_copies
                    .iter()
                    .map(|(_, skill)| (&skill.name, &skill.origin_hash)),
            );
        copies.insert(agent_name, agent_copies);
    }

    for agent_name in &agent_names {
        let Ok(agent) = Agent::from_config_name(agent_name) else {
            promote_to_partial(&mut progress.outcome);
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
            promote_to_partial(&mut progress.outcome);
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
                promote_to_partial(&mut progress.outcome);
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

        for &(skill_index, skill) in &copies[agent_name.as_str()] {
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
                    promote_to_partial(&mut progress.outcome);
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
                suffixed_skill_dir(skill_name, origin_hash)
            };
            let dest_dir = agent.project_skill_dir(&project_root, &dir_name);

            // If the dest exists but is user-managed, skip it.
            if slot_taken(&dest_dir) && !is_managed_skill_dir(&dest_dir) {
                promote_to_partial(&mut progress.outcome);
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

            match sync_skill_dir(
                source_dir,
                &dest_dir,
                &project_root,
                debounce,
                &mut warned,
                skill.package_root.as_deref(),
            ) {
                Ok(status) => {
                    progress
                        .layout
                        .record(dest_dir.clone(), dir_name.clone(), skill_index, status);
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
                        progress
                            .layout
                            .preserve(dest_dir.clone(), dir_name.clone(), skill_index);
                    }
                    promote_to_partial(&mut progress.outcome);
                    tracing::info!(
                        report = %crate::report::ReportEvent::Warning {
                            message: format!("failed to install skill {dir_name}: {e}"),
                        },
                    );
                }
            }
        }
    }

    // A revoked package's MCP servers come off wherever Symposium registered
    // them: every agent, every file in the ledger (user scope included, the
    // default), even while the cleanup below is held back. The names come
    // from its manifest, with no predicate evaluated; a name an active plugin
    // still declares stays, since the entry may now be that plugin's.
    let still_declared: BTreeSet<&str> = progress
        .active
        .iter()
        .flat_map(|resolved| resolved.plugin.manifest.mcp_servers.iter())
        .map(|s| crate::agents::mcp_server_name(&s.server))
        .collect();
    let revoked_mcp: BTreeSet<&str> = revoked
        .iter()
        .flat_map(|p| p.manifest.mcp_servers.iter())
        .map(|s| crate::agents::mcp_server_name(&s.server))
        .filter(|name| !still_declared.contains(name))
        .collect();
    if !revoked_mcp.is_empty() {
        unregister_recorded_mcp_servers(
            sym,
            |_, registration| revoked_mcp.contains(registration.name.as_str()),
            out,
        );
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
        let requested_parent = skills_parent_dir(agent, &project_root);
        let parent = match requested_parent.canonicalize() {
            Ok(parent) => parent,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                progress.mark_partial();
                tracing::info!(
                    report = %crate::report::ReportEvent::Warning {
                        message: format!(
                            "failed to inspect skills directory {}: {error}",
                            display_path(&requested_parent),
                        ),
                    },
                );
                continue;
            }
        };
        if (skip_cleanup && revoked_dirs.is_empty()) || !scanned.insert(parent.clone()) {
            continue;
        }
        // While cleanup is skipped, only directories named like a revoked
        // package's skills are removed. A plain-named directory belonging
        // to another, currently unreadable origin can be removed too; its
        // source is untouched, so the next sync that can read it
        // reinstalls it.
        let reaped = reap_unlisted(&parent, |dir| {
            installed.contains(dir)
                || compile::is_plugin_dir(dir)
                || (skip_cleanup
                    && !dir
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| revoked_dirs.contains(name)))
        });
        progress.reaped = progress
            .reaped
            .checked_add(reaped.removed)
            .expect("BUG: reaped directories must fit in usize");
        if !reaped.complete {
            progress.mark_partial();
        }
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
    for name in &failed_pms {
        tracing::info!(
            report = %crate::report::ReportEvent::Warning {
                message: format!(
                    "package manager `{name}` failed; skipped cleanup of installed skills"
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

    if !warned.is_empty() {
        progress.mark_partial();
    }

    if progress.skills.is_empty() {
        tracing::info!(
            report = %crate::report::ReportEvent::Info {
                message: "no applicable skills found for workspace dependencies".into(),
            },
        );
    }

    Ok(progress.finish(plugin_changes))
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
        if !agent.is_configured(sym) {
            agent.deliver_user_plugins(sym, &[]);
        }
    }
}

fn unregister_project_plugins(sym: &Symposium, project_root: &Path) {
    for &agent in Agent::all() {
        if !agent.is_configured(sym) {
            agent.deliver_project_plugins(sym, project_root, &[]);
        }
    }
}

/// Register global hooks for all configured agents.
/// Register hooks for all configured agents. Uses `home_dir` (global scope).
/// Called from `init` after writing the user config.
pub fn register_hooks(sym: &Symposium, out: &Output) -> Result<()> {
    let agent_names: Vec<String> = sym.config.agents.iter().map(|a| a.name.clone()).collect();

    // `init` has no workspace to evaluate a plugin's gate against, so MCP
    // servers are left to `sync`.
    let home = sym.home_dir();
    for agent_name in &agent_names {
        let Ok(agent) = Agent::from_config_name(agent_name) else {
            out.warn(unknown_agent_warning(agent_name));
            continue;
        };
        agent.register_hooks(home, sym, out)?;
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

fn package_mcp_servers(
    sym: &Symposium,
    plugin: &plugins::Plugin,
    servers: Vec<sacp::schema::McpServer>,
) -> Vec<sacp::schema::McpServer> {
    let Some(root) = &plugin.agent_plugin_root else {
        return servers;
    };
    let stdio =
        |server: &sacp::schema::McpServer| matches!(server, sacp::schema::McpServer::Stdio(_));
    if !servers.iter().any(stdio) {
        return servers;
    }
    match plugin_data_dir(sym, &plugin.canonical) {
        Ok(data) => servers
            .into_iter()
            .map(|server| {
                plugins::launched_from(plugins::with_plugin_vars(server, root, &data), root)
            })
            .collect(),
        Err(e) => {
            tracing::info!(
                report = %crate::report::ReportEvent::Warning {
                    message: format!(
                        "skipping the stdio MCP servers of plugin `{}`: {e:#}",
                        plugin.manifest.name
                    ),
                },
            );
            servers
                .into_iter()
                .filter(|server| !stdio(server))
                .collect()
        }
    }
}

fn plugin_data_dir(sym: &Symposium, id: &crate::pm::PackageId) -> Result<PathBuf> {
    let dir = sym
        .config_dir()
        .join(PLUGIN_DATA_DIR)
        .join(&id.pm)
        .join(&id.name);
    fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", display_path(&dir)))?;
    Ok(fs::canonicalize(&dir)?)
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

    fn plugin_with_workspace_gated_mcp(
        name: &str,
        workspace_member: bool,
    ) -> plugins::ResolvedPlugin {
        let server = toml::from_str(
            r#"
                name = "workspace-server"
                command = "workspace-server"
                args = []
                env = []
            "#,
        )
        .unwrap();
        let plugin = plugins::Plugin {
            manifest: plugins::PluginManifest {
                name: name.to_owned(),
                version: None,
                description: None,
                predicates: Default::default(),
                installations: Vec::new(),
                hooks: Vec::new(),
                skills: Vec::new(),
                mcp_servers: vec![plugins::PluginMcpServer {
                    predicates: crate::predicate::PredicateSet {
                        predicates: vec![crate::predicate::Predicate::WorkspaceMember],
                    },
                    server,
                }],
                subcommands: Default::default(),
                custom_predicates: Vec::new(),
                chained: Vec::new(),
                requires_use: false,
            },
            workspace_member,
            agent_plugin_root: None,
            root: PathBuf::new(),
            canonical: crate::pm::PackageId::any_version("test", name),
            resolved_content: None,
        };
        plugins::ResolvedPlugin {
            plugin,
            evidence: None,
        }
    }

    fn dependency_evidence(
        dependency: &crate::pm::ResolvedDependency,
    ) -> crate::telemetry::ResolutionEvidence {
        let dependencies = [dependency.clone()];
        let mut context = crate::predicate::ResolvedPredicateContext::new(&dependencies);
        let predicates = crate::predicate::PredicateSet {
            predicates: vec![crate::predicate::Predicate::DependsOn(
                crate::predicate::Dependency::named(dependency.id().name.clone()),
            )],
        };
        crate::telemetry::evaluate_resolution_predicates(
            &predicates,
            &mut context,
            &mut crate::telemetry::OptionalWitnessMode::new(true),
        )
        .unwrap()
        .unwrap()
    }

    #[test]
    fn skill_sync_reports_each_filesystem_outcome() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source");
        let destination = tmp.path().join("skills/example");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "first").unwrap();
        let mut warned = BTreeSet::new();

        assert_eq!(
            sync_skill_dir(
                &source,
                &destination,
                tmp.path(),
                Duration::ZERO,
                &mut warned,
                None,
            )
            .unwrap(),
            SkillSyncStatus::Installed,
        );
        assert_eq!(
            sync_skill_dir(
                &source,
                &destination,
                tmp.path(),
                Duration::MAX,
                &mut warned,
                None,
            )
            .unwrap(),
            SkillSyncStatus::Debounced,
        );
        assert_eq!(
            sync_skill_dir(
                &source,
                &destination,
                tmp.path(),
                Duration::ZERO,
                &mut warned,
                None,
            )
            .unwrap(),
            SkillSyncStatus::Unchanged,
        );

        fs::write(source.join("SKILL.md"), "second").unwrap();
        assert_eq!(
            sync_skill_dir(
                &source,
                &destination,
                tmp.path(),
                Duration::ZERO,
                &mut warned,
                None,
            )
            .unwrap(),
            SkillSyncStatus::Updated,
        );
    }

    #[test]
    fn shared_parent_keeps_the_strongest_slot_status() {
        let destination = PathBuf::from("project/.agents/skills/example");
        let mut layout = InstalledLayout::new();

        layout.record(
            destination.clone(),
            "example".into(),
            0,
            SkillSyncStatus::Installed,
        );
        layout.record(
            destination.clone(),
            "example".into(),
            0,
            SkillSyncStatus::Debounced,
        );

        assert_eq!(layout.skills.len(), 1);
        assert_eq!(
            layout.skills[&destination].status,
            Some(SkillSyncStatus::Installed),
        );
        assert_eq!(
            layout.managed_parents,
            BTreeSet::from([PathBuf::from("project/.agents/skills")]),
        );
    }

    #[test]
    fn failed_refresh_preserves_the_managed_slot_without_counting_it() {
        let destination = PathBuf::from("project/.agents/skills/example");
        let mut layout = InstalledLayout::new();

        layout.preserve(destination.clone(), "example".into(), 0);

        assert_eq!(layout.skills[&destination].status, None);
        assert!(
            layout
                .managed_parents
                .contains(&PathBuf::from("project/.agents/skills"))
        );
    }

    #[test]
    fn reap_report_counts_only_removed_managed_slots() {
        let tmp = tempfile::tempdir().unwrap();
        let stale = tmp.path().join("stale");
        let retained = tmp.path().join("retained");
        let user_owned = tmp.path().join("user-owned");
        for directory in [&stale, &retained, &user_owned] {
            fs::create_dir(directory).unwrap();
        }
        touch_marker(&stale.join(MARKER_FILE)).unwrap();
        touch_marker(&retained.join(MARKER_FILE)).unwrap();

        let report = reap_unlisted(tmp.path(), |path| path == retained);

        assert_eq!(
            report,
            ReapReport {
                removed: 1,
                complete: true,
            }
        );
        assert!(!stale.exists());
        assert!(retained.exists());
        assert!(user_owned.exists());
    }

    #[test]
    fn hard_failure_preserves_the_safe_progress_report() {
        let failure = SyncProgress::new(false).fail(anyhow::anyhow!("sentinel failure"));

        assert_eq!(
            failure.report().outcome(),
            crate::telemetry::SyncOutcome::Error
        );
        assert_eq!(failure.to_string(), "sentinel failure");
        assert_eq!(
            std::error::Error::source(&failure).unwrap().to_string(),
            "sentinel failure",
        );
        assert_eq!(failure.into_source().to_string(), "sentinel failure");
    }

    #[test]
    fn successful_progress_retains_the_safe_report() {
        let completed = SyncProgress::new(false).finish(Vec::new());

        assert!(completed.completion.plugin_changes.is_empty());
        assert_eq!(
            completed.completion.report.outcome(),
            crate::telemetry::SyncOutcome::Ok,
        );
    }

    #[test]
    fn compilation_outcomes_join_skill_artifact_counts() {
        let mut progress = SyncProgress::new(false);
        progress.record_compilation(crate::compile::CompilationOutcome {
            installed: 1,
            updated: 2,
            reaped: 3,
            complete: false,
        });
        progress.layout.record(
            PathBuf::from("project/.agents/skills/new"),
            "new".into(),
            0,
            SkillSyncStatus::Installed,
        );
        progress.layout.record(
            PathBuf::from("project/.agents/skills/changed"),
            "changed".into(),
            1,
            SkillSyncStatus::Updated,
        );

        let completed = progress.finish(Vec::new());

        assert_eq!(
            completed.completion.report.outcome(),
            crate::telemetry::SyncOutcome::Partial,
        );
        assert_eq!(completed.completion.report.artifact_counts(), (2, 3, 3));
    }

    #[test]
    fn resolved_skill_evidence_does_not_depend_on_installation() {
        use symposium_sdk::pm::PackageSourceClaim;

        let dependency = crate::pm::ResolvedDependency::for_test(
            0,
            crate::pm::PackageId::new(crate::pm::CARGO_PM, "runtime", "1.0.0"),
            PackageSourceClaim::CratesIo,
            crate::pm::PackageNamingAuthority::BuiltinCargo,
        );
        let evidence = crate::telemetry::skill_resolution_evidence(
            Some(dependency_evidence(&dependency)),
            None,
            None,
            crate::skills::SkillNamingView::for_test("resolved-skill", None),
        )
        .unwrap();
        let mut progress = SyncProgress::new(true);
        progress.skills.push(ResolvedSkillInstall {
            name: "resolved-skill".into(),
            origin_hash: "origin".into(),
            source: PathBuf::from("source/SKILL.md"),
            package_root: None,
            evidence: Some(evidence),
        });

        assert!(progress.layout.skills.is_empty());
        assert_eq!(progress.resolved_extension_evidence().len(), 1);
    }

    #[test]
    fn duplicate_skill_routes_union_exact_contributors() {
        use symposium_sdk::pm::PackageSourceClaim;

        let dependency = |slot, name: &str| {
            crate::pm::ResolvedDependency::for_test(
                slot,
                crate::pm::PackageId::new(crate::pm::CARGO_PM, name, "1.0.0"),
                PackageSourceClaim::CratesIo,
                crate::pm::PackageNamingAuthority::BuiltinCargo,
            )
        };
        let left = dependency(0, "left-runtime");
        let right = dependency(1, "right-runtime");
        let skill = |evidence| {
            let mut skill = crate::skills::SkillWithGroupContext::for_test(
                crate::skills::Skill {
                    frontmatter: std::collections::BTreeMap::from([(
                        "name".to_string(),
                        "shared-skill".to_string(),
                    )]),
                    predicates: Default::default(),
                    path: PathBuf::from("shared/SKILL.md"),
                },
                "same-origin".into(),
                0,
                None,
            );
            skill.evidence = Some(evidence);
            skill
        };

        let deduplicated = deduplicate_skills(vec![
            skill(dependency_evidence(&left)),
            skill(dependency_evidence(&right)),
        ]);

        assert_eq!(deduplicated.len(), 1);
        let contributors = deduplicated[0].evidence.as_ref().unwrap().contributors();
        assert_eq!(contributors.len(), 2);
        assert!(contributors.contains(&left.handle()));
        assert!(contributors.contains(&right.handle()));
    }

    #[test]
    fn mcp_predicates_use_their_own_plugins_workspace_provenance() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(tmp.path());
        let active = [
            plugin_with_workspace_gated_mcp("workspace", true),
            plugin_with_workspace_gated_mcp("package", false),
        ];
        let dependencies = Vec::new();
        let mut context = crate::predicate::ResolvedPredicateContext::new(&dependencies);

        let servers = collect_mcp_servers(&sym, &active, &mut context);

        assert_eq!(servers.len(), 1);
    }

    #[test]
    fn a_failed_tree_write_keeps_the_installed_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("plugin");
        let installed: TreeContents = vec![(PathBuf::from("a.txt"), Some(b"one".to_vec()))];
        assert!(sync_tree(&dest, &installed).unwrap());

        let broken: TreeContents = vec![(PathBuf::from("missing/b.txt"), Some(b"two".to_vec()))];
        assert!(sync_tree(&dest, &broken).is_err());

        assert_eq!(fs::read_to_string(dest.join("a.txt")).unwrap(), "one");
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 1);
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

        copy_dir_recursive(&src, &dst, None, &mut BTreeSet::new()).unwrap();

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
            !dir_contents_differ(&src, &dst, None).unwrap(),
            "the source scan must see exactly what the copier wrote, or every \
             sync turns into a full recopy"
        );
    }

    #[cfg(unix)]
    #[test]
    fn unfollowable_entries_are_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tree_with_symlinks(tmp.path());

        let (_, skipped) = walk_tree(&src, None).unwrap();

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
        copy_dir_recursive(&src, &dst, None, &mut BTreeSet::new()).unwrap();

        fs::write(tmp.path().join("outside/lib.txt"), "edited").unwrap();

        assert!(dir_contents_differ(&src, &dst, None).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn a_package_root_skips_what_resolves_outside_it() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::create_dir(root.join("outside")).unwrap();
        fs::write(root.join("outside/lib.txt"), "outside").unwrap();
        let package = root.join("package");
        let skill = package.join("skills/s");
        fs::create_dir_all(&skill).unwrap();
        fs::create_dir(package.join("shared")).unwrap();
        fs::write(package.join("shared/notes.txt"), "inside").unwrap();
        fs::write(skill.join("SKILL.md"), "skill").unwrap();
        symlink("../../shared/notes.txt", skill.join("notes.txt")).unwrap();
        symlink("../../shared", skill.join("shared")).unwrap();
        symlink("../../../outside/lib.txt", skill.join("lib.txt")).unwrap();
        symlink("../../../outside", skill.join("vendor")).unwrap();
        symlink("../../outside", package.join("skills/linked")).unwrap();

        let inside = ["SKILL.md", "notes.txt", "shared", "shared/notes.txt"];
        let cases: [(&str, &[&str], usize); 2] =
            [("skills/s", &inside, 2), ("skills/linked", &[], 1)];
        for (dir, kept, refused) in cases {
            let (entries, skipped) = walk_tree(&package.join(dir), Some(&package)).unwrap();
            let entries: Vec<&str> = entries
                .iter()
                .map(|(rel, _)| rel.to_str().unwrap())
                .collect();
            assert_eq!(entries, kept, "{dir}");
            assert_eq!(skipped.len(), refused, "{dir}: {skipped:?}");
        }
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
            let _complete = skills::find_skill_files_recursive(dir, &mut found);
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

        copy_dir_recursive(&src, &dst, None, &mut BTreeSet::new()).unwrap();

        assert_eq!(fs::read_to_string(dst.join("sub/a.txt")).unwrap(), "a");
        assert!(!dir_contents_differ(&src, &dst, None).unwrap());
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

        copy_dir_recursive(&src, &dst, None, &mut BTreeSet::new()).unwrap();
        mark_generated_skill_directory(&dst).unwrap();

        assert!(!dir_contents_differ(&src, &dst, None).unwrap());
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
            None,
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
            sync_skill_dir(
                &src,
                &dst,
                tmp.path(),
                Duration::ZERO,
                &mut BTreeSet::new(),
                None
            )
            .is_err()
        );
        assert_eq!(fs::read_dir(&skills).unwrap().count(), 0);

        fs::set_permissions(&notes, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            sync_skill_dir(
                &src,
                &dst,
                tmp.path(),
                Duration::ZERO,
                &mut BTreeSet::new(),
                None
            )
            .unwrap(),
            SkillSyncStatus::Installed,
        );
        assert!(dst.join(MARKER_FILE).exists());
        assert_eq!(fs::read_to_string(dst.join("notes.txt")).unwrap(), "notes");
    }
}
