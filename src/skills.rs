//! Skill model, frontmatter parsing, discovery, and crate advice output.
//!
//! Skills follow the [agentskills.io](https://agentskills.io/specification.md) format
//! and live inside plugin directories under `skills/*/SKILL.md`.

use crate::predicate::PredicateEval;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use symposium_install::UpdateLevel;

use crate::config::Symposium;
use crate::output::display_path;
use crate::plugins::{Plugin, PluginSource, SkillGroup};
#[cfg(test)]
use crate::predicate::PredicateContext;
use crate::predicate::{PredicateContextImpl, PredicateDependencies, PredicateSet};

fn source_display(source: &PluginSource) -> String {
    match source {
        PluginSource::Path(p) => format!("path:{}", p.display()),
        PluginSource::Git(url) => format!("git:{url}"),
    }
}

/// The group's resolved source label: the string the package manager attached
/// to a `Path` source, or the derived form for a `Git` source.
fn group_source_label(group: &crate::plugins::SkillGroup) -> String {
    group
        .source_label
        .clone()
        .unwrap_or_else(|| source_display(&group.source))
}

/// A parsed skill from a SKILL.md file.
#[derive(Debug, Clone)]
pub struct Skill {
    /// String-valued frontmatter fields (name, description, license, etc.).
    pub frontmatter: BTreeMap<String, String>,
    /// Skill-level activation predicates: the frontmatter `depends-on` (lowered to
    /// `any(depends-on(...))`) merged with `predicates`. ANDed with the plugin- and
    /// group-level sets.
    pub predicates: PredicateSet,
    /// Path to the SKILL.md file on disk.
    pub path: PathBuf,
}

impl Skill {
    /// Return the skill name from frontmatter, or "unknown".
    pub fn name(&self) -> &str {
        self.frontmatter
            .get("name")
            .map_or("unknown", |s| s.as_str())
    }
}

// Install-path disambiguator identifying *where a skill's bytes live*.
//
// A skill's origin is the *canonical* on-disk path of its `SKILL.md`, hashed.
// Two references that resolve to the same file — the same crate reached through
// two chained plugins, or a `source.path` group and the standalone walk landing
// on the same bundle — produce the same hash and dedupe at sync time; skills at
// different paths stay distinct.
//
// Canonicalizing is what makes that hold. Group discovery walks a canonicalized
// scan dir, while the standalone registry walk uses the configured source path
// verbatim; on a platform whose temp prefix is a symlink (macOS `/var` ->
// `/private/var`) the same file would otherwise hash two ways and install twice.
//
// This is deliberately coarser than a structured `(pm, plugin, skill_path)`
// identity: the hash *is* both the dedup key and the disambiguating suffix, so
// only a string — not a structured origin — is carried to the sync layer.

/// 8-hex-char prefix of SHA-256 over the JSON-serialized origin key.
pub(crate) fn hash_origin_key<T: serde::Serialize>(key: &T) -> String {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(key).expect("origin key always serializes");
    let digest = Sha256::digest(&bytes);
    let mut out = String::with_capacity(8);
    for byte in &digest[..4] {
        use std::fmt::Write;
        write!(out, "{byte:02x}").unwrap();
    }
    out
}

/// Origin hash for a skill, keyed on its `SKILL.md`'s canonical path so that the
/// same file reached two ways hashes identically. Falls back to the given path
/// when it can't be canonicalized.
pub(crate) fn skill_origin_hash(skill_md: &Path) -> String {
    let canonical = dunce::canonicalize(skill_md);
    let path = canonical.as_deref().unwrap_or(skill_md);
    canonical_skill_origin_hash(path)
}

fn canonical_skill_origin_hash(skill_md: &Path) -> String {
    hash_origin_key(&skill_md.to_string_lossy())
}

/// An applicable skill paired with the origin it was discovered through.
///
/// The plugin-, group-, and skill-level predicate sets are all evaluated during
/// collection; only skills whose every level holds end up here.
#[derive(Clone)]
pub struct SkillWithGroupContext {
    skill: Skill,
    /// The hash of where the skill was discovered. Drives install-path disambiguation
    /// and dedup at sync time.
    origin_hash: String,
    pub plugin_index: usize,
    pub agent_plugin_root: Option<PathBuf>,
    /// Proof that this skill's canonical `SKILL.md` belonged to its admitted
    /// package. Kept private so it cannot be paired with another skill.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "resolution producers activate skill naming in a later slice"
        )
    )]
    package_content: Option<PackageContent>,
}

/// Proof that this record's canonical skill file belonged to one admitted
/// package when discovery read it.
///
/// Construction and storage are both private to skill discovery. Cloning the
/// containing record preserves the same skill/proof pairing; callers cannot
/// project the proof without the skill name it was bound to.
#[derive(Clone)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "resolution producers activate skill naming in a later slice"
    )
)]
struct PackageContent {
    package: crate::pm::ResolvedPackage,
    canonical_file: PathBuf,
}

impl PackageContent {
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "resolution producers activate skill naming in a later slice"
        )
    )]
    fn naming_view(&self) -> crate::pm::PackageNamingView<'_> {
        debug_assert!(
            self.canonical_file.is_absolute(),
            "BUG: package-content proofs retain a canonical file"
        );
        self.package.naming_view()
    }
}

/// The public-naming facts bound to one discovered skill.
///
/// Callers cannot pair a package proof from one skill with another skill's
/// name: both values are projected together from [`SkillWithGroupContext`].
#[derive(Clone, Copy)]
pub(crate) struct SkillNamingView<'a> {
    name: &'a str,
    package: Option<crate::pm::PackageNamingView<'a>>,
}

impl SkillWithGroupContext {
    #[cfg(test)]
    pub(crate) fn for_test(
        skill: Skill,
        origin_hash: String,
        plugin_index: usize,
        agent_plugin_root: Option<PathBuf>,
    ) -> Self {
        Self {
            skill,
            origin_hash,
            plugin_index,
            agent_plugin_root,
            package_content: None,
        }
    }

    /// The immutable skill bound to this resolution record.
    pub(crate) fn skill(&self) -> &Skill {
        &self.skill
    }

    /// The install-path disambiguator derived from this skill's canonical
    /// `SKILL.md` destination.
    pub(crate) fn origin_hash(&self) -> &str {
        &self.origin_hash
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "resolution producers activate skill naming in a later slice"
        )
    )]
    pub(crate) fn naming_view(&self) -> SkillNamingView<'_> {
        SkillNamingView {
            name: self.skill.name(),
            package: self
                .package_content
                .as_ref()
                .map(PackageContent::naming_view),
        }
    }
}

impl<'a> SkillNamingView<'a> {
    pub(crate) fn name(self) -> &'a str {
        self.name
    }

    pub(crate) fn package(self) -> Option<crate::pm::PackageNamingView<'a>> {
        self.package
    }

    #[cfg(test)]
    pub(crate) const fn for_test(
        name: &'a str,
        package: Option<crate::pm::PackageNamingView<'a>>,
    ) -> Self {
        Self { name, package }
    }
}

/// Resolve all applicable skills from the registry.
///
/// Resolve all skills applicable to the given crates.
///
/// `for_crates` is the set of crate name/version pairs to match against.
/// For `crate --list`, this is the full workspace deps.
/// For `crate <name>`, this is a single-element slice with the resolved crate.
///
/// `workspace_root` scopes the `[plugins] use` entries that apply, which
/// decide both which dormant plugins wake up and which dependency-embedded
/// plugins load. `None` (no workspace) means no entry applies.
/// Convenience wrapper over [`crate::plugins::active_plugins`] + [`collect_skills`] that builds
/// the predicate context itself. Production sync shares one context across the
/// skill and MCP passes, so it calls the two primitives directly; this keeps the
/// combined form for tests that only care about the resulting skills.
#[cfg(test)]
pub async fn skills_applicable_to(
    sym: &Symposium,
    registry: &crate::plugins::PluginRegistry,
    ws: &std::sync::Arc<crate::pm::Workspace>,
    workspace_root: Option<&Path>,
    custom_predicate_entries: std::collections::HashMap<
        String,
        crate::predicate::ResolvedPredicateEntry,
    >,
    update: UpdateLevel,
) -> Vec<SkillWithGroupContext> {
    let for_crates = ws.dep_ids().await.to_vec();
    let pms = ws.pms();
    let used =
        crate::discovery::used_plugins(sym, workspace_root, &registry.plugins, &for_crates, pms)
            .await;
    let mut ctx = PredicateContext::new(&for_crates)
        .with_custom_predicates(custom_predicate_entries)
        .with_used(&used);

    let active =
        crate::plugins::active_plugins(sym, registry, pms, workspace_root, &mut ctx, update).await;
    collect_skills(sym, &active, &mut ctx, update).await
}

/// Extract the applicable skills from an already-resolved active plugin set.
///
/// Provenance is re-stamped per plugin so a `source.path` group's
/// `workspace-member()` predicate sees the plugin it belongs to. Skill git
/// sources are fetched here (hence `update`), which is why this is separate
/// from [`crate::plugins::active_plugins`].
pub(crate) async fn collect_skills<D>(
    sym: &Symposium,
    active: &[Plugin],
    ctx: &mut PredicateContextImpl<D>,
    update: UpdateLevel,
) -> Vec<SkillWithGroupContext>
where
    D: PredicateDependencies,
{
    let mut results = Vec::new();
    for (plugin_index, parsed) in active.iter().enumerate() {
        ctx.set_workspace_member(parsed.workspace_member);
        for group in &parsed.manifest.skills {
            let skills = load_skills_for_group(sym, parsed, group, ctx, update).await;
            for (skill, origin_hash, package_content) in skills {
                collect_skill_applicable_to(
                    skill,
                    origin_hash,
                    plugin_index,
                    package_content,
                    parsed,
                    ctx,
                    &mut results,
                );
            }
        }
    }
    results
}

/// Every skill `plugins` could have installed, as `(name, origin hash)`,
/// found without running anything the plugins carry.
///
/// This is for plugins that must not run, such as a revoked crate whose
/// installs sync retires. No predicate is evaluated at any level (a `shell`
/// or custom predicate would run the plugin's code), and a `source.git` group
/// is read only from the git cache, never fetched: one not cached contributes
/// nothing. A skill that does not load is skipped silently.
pub(crate) fn skills_on_disk(sym: &Symposium, plugins: &[Plugin]) -> Vec<(String, String)> {
    let git_cache = symposium_install::git::GitCacheManager::new(&sym.install_context(), "plugins");
    let mut out = Vec::new();
    for plugin in plugins {
        for group in &plugin.manifest.skills {
            let dir = match &group.source {
                PluginSource::Path(dir) => Some(dir.clone()),
                PluginSource::Git(url) => git_cache.cache_path_for_url(url),
            };
            let Some(dir) = dir else { continue };
            for skill in discover_skills(&dir, group.workspace_member, &group.predicates)
                .into_iter()
                .flatten()
            {
                let origin_hash = skill_origin_hash(&skill.path);
                out.push((skill.name().to_string(), origin_hash));
            }
        }
    }
    out
}

/// Discover and load skills for a group, applying pre-fetch filtering.
///
/// Checks group-level `depends-on` predicates against `for_crates` before
/// fetching git sources, to avoid unnecessary downloads. Each returned skill is
/// paired with the origin hash it was discovered through — its group's origin
/// key combined
/// with the SKILL.md's path within that origin, one per discovered SKILL.md.
async fn load_skills_for_group<D>(
    sym: &Symposium,
    parsed: &Plugin,
    group: &SkillGroup,
    ctx: &mut PredicateContextImpl<D>,
    update: UpdateLevel,
) -> Vec<(Skill, String, Option<PackageContent>)>
where
    D: PredicateDependencies,
{
    let plugin = &parsed.manifest;

    // Pre-fetch filtering: skip groups whose predicates don't hold (crate
    // matching and runtime checks alike). Done before any git/crates fetch so
    // we don't pay network cost when the group doesn't apply.
    let predicates_display = group
        .predicates
        .predicates
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let predicates_display = (!predicates_display.is_empty()).then_some(predicates_display);

    if !group.predicates.evaluate(ctx) {
        tracing::debug!(plugin = %plugin.name, "skill group predicates failed, skipping");
        tracing::debug!(
            report = %crate::report::ReportEvent::SkillGroupConsidered {
                plugin: plugin.name.clone(),
                group_crates: predicates_display,
                source: Some(group_source_label(group)),
                matched: false,
                skills_found: None,
                reason: Some("group predicates not satisfied".into()),
            },
        );
        return Vec::new();
    }

    let resolved = resolve_group_dirs(sym, parsed, group, update).await;
    let skills = collect_skills_from_dirs(resolved, group);

    tracing::debug!(
        report = %crate::report::ReportEvent::SkillGroupConsidered {
            plugin: plugin.name.clone(),
            group_crates: predicates_display,
            source: Some(group_source_label(group)),
            matched: true,
            skills_found: Some(skills.len()),
            reason: None,
        },
    );

    skills
}

/// A base directory resolved for a skill group, with the report labels for the
/// skills discovered inside it. Both `source` variants reduce to this: `Path` is
/// already on disk; `Git` is fetched via the git cache. (A crate is not a group
/// source — it becomes a plugin through a `[[plugins]]` chained reference; see
/// [`crate::plugins::active_plugins`].)
struct ResolvedSkillDir<'a> {
    dir: PathBuf,
    /// `SkillSourceSearched` report `plugin` label.
    plugin_label: String,
    /// `SkillSourceSearched` report `source` label.
    source_label: String,
    origin: SkillDirectoryOrigin<'a>,
}

/// Whether a skill directory may inherit an admitted package's authority.
///
/// A detached Git group stays detached even when its cache directory happens
/// to sit beneath the plugin package's content root.
#[derive(Clone, Copy)]
enum SkillDirectoryOrigin<'a> {
    Package(&'a crate::pm::ResolvedContent),
    Detached,
}

fn skill_directory_origin<'a>(
    content: Option<&'a crate::pm::ResolvedContent>,
    source: &PluginSource,
) -> SkillDirectoryOrigin<'a> {
    match source {
        PluginSource::Path(_) => content.map_or(
            SkillDirectoryOrigin::Detached,
            SkillDirectoryOrigin::Package,
        ),
        PluginSource::Git(_) => SkillDirectoryOrigin::Detached,
    }
}

/// Resolve a group's `source` to the base directories to scan. Both variants
/// land on a directory to walk; the only difference is whether the base is local
/// (`Path`) or fetched (`Git` via a git cache). Skill identity is not decided
/// here — every discovered skill's origin is the hash of its on-disk `SKILL.md`
/// path (see the module-level note above the origin-hash helpers).
async fn resolve_group_dirs<'a>(
    sym: &Symposium,
    parsed: &'a Plugin,
    group: &SkillGroup,
    update: UpdateLevel,
) -> Vec<ResolvedSkillDir<'a>> {
    let plugin = &parsed.manifest;

    match &group.source {
        // Resolved to an absolute directory by the package manager, so there is
        // nothing to join here.
        PluginSource::Path(dir) => {
            vec![ResolvedSkillDir {
                dir: dir.clone(),
                plugin_label: plugin.name.clone(),
                source_label: group_source_label(group),
                origin: skill_directory_origin(parsed.resolved_content(), &group.source),
            }]
        }
        PluginSource::Git(url) => {
            let Some((cache_dir, source, _commit_sha)) =
                fetch_git_skill_source(sym, url, update).await
            else {
                return Vec::new();
            };
            vec![ResolvedSkillDir {
                dir: cache_dir,
                plugin_label: source.repo_id(),
                source_label: format!("git:{url}"),
                origin: skill_directory_origin(parsed.resolved_content(), &group.source),
            }]
        }
    }
}

/// Discover skills in each resolved base dir and stamp origins. The single
/// path all group sources funnel through, replacing the former per-source
/// `load_*_skills` functions.
fn collect_skills_from_dirs(
    resolved: Vec<ResolvedSkillDir<'_>>,
    group: &SkillGroup,
) -> Vec<(Skill, String, Option<PackageContent>)> {
    let mut skills = Vec::new();
    for entry in resolved {
        let discovered = discover_skills_with_origin(
            &entry.dir,
            group.workspace_member,
            &group.predicates,
            entry.origin,
        );
        tracing::debug!(
            report = %crate::report::ReportEvent::SkillSourceSearched {
                plugin: entry.plugin_label.clone(),
                source: entry.source_label.clone(),
                path: entry.dir.display().to_string(),
                skills_found: discovered.iter().filter(|r| r.is_ok()).count(),
            },
        );
        for result in discovered {
            match result {
                Ok(discovered) => {
                    skills.push((
                        discovered.skill,
                        discovered.origin_hash,
                        discovered.package_content,
                    ));
                }
                Err(e) => tracing::info!(
                    report = %crate::report::ReportEvent::Warning {
                        message: format!("skipping {e:#}"),
                    },
                ),
            }
        }
    }
    skills
}

/// Fetch a `source.git` group's tarball and look up the resolved commit
/// SHA from the cache meta. Returns `(cache_dir, parsed_source, commit_sha)`
/// or `None` (with a warning) on failure.
async fn fetch_git_skill_source(
    sym: &Symposium,
    git_url: &str,
    update: UpdateLevel,
) -> Option<(PathBuf, symposium_install::git::GitSource, String)> {
    let cache_mgr = symposium_install::git::GitCacheManager::new(&sym.install_context(), "plugins");
    let (cache_dir, source) = match cache_mgr.fetch_url_parsed(git_url, update).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(git = %git_url, error = %e, "failed to fetch skill source");
            return None;
        }
    };
    let Some(meta) = cache_mgr.read_meta_for(&cache_dir) else {
        tracing::warn!(
            git = %git_url,
            cache_dir = %cache_dir.display(),
            "skipping git skill group: cache meta missing (cannot pin commit SHA)"
        );
        return None;
    };
    Some((cache_dir, source, meta.commit_sha))
}

/// Discover all skills found in a given directory.
///
/// Recursively searches for `SKILL.md` files, then prunes nested candidates
/// (if `A/SKILL.md` exists, `A/B/SKILL.md` is excluded — skills don't nest).
pub(crate) fn discover_skills(
    skills_dir: &Path,
    workspace_member: bool,
    group_predicates: &PredicateSet,
) -> Vec<Result<Skill>> {
    discover_skills_with_origin(
        skills_dir,
        workspace_member,
        group_predicates,
        SkillDirectoryOrigin::Detached,
    )
    .into_iter()
    .map(|result| result.map(|discovered| discovered.skill))
    .collect()
}

struct DiscoveredSkill {
    skill: Skill,
    origin_hash: String,
    package_content: Option<PackageContent>,
}

struct ResolvedSkillFile {
    canonical_file: PathBuf,
    package_content: Option<PackageContent>,
}

impl ResolvedSkillFile {
    fn path(&self) -> &Path {
        &self.canonical_file
    }

    fn into_package_content(self) -> Option<PackageContent> {
        self.package_content
    }
}

fn resolve_skill_file(
    skill_md: &Path,
    origin: SkillDirectoryOrigin<'_>,
) -> io::Result<ResolvedSkillFile> {
    let canonical_file = dunce::canonicalize(skill_md)?;
    if !canonical_file.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "resolved skill content is not a file",
        ));
    }

    let package_content = match origin {
        SkillDirectoryOrigin::Package(content) if canonical_file.starts_with(content.root()) => {
            Some(PackageContent {
                package: content.package().clone(),
                canonical_file: canonical_file.clone(),
            })
        }
        SkillDirectoryOrigin::Package(_) | SkillDirectoryOrigin::Detached => None,
    };

    Ok(ResolvedSkillFile {
        canonical_file,
        package_content,
    })
}

fn discover_skills_with_origin(
    skills_dir: &Path,
    workspace_member: bool,
    group_predicates: &PredicateSet,
    origin: SkillDirectoryOrigin<'_>,
) -> Vec<Result<DiscoveredSkill>> {
    if !skills_dir.is_dir() {
        return Vec::new();
    }

    let mut skill_files = Vec::new();
    find_skill_files_recursive(skills_dir, &mut skill_files);
    prune_nested_skills(&mut skill_files);
    skill_files.sort();

    skill_files
        .into_iter()
        .map(|skill_md| {
            (|| -> Result<DiscoveredSkill> {
                let resolved_file = resolve_skill_file(&skill_md, origin)?;
                let origin_hash = canonical_skill_origin_hash(resolved_file.path());
                let skill = load_skill_from(
                    resolved_file.path(),
                    &skill_md,
                    workspace_member,
                    group_predicates,
                )?;
                Ok(DiscoveredSkill {
                    skill,
                    origin_hash,
                    package_content: resolved_file.into_package_content(),
                })
            })()
            .with_context(|| display_path(&skill_md))
        })
        .collect()
}

/// Recursively walk a directory collecting paths to `SKILL.md` files.
///
/// Directories carrying the `.symposium` marker are skipped: the marker means
/// symposium itself installed the directory, and installed copies are never
/// sources. This matters for `.agents/skills/`, which is both a workspace
/// skill-group source and the install destination for vendor-neutral agents.
pub(crate) fn find_skill_files_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut ancestors = crate::dir_walk::Ancestors::rooted_at(dir);
    find_skill_files_in(dir, &mut ancestors, out);
}

fn find_skill_files_in(
    dir: &Path,
    ancestors: &mut crate::dir_walk::Ancestors,
    out: &mut Vec<PathBuf>,
) {
    if crate::sync::has_symposium_marker(dir) {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // `is_dir` follows symlinks, so a symlinked directory of skills is
        // discovered — and can name a directory this walk is already inside.
        if path.is_dir() {
            if ancestors.enter(&path) {
                find_skill_files_in(&path, ancestors, out);
                ancestors.leave();
            }
        } else if path.file_name().is_some_and(|f| f == "SKILL.md") {
            out.push(path);
        }
    }
}

/// Remove nested skill candidates: if `A/SKILL.md` and `A/B/SKILL.md` both
/// exist, keep only the shallower `A/SKILL.md`.
pub(crate) fn prune_nested_skills(paths: &mut Vec<PathBuf>) {
    // Sort shallowest first so we encounter parents before children.
    paths.sort_by_key(|p| p.components().count());
    let mut kept: Vec<PathBuf> = Vec::new();
    for path in paths.drain(..) {
        let skill_dir = path.parent().unwrap();
        let nested = kept.iter().any(|k| {
            let k_dir = k.parent().unwrap();
            skill_dir.starts_with(k_dir)
        });
        if !nested {
            kept.push(path);
        }
    }
    *paths = kept;
}

/// Merge a SKILL.md's frontmatter `depends-on` (dependency atoms) and
/// `predicates` (function-call syntax) into one predicate set.
fn frontmatter_predicates(
    depends_on: Option<&str>,
    predicates: Option<&str>,
) -> Result<PredicateSet> {
    let depends_on = match depends_on {
        Some(s) => Some(crate::predicate::DependsOnList::parse(s)?),
        None => None,
    };
    let extra = match predicates {
        Some(s) => PredicateSet::parse(s)?,
        None => PredicateSet::default(),
    };
    Ok(PredicateSet::merged(depends_on, extra))
}

/// The frontmatter a synthesized plugin needs from a bare `SKILL.md` entry:
/// its declared `name` (the skill's identity, used as the plugin name) and
/// its `depends-on`/`predicates` hoisted into one set (so the plugin is gated
/// by them and the ordinary dormancy rule applies — a bare skill with no
/// dependency is dormant until `use`d). See
/// [`plugins::standalone_skill_manifest`](crate::plugins::standalone_skill_manifest).
pub(crate) fn standalone_skill_meta(skill_md: &Path) -> Result<(Option<String>, PredicateSet)> {
    let content = std::fs::read_to_string(skill_md)
        .with_context(|| format!("failed to read {}", skill_md.display()))?;
    let fm = parse_frontmatter(&content)
        .with_context(|| format!("failed to parse frontmatter in {}", skill_md.display()))?;
    let name = fm.fields.get("name").map(|n| {
        n.strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or(n)
            .to_string()
    });
    let predicates = frontmatter_predicates(fm.depends_on.as_deref(), fm.predicates.as_deref())?;
    Ok((name, predicates))
}

/// Load a single skill from a SKILL.md file.
///
/// A skill should have `depends-on` at either the skill level or
/// the group level (or both). If neither provides it, a warning is logged
/// but loading succeeds (the skill simply won't match any dependency query).
///
/// Workspace-member groups load leniently: the frontmatter (and its `name`
/// and `description` fields) is optional — `name` falls back to the skill
/// directory's name. Workspace skills are the maintainers' own informal
/// notes; the agentskills.io contract applies to published skills.
#[cfg(test)]
fn load_skill(
    skill_md_path: &Path,
    workspace_member: bool,
    group_predicates: &PredicateSet,
) -> Result<Skill> {
    load_skill_from(
        skill_md_path,
        skill_md_path,
        workspace_member,
        group_predicates,
    )
}

/// Read the canonical file while retaining the discovered bundle path used by
/// installation. These paths differ when `SKILL.md` itself is a symlink.
fn load_skill_from(
    canonical_skill_md: &Path,
    bundle_skill_md: &Path,
    workspace_member: bool,
    group_predicates: &PredicateSet,
) -> Result<Skill> {
    let content = std::fs::read_to_string(canonical_skill_md).context("failed to read")?;

    let fm = if workspace_member && !content.trim_start().starts_with("---") {
        RawFrontmatter {
            fields: BTreeMap::new(),
            depends_on: None,
            predicates: None,
        }
    } else {
        parse_frontmatter(&content).context("failed to parse frontmatter")?
    };

    let mut frontmatter = fm.fields;

    // Strip surrounding quotes from name if present (YAML scalars may be quoted)
    if let Some(name) = frontmatter.get_mut("name")
        && let Some(unquoted) = name.strip_prefix('"').and_then(|s| s.strip_suffix('"'))
    {
        *name = unquoted.to_string();
    }

    if workspace_member
        && !frontmatter.contains_key("name")
        && let Some(dir_name) = bundle_skill_md
            .parent()
            .and_then(|dir| dir.file_name())
            .and_then(|name| name.to_str())
    {
        frontmatter.insert("name".to_string(), dir_name.to_string());
    }

    let name = frontmatter
        .get("name")
        .context("SKILL.md frontmatter missing required `name` field")?;
    if !is_safe_dir_name(name) {
        bail!("SKILL.md `name` must be a plain directory name, got {name:?}");
    }

    // Validate description per agentskills.io spec
    // (https://agentskills.io/specification.md): required, non-empty, max 1024 chars.
    match frontmatter.get("description") {
        None if workspace_member => {}
        None => bail!("SKILL.md frontmatter missing required `description` field"),
        Some(desc) => {
            let trimmed_desc = desc.trim();
            if trimmed_desc.is_empty() {
                bail!("SKILL.md `description` must not be empty");
            }
            if trimmed_desc.len() > 1024 {
                bail!(
                    "SKILL.md `description` exceeds 1024 characters ({} chars)",
                    trimmed_desc.len()
                );
            }
        }
    }

    // Merge the skill-level `depends-on` (dependency atoms, OR-combined) with
    // the frontmatter `predicates` (function-call syntax) into one set, ANDed
    // with the plugin- and group-level sets at match time.
    let predicates = frontmatter_predicates(fm.depends_on.as_deref(), fm.predicates.as_deref())?;

    // Warn if no dependency is referenced at either level — the skill won't
    // match any dependency query, but we don't fail so a misconfigured plugin
    // can't bring down the tool.
    if !predicates.mentions_dep() && !group_predicates.mentions_dep() {
        tracing::warn!(
            skill = %name,
            "skill references no dependency in SKILL.md frontmatter or its plugin [[skills]] group"
        );
    }

    let skill = Skill {
        frontmatter,
        predicates,
        path: bundle_skill_md.to_path_buf(),
    };
    tracing::debug!(name = %skill.name(), path = %bundle_skill_md.display(), "skill loaded");
    Ok(skill)
}

pub(crate) fn is_safe_dir_name(name: &str) -> bool {
    // Windows trims trailing dots and spaces, so a name made only of them resolves to the parent.
    let only_dots_and_spaces = name.chars().all(|c| c == '.' || c == ' ');
    !only_dots_and_spaces && !name.contains(['/', '\\', ':'])
}

/// Evaluate the skill-level predicate set and collect the skill if it holds.
///
/// Plugin- and group-level predicates have already been evaluated by callers as
/// a pre-filter, so only the skill-level set is checked here.
fn collect_skill_applicable_to<D>(
    skill: Skill,
    origin_hash: String,
    plugin_index: usize,
    package_content: Option<PackageContent>,
    plugin: &Plugin,
    ctx: &mut PredicateContextImpl<D>,
    results: &mut Vec<SkillWithGroupContext>,
) where
    D: PredicateDependencies,
{
    if !skill.predicates.evaluate(ctx) {
        tracing::debug!(
            report = %crate::report::ReportEvent::SkillConsidered {
                skill: skill.name().to_string(),
                plugin: plugin.manifest.name.clone(),
                matched: false,
                reason: Some("skill-level predicates not satisfied".into()),
            },
        );
        return;
    }

    tracing::debug!(
        report = %crate::report::ReportEvent::SkillConsidered {
            skill: skill.name().to_string(),
            plugin: plugin.manifest.name.clone(),
            matched: true,
            reason: None,
        },
    );
    results.push(SkillWithGroupContext {
        skill,
        origin_hash,
        plugin_index,
        agent_plugin_root: plugin.agent_plugin_root.clone(),
        package_content,
    });
}

/// Raw frontmatter fields extracted from a SKILL.md file.
/// `depends-on` is comma-separated on a single line.
#[derive(Debug)]
struct RawFrontmatter {
    fields: BTreeMap<String, String>,
    /// Raw `depends-on` value (comma-separated predicate string).
    depends_on: Option<String>,
    /// Raw `predicates` value (comma-separated predicate expressions).
    predicates: Option<String>,
}

/// Parse SKILL.md content: extract the `---`-fenced frontmatter. The body
/// after the frontmatter is not retained — skills install by copying the
/// file, not by re-emitting parsed content.
fn parse_frontmatter(content: &str) -> Result<RawFrontmatter> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        bail!("SKILL.md must start with --- frontmatter fence");
    }

    let after_first_fence = &trimmed[3..];
    let after_first_fence = after_first_fence
        .strip_prefix('\n')
        .unwrap_or(after_first_fence);

    let end = after_first_fence
        .find("\n---")
        .context("no closing --- fence in frontmatter")?;

    let frontmatter_text = &after_first_fence[..end];

    let yaml: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(frontmatter_text).context("frontmatter is not valid YAML")?;
    let mapping = yaml
        .as_mapping()
        .context("SKILL.md frontmatter must be a YAML mapping")?;

    let mut fields = BTreeMap::new();
    let mut depends_on = None;
    let mut predicates = None;

    for (key, value) in mapping {
        let Some(key) = key.as_str() else {
            bail!("SKILL.md frontmatter keys must be strings");
        };

        if key == "crates" {
            bail!("the `crates` frontmatter field has been renamed; use `depends-on` instead");
        }

        let Some(value) = value.as_str() else {
            if matches!(key, "name" | "description" | "depends-on" | "predicates") {
                bail!("SKILL.md frontmatter field `{key}` must be a string");
            }
            continue;
        };

        match key {
            "depends-on" => depends_on = Some(value.to_string()),
            "predicates" => predicates = Some(value.to_string()),
            _ => {
                fields.insert(key.to_string(), value.to_string());
            }
        }
    }

    Ok(RawFrontmatter {
        fields,
        depends_on,
        predicates,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use std::fs;

    use crate::{
        pm::{ANY_VERSION, PackageId},
        predicate::Predicate,
    };

    /// Build a predicate set from dependency atoms (the `depends-on` field form).
    fn pred_set(s: &str) -> PredicateSet {
        PredicateSet::from_depends_on(s).unwrap()
    }

    /// Discovery follows directory links, so a skills tree that links back
    /// into itself must not be walked forever — and must not report the same
    /// `SKILL.md` once per turn around the loop.
    #[test]
    #[cfg(unix)]
    fn discovery_terminates_on_a_symlink_cycle() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = tmp.path().join("skills");
        fs::create_dir_all(skills.join("my-skill")).unwrap();
        fs::write(skills.join("my-skill/SKILL.md"), "# skill").unwrap();
        std::os::unix::fs::symlink("..", skills.join("my-skill/back")).unwrap();

        let mut found = Vec::new();
        find_skill_files_recursive(&skills, &mut found);

        assert_eq!(found, vec![skills.join("my-skill/SKILL.md")]);
    }

    /// Reaching one SKILL.md through a symlinked prefix must yield one origin.
    /// This is the macOS `/var` -> `/private/var` case: group discovery walks a
    /// canonicalized dir while the standalone walk uses the configured path
    /// verbatim, so without canonicalizing here the file installs twice.
    #[test]
    #[cfg(unix)]
    fn skill_origin_hash_is_stable_across_symlinked_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real");
        fs::create_dir_all(real.join("my-skill")).unwrap();
        let skill_md = real.join("my-skill/SKILL.md");
        fs::write(&skill_md, "# skill").unwrap();

        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let via_link = link.join("my-skill/SKILL.md");

        assert_ne!(skill_md, via_link, "paths should differ textually");
        assert_eq!(
            skill_origin_hash(&skill_md),
            skill_origin_hash(&via_link),
            "same file reached through a symlink must hash to one origin"
        );
    }

    fn write_test_skill(path: &Path, name: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            format!(
                "---\nname: {name}\ndescription: package-content test\ndepends-on: serde\n---\n"
            ),
        )
        .unwrap();
    }

    #[test]
    fn contained_package_proof_stays_bound_to_its_discovered_skill() {
        let root = tempfile::tempdir().unwrap();
        let skill_md = root.path().join("skills/contained/SKILL.md");
        write_test_skill(&skill_md, "contained");
        let content = crate::pm::resolved_content_for_test(root.path());

        let mut discovered = discover_skills_with_origin(
            &root.path().join("skills"),
            false,
            &PredicateSet::default(),
            SkillDirectoryOrigin::Package(&content),
        );
        let discovered = discovered.pop().unwrap().unwrap();
        let resolved = SkillWithGroupContext {
            origin_hash: discovered.origin_hash,
            skill: discovered.skill,
            plugin_index: 0,
            agent_plugin_root: None,
            package_content: discovered.package_content,
        };
        let naming = resolved.naming_view();

        assert_eq!(resolved.skill.path, skill_md);
        assert_eq!(naming.name(), "contained");
        assert_eq!(naming.package().unwrap().id().name, "test-package");
    }

    #[test]
    fn parent_segments_cannot_grant_package_content_authority() {
        let container = tempfile::tempdir().unwrap();
        let package = container.path().join("package");
        let outside = container.path().join("outside/SKILL.md");
        fs::create_dir_all(&package).unwrap();
        write_test_skill(&outside, "outside");
        let content = crate::pm::resolved_content_for_test(&package);

        let resolved = resolve_skill_file(
            &package.join("../outside/SKILL.md"),
            SkillDirectoryOrigin::Package(&content),
        )
        .unwrap();

        assert!(resolved.into_package_content().is_none());
    }

    #[test]
    fn unavailable_files_cannot_mint_package_content_proofs() {
        let root = tempfile::tempdir().unwrap();
        let content = crate::pm::resolved_content_for_test(root.path());

        let result = resolve_skill_file(
            &root.path().join("missing/SKILL.md"),
            SkillDirectoryOrigin::Package(&content),
        );

        assert!(result.is_err());
    }

    #[test]
    fn git_skill_inside_a_package_cache_remains_detached() {
        let root = tempfile::tempdir().unwrap();
        let git_cache = root.path().join("nested-git-cache");
        let skill_md = git_cache.join("detached/SKILL.md");
        write_test_skill(&skill_md, "detached");
        let content = crate::pm::resolved_content_for_test(root.path());
        let source = PluginSource::Git(format!("file://{}", git_cache.display()));
        let origin = skill_directory_origin(Some(&content), &source);

        let mut discovered =
            discover_skills_with_origin(&git_cache, false, &PredicateSet::default(), origin);
        let discovered = discovered.pop().unwrap().unwrap();
        let resolved = SkillWithGroupContext {
            origin_hash: discovered.origin_hash,
            skill: discovered.skill,
            plugin_index: 0,
            agent_plugin_root: None,
            package_content: discovered.package_content,
        };

        assert!(resolved.naming_view().package().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn directory_symlinks_outside_the_package_remain_detached() {
        use std::os::unix::fs::symlink;

        let container = tempfile::tempdir().unwrap();
        let package = container.path().join("package");
        let outside = container.path().join("outside/example/SKILL.md");
        fs::create_dir_all(&package).unwrap();
        write_test_skill(&outside, "outside");
        symlink(
            outside.parent().unwrap().parent().unwrap(),
            package.join("linked"),
        )
        .unwrap();
        let content = crate::pm::resolved_content_for_test(&package);

        let resolved = resolve_skill_file(
            &package.join("linked/example/SKILL.md"),
            SkillDirectoryOrigin::Package(&content),
        )
        .unwrap();

        assert!(resolved.into_package_content().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_skill_file_keeps_its_discovered_bundle_directory() {
        use std::os::unix::fs::symlink;

        let container = tempfile::tempdir().unwrap();
        let package = container.path().join("package");
        let bundle = package.join("skills/bundle");
        let shared = container.path().join("shared/instructions.md");
        write_test_skill(&shared, "linked-skill");
        fs::create_dir_all(&bundle).unwrap();
        fs::write(bundle.join("asset.txt"), "bundle asset").unwrap();
        symlink(&shared, bundle.join("SKILL.md")).unwrap();
        let content = crate::pm::resolved_content_for_test(&package);

        let mut discovered = discover_skills_with_origin(
            &package.join("skills"),
            false,
            &PredicateSet::default(),
            SkillDirectoryOrigin::Package(&content),
        );
        let discovered = discovered.pop().unwrap().unwrap();
        let resolved = SkillWithGroupContext {
            origin_hash: discovered.origin_hash,
            skill: discovered.skill,
            package_content: discovered.package_content,
        };

        assert_eq!(resolved.skill().path, bundle.join("SKILL.md"));
        assert_eq!(resolved.skill().path.parent(), Some(bundle.as_path()));
        assert_eq!(
            resolved.origin_hash(),
            canonical_skill_origin_hash(&dunce::canonicalize(shared).unwrap())
        );
        assert!(resolved.naming_view().package().is_none());
    }

    fn ctx(deps: &[crate::pm::PackageId]) -> PredicateContext<'_> {
        PredicateContext::new(deps)
    }

    fn ws(pairs: &[(&str, &str)]) -> Vec<crate::pm::PackageId> {
        pairs
            .iter()
            .map(|(n, ver)| crate::pm::PackageId::new(crate::pm::CARGO_PM, *n, *ver))
            .collect()
    }

    // --- Frontmatter parsing ---

    #[test]
    fn parse_frontmatter_basic() {
        let content = indoc! {"
            ---
            name: my-skill
            description: A test skill
            depends-on: serde
            ---

            # Body content

            Some instructions here.
        "};
        let fm = parse_frontmatter(content).unwrap();
        assert_eq!(fm.fields.get("name").unwrap(), "my-skill");
        assert_eq!(fm.fields.get("description").unwrap(), "A test skill");
        assert_eq!(fm.depends_on.as_deref(), Some("serde"));
    }

    #[test]
    fn parse_frontmatter_comma_separated_depends_on() {
        let content = indoc! {"
            ---
            name: multi
            depends-on: serde, serde_json>=1.0, toml
            ---

            Body.
        "};
        let fm = parse_frontmatter(content).unwrap();
        assert_eq!(
            fm.depends_on.as_deref(),
            Some("serde, serde_json>=1.0, toml")
        );
    }

    #[test]
    fn parse_frontmatter_rejects_renamed_crates_field() {
        let content = indoc! {"
            ---
            name: old-spelling
            description: Old spelling
            crates: serde
            ---

            Body.
        "};
        let err = parse_frontmatter(content).unwrap_err();
        assert!(
            err.to_string().contains("use `depends-on` instead"),
            "expected migration hint, got: {err}"
        );
    }

    #[test]
    fn parse_frontmatter_rejects_invalid_yaml_description() {
        let content = indoc! {"
            ---
            name: rust-best-practice
            description: [Critical] Best practice for Rust coding.
            ---

            Body.
        "};

        let err = parse_frontmatter(content).unwrap_err();
        assert!(
            err.to_string().contains("frontmatter is not valid YAML"),
            "expected YAML parse error, got: {err}"
        );
    }

    #[test]
    fn parse_frontmatter_rejects_non_string_description() {
        let content = indoc! {"
            ---
            name: rust-best-practice
            description: [Critical]
            ---

            Body.
        "};

        let err = parse_frontmatter(content).unwrap_err();
        assert!(
            err.to_string()
                .contains("frontmatter field `description` must be a string"),
            "expected scalar string error, got: {err}"
        );
    }

    #[test]
    fn parse_frontmatter_accepts_non_string_values_in_other_fields() {
        let content = indoc! {"
            ---
            name: my-skill
            description: A test skill
            depends-on: serde
            allowed-tools: [Read, Grep]
            disable-model-invocation: true
            metadata:
              author: someone
              tags: [a, b]
            ---

            Body.
        "};

        let fm = parse_frontmatter(content).unwrap();
        assert_eq!(fm.fields.get("name").unwrap(), "my-skill");
        assert_eq!(fm.fields.get("description").unwrap(), "A test skill");
        assert_eq!(fm.depends_on.as_deref(), Some("serde"));
    }

    #[test]
    fn parse_frontmatter_no_fence() {
        let content = "# Just markdown\n\nNo frontmatter here.\n";
        assert!(parse_frontmatter(content).is_err());
    }

    #[test]
    fn parse_frontmatter_no_closing_fence() {
        let content = "---\nname: broken\n";
        assert!(parse_frontmatter(content).is_err());
    }

    // --- Skill loading ---

    #[test]
    fn load_skill_from_frontmatter() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_md = tmp.path().join("SKILL.md");
        fs::write(
            &skill_md,
            indoc! {"
                ---
                name: test-skill
                description: Test
                depends-on: serde
                ---

                Use serde like this.
            "},
        )
        .unwrap();

        let skill = load_skill(&skill_md, false, &PredicateSet::default()).unwrap();

        assert_eq!(skill.frontmatter.get("name").unwrap(), "test-skill");
        assert!(skill.predicates.references_dep("serde"));
    }

    #[test]
    fn load_skill_comma_separated_crates() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_md = tmp.path().join("SKILL.md");
        fs::write(
            &skill_md,
            indoc! {"
                ---
                name: multi-crate
                description: Multi-crate skill
                depends-on: serde, tokio>=1.0
                ---

                Body.
            "},
        )
        .unwrap();

        let skill = load_skill(&skill_md, false, &PredicateSet::default()).unwrap();
        assert!(skill.predicates.references_dep("serde"));
        assert!(skill.predicates.references_dep("tokio"));
    }

    #[test]
    fn load_skill_inherits_toml_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_md = tmp.path().join("SKILL.md");
        fs::write(
            &skill_md,
            indoc! {"
                ---
                name: inherited
                description: Inherits crates from TOML
                ---

                Body.
            "},
        )
        .unwrap();

        let skill = load_skill(&skill_md, false, &pred_set("tokio")).unwrap();

        // Skill has no crates in frontmatter, so it's empty at skill level.
        // The plugin default provides the crates scope.
        assert!(skill.predicates.is_empty());
    }

    #[test]
    fn load_skill_frontmatter_specializes_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_md = tmp.path().join("SKILL.md");
        fs::write(
            &skill_md,
            indoc! {"
                ---
                name: override
                description: Override skill
                depends-on: serde
                ---

                Body.
            "},
        )
        .unwrap();

        let skill = load_skill(&skill_md, false, &pred_set("tokio")).unwrap();

        // Skill-level crates specializes (ANDs with) plugin defaults
        assert!(skill.predicates.references_dep("serde"));
        assert!(!skill.predicates.references_dep("tokio"));
    }

    #[test]
    fn load_skill_missing_crates_warns_but_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_md = tmp.path().join("SKILL.md");
        fs::write(
            &skill_md,
            indoc! {"
                ---
                name: no-depends-on
                description: Missing depends-on
                ---

                Body.
            "},
        )
        .unwrap();

        // No longer an error — just a warning. The skill loads but won't match anything.
        let skill = load_skill(&skill_md, false, &PredicateSet::default()).unwrap();
        assert!(skill.predicates.is_empty());
    }

    #[test]
    fn load_skill_ok_when_plugin_provides_crates() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_md = tmp.path().join("SKILL.md");
        fs::write(
            &skill_md,
            indoc! {"
                ---
                name: no-own-crates
                description: PluginManifest provides crates
                ---

                Body.
            "},
        )
        .unwrap();

        // Plugin defaults provide crates, so the skill doesn't need its own
        let skill = load_skill(&skill_md, false, &pred_set("serde")).unwrap();
        assert!(skill.predicates.is_empty()); // skill-level is empty
        assert_eq!(skill.frontmatter.get("name").unwrap(), "no-own-crates");
    }

    #[test]
    fn load_skill_rejects_names_that_are_not_plain_directory_names() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_md = tmp.path().join("SKILL.md");
        for name in [
            "",
            ".",
            "..",
            "...",
            ".. ",
            "../escaped",
            "nested/skill",
            "/tmp/escaped",
            "..\\escaped",
            "C:escaped",
        ] {
            fs::write(
                &skill_md,
                format!("---\nname: {name:?}\ndescription: d\ndepends-on: serde\n---\n"),
            )
            .unwrap();
            let err = load_skill(&skill_md, false, &PredicateSet::default()).unwrap_err();
            assert!(
                err.to_string().contains("plain directory name"),
                "{name:?}: {err}"
            );
        }
    }

    // --- Bare SKILL.md loading (see plugins::standalone_skill_manifest
    // for how a bare skill becomes a plugin) ---

    #[test]
    fn bare_skill_is_self_contained() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("my-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            indoc! {"
                ---
                name: my-standalone
                description: A standalone skill
                depends-on: serde
                ---

                Standalone body.
            "},
        )
        .unwrap();

        let skill =
            load_skill(&skill_dir.join("SKILL.md"), false, &PredicateSet::default()).unwrap();
        assert_eq!(skill.name(), "my-standalone");
        assert!(skill.predicates.references_dep("serde"));
    }

    #[test]
    fn workspace_group_skill_needs_no_frontmatter() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("release-notes");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "Plain maintainer notes.\n").unwrap();

        let skill =
            load_skill(&skill_dir.join("SKILL.md"), true, &PredicateSet::default()).unwrap();
        assert_eq!(skill.name(), "release-notes");
        assert!(!skill.frontmatter.contains_key("description"));

        // Registry groups keep the agentskills.io contract.
        let err =
            load_skill(&skill_dir.join("SKILL.md"), false, &PredicateSet::default()).unwrap_err();
        assert!(err.to_string().contains("frontmatter"), "{err}");
    }

    #[test]
    fn workspace_group_skill_frontmatter_fields_stay_optional() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("style");
        fs::create_dir_all(&skill_dir).unwrap();
        // Frontmatter present, but neither name nor description: the name
        // falls back to the directory, other fields still parse.
        fs::write(
            skill_dir.join("SKILL.md"),
            indoc! {"
                ---
                depends-on: serde
                ---

                Body.
            "},
        )
        .unwrap();

        let skill =
            load_skill(&skill_dir.join("SKILL.md"), true, &PredicateSet::default()).unwrap();
        assert_eq!(skill.name(), "style");
        assert!(skill.predicates.references_dep("serde"));

        // An explicit name still wins over the directory fallback.
        fs::write(
            skill_dir.join("SKILL.md"),
            indoc! {"
                ---
                name: explicit
                ---

                Body.
            "},
        )
        .unwrap();
        let skill =
            load_skill(&skill_dir.join("SKILL.md"), true, &PredicateSet::default()).unwrap();
        assert_eq!(skill.name(), "explicit");
    }

    #[test]
    fn validate_standalone_skill_bad_depends_on() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("bad-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            indoc! {"
                ---
                name: bad
                description: Bad depends-on skill
                depends-on: \">=not_valid!!\"
                ---

                Body.
            "},
        )
        .unwrap();

        let err =
            load_skill(&skill_dir.join("SKILL.md"), false, &PredicateSet::default()).unwrap_err();
        assert!(
            err.to_string().contains("depends-on predicate"),
            "expected parse error, got: {err}"
        );
    }

    // --- Multi-level crate filtering tests ---

    #[tokio::test]
    async fn test_plugin_level_filtering_blocks_skills() {
        use crate::plugins::{Plugin, PluginManifest, PluginRegistry, PluginSource, SkillGroup};
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let sym = crate::config::Symposium::from_dir(tmp.path());

        // Create a plugin that only applies to "other-crate"
        let plugin = PluginManifest {
            name: "other-crate-plugin".to_string(),
            version: None,
            description: None,
            predicates: pred_set("other-crate"),
            hooks: vec![],
            skills: vec![SkillGroup {
                predicates: pred_set("serde"), // Group targets serde
                source: PluginSource::Path(PathBuf::from("skills")),
                source_label: None,
                workspace_member: false,
            }],
            mcp_servers: vec![],
            installations: Vec::new(),
            subcommands: BTreeMap::new(),
            custom_predicates: vec![],
            chained: vec![],
            requires_use: false,
        };

        let registry = PluginRegistry {
            plugins: vec![Plugin {
                canonical: PackageId::new("test", &plugin.name, ANY_VERSION),
                resolved_content: None,
                manifest: plugin,
                workspace_member: false,
                agent_plugin_root: None,
                root: PathBuf::new(),
            }],
            warnings: vec![],
            unreadable_registries: vec![],
            custom_predicates: crate::plugins::CustomPredicateRegistry::default(),
        };

        // Query for serde - should find no skills because plugin doesn't apply
        let workspace_crates = vec![crate::test_utils::workspace_crate(
            "serde".to_string(),
            semver::Version::new(1, 0, 0),
            None,
        )];
        let skills = skills_applicable_to(
            &sym,
            &registry,
            &crate::pm::Workspace::fixture(std::path::PathBuf::new(), workspace_crates),
            None,
            std::collections::HashMap::new(),
            UpdateLevel::None,
        )
        .await;

        assert!(
            skills.is_empty(),
            "Plugin should be filtered out at plugin level"
        );
    }

    #[tokio::test]
    async fn test_group_level_filtering_blocks_skills() {
        use crate::plugins::{Plugin, PluginManifest, PluginRegistry, PluginSource, SkillGroup};
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let sym = crate::config::Symposium::from_dir(tmp.path());

        // Create a plugin with wildcard that has a group targeting different crate
        let plugin = PluginManifest {
            name: "wildcard-plugin".to_string(),
            version: None,
            description: None,
            predicates: pred_set("*"), // Plugin applies to all
            hooks: vec![],
            skills: vec![SkillGroup {
                predicates: pred_set("other-crate"), // But group targets other-crate
                source: PluginSource::Path(PathBuf::from("skills")),
                source_label: None,
                workspace_member: false,
            }],
            mcp_servers: vec![],
            installations: Vec::new(),
            subcommands: BTreeMap::new(),
            custom_predicates: vec![],
            chained: vec![],
            requires_use: false,
        };

        let registry = PluginRegistry {
            plugins: vec![Plugin {
                canonical: PackageId::new("test", &plugin.name, ANY_VERSION),
                resolved_content: None,
                manifest: plugin,
                workspace_member: false,
                agent_plugin_root: None,
                root: PathBuf::new(),
            }],
            warnings: vec![],
            unreadable_registries: vec![],
            custom_predicates: crate::plugins::CustomPredicateRegistry::default(),
        };

        // Query for serde - should find no skills because group doesn't match
        let workspace_crates = vec![crate::test_utils::workspace_crate(
            "serde".to_string(),
            semver::Version::new(1, 0, 0),
            None,
        )];
        let skills = skills_applicable_to(
            &sym,
            &registry,
            &crate::pm::Workspace::fixture(std::path::PathBuf::new(), workspace_crates),
            None,
            std::collections::HashMap::new(),
            UpdateLevel::None,
        )
        .await;

        assert!(
            skills.is_empty(),
            "Skills should be filtered out at group level"
        );
    }

    #[tokio::test]
    async fn test_all_levels_match_allows_skills() {
        use crate::plugins::{Plugin, PluginManifest, PluginRegistry, PluginSource, SkillGroup};
        use std::fs;
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let sym = crate::config::Symposium::from_dir(tmp.path());

        // Create skill directory and file
        let skill_dir = tmp.path().join("serde-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            indoc! {"
                ---
                name: serde-basics
                description: Basic serde usage
                depends-on: serde
                ---

                Use derive macros.
            "},
        )
        .unwrap();

        // Create a plugin where all levels match serde
        let plugin = PluginManifest {
            name: "serde-plugin".to_string(),
            version: None,
            description: None,
            predicates: pred_set("serde"), // Plugin targets serde
            hooks: vec![],
            skills: vec![SkillGroup {
                predicates: pred_set("serde"), // Group also targets serde
                source: PluginSource::Path(skill_dir.to_path_buf()),
                source_label: None,
                workspace_member: false,
            }],
            mcp_servers: vec![],
            installations: Vec::new(),
            subcommands: BTreeMap::new(),
            custom_predicates: vec![],
            chained: vec![],
            requires_use: false,
        };

        let registry = PluginRegistry {
            plugins: vec![Plugin {
                canonical: PackageId::new("test", &plugin.name, ANY_VERSION),
                resolved_content: None,
                manifest: plugin,
                workspace_member: false,
                agent_plugin_root: None,
                root: PathBuf::new(),
            }],
            warnings: vec![],
            unreadable_registries: vec![],
            custom_predicates: crate::plugins::CustomPredicateRegistry::default(),
        };

        let workspace_crates = vec![crate::test_utils::workspace_crate(
            "serde".to_string(),
            semver::Version::new(1, 0, 0),
            None,
        )];
        let skills = skills_applicable_to(
            &sym,
            &registry,
            &crate::pm::Workspace::fixture(std::path::PathBuf::new(), workspace_crates),
            None,
            std::collections::HashMap::new(),
            UpdateLevel::None,
        )
        .await;

        assert_eq!(
            skills.len(),
            1,
            "Should find one skill when all levels match"
        );
        assert_eq!(skills[0].skill.name(), "serde-basics");
    }

    #[tokio::test]
    async fn predicate_failure_filters_skill() {
        use crate::plugins::{Plugin, PluginManifest, PluginRegistry, PluginSource, SkillGroup};
        use std::fs;
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let sym = crate::config::Symposium::from_dir(tmp.path());

        let skill_dir = tmp.path().join("serde-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            indoc! {"
                ---
                name: serde-basics
                description: Basic serde usage
                depends-on: serde
                ---

                Use derive macros.
            "},
        )
        .unwrap();

        // Plugin matches by crates, but its shell predicate fails.
        let plugin = PluginManifest {
            name: "p".into(),
            version: None,
            description: None,
            predicates: PredicateSet {
                predicates: vec![
                    Predicate::DependsOn(crate::predicate::Dependency::named("serde")),
                    Predicate::Shell("false".into()),
                ],
            },
            hooks: vec![],
            skills: vec![SkillGroup {
                predicates: pred_set("serde"),
                source: PluginSource::Path(skill_dir.to_path_buf()),
                source_label: None,
                workspace_member: false,
            }],
            mcp_servers: vec![],
            installations: Vec::new(),
            subcommands: Default::default(),
            custom_predicates: vec![],
            chained: vec![],
            requires_use: false,
        };

        let registry = PluginRegistry {
            plugins: vec![Plugin {
                canonical: PackageId::new("test", &plugin.name, ANY_VERSION),
                resolved_content: None,
                manifest: plugin,
                workspace_member: false,
                agent_plugin_root: None,
                root: PathBuf::new(),
            }],
            warnings: vec![],
            unreadable_registries: vec![],
            custom_predicates: crate::plugins::CustomPredicateRegistry::default(),
        };

        let workspace = vec![crate::test_utils::workspace_crate(
            "serde",
            semver::Version::new(1, 0, 0),
            None,
        )];
        let skills = skills_applicable_to(
            &sym,
            &registry,
            &crate::pm::Workspace::fixture(std::path::PathBuf::new(), workspace),
            None,
            std::collections::HashMap::new(),
            UpdateLevel::None,
        )
        .await;
        assert!(
            skills.is_empty(),
            "plugin predicate=false should filter out skills"
        );
    }

    #[tokio::test]
    async fn predicate_pass_allows_skill() {
        use crate::plugins::{Plugin, PluginManifest, PluginRegistry, PluginSource, SkillGroup};
        use std::fs;
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let sym = crate::config::Symposium::from_dir(tmp.path());

        let skill_dir = tmp.path().join("serde-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            indoc! {"
                ---
                name: serde-basics
                description: Basic serde usage
                depends-on: serde
                ---

                Body.
            "},
        )
        .unwrap();

        let plugin = PluginManifest {
            name: "p".into(),
            version: None,
            description: None,
            predicates: PredicateSet {
                predicates: vec![
                    Predicate::DependsOn(crate::predicate::Dependency::named("serde")),
                    Predicate::Shell("true".into()),
                ],
            },
            hooks: vec![],
            skills: vec![SkillGroup {
                predicates: PredicateSet {
                    predicates: vec![
                        Predicate::DependsOn(crate::predicate::Dependency::named("serde")),
                        Predicate::Shell("true".into()),
                    ],
                },
                source: PluginSource::Path(skill_dir.to_path_buf()),
                source_label: None,
                workspace_member: false,
            }],
            mcp_servers: vec![],
            installations: Vec::new(),
            subcommands: Default::default(),
            custom_predicates: vec![],
            chained: vec![],
            requires_use: false,
        };

        let registry = PluginRegistry {
            plugins: vec![Plugin {
                canonical: PackageId::new("test", &plugin.name, ANY_VERSION),
                resolved_content: None,
                manifest: plugin,
                workspace_member: false,
                agent_plugin_root: None,
                root: PathBuf::new(),
            }],
            warnings: vec![],
            unreadable_registries: vec![],
            custom_predicates: crate::plugins::CustomPredicateRegistry::default(),
        };

        let workspace = vec![crate::test_utils::workspace_crate(
            "serde",
            semver::Version::new(1, 0, 0),
            None,
        )];
        let skills = skills_applicable_to(
            &sym,
            &registry,
            &crate::pm::Workspace::fixture(std::path::PathBuf::new(), workspace),
            None,
            std::collections::HashMap::new(),
            UpdateLevel::None,
        )
        .await;
        assert_eq!(skills.len(), 1);
    }

    #[test]
    fn skill_frontmatter_parses_predicates() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_md = tmp.path().join("SKILL.md");
        std::fs::write(
            &skill_md,
            indoc! {"
                ---
                name: with-env
                description: A skill with runtime predicates
                depends-on: serde
                predicates: shell(command -v rg), path_exists(Cargo.toml)
                ---

                Body.
            "},
        )
        .unwrap();

        let skill = load_skill(&skill_md, false, &PredicateSet::default()).unwrap();
        // `depends-on: serde` lowers to a leading `depends-on(serde)`, then the two
        // function-call predicates.
        assert_eq!(
            skill.predicates.predicates,
            vec![
                Predicate::DependsOn(crate::predicate::Dependency::named("serde")),
                Predicate::Shell("command -v rg".into()),
                Predicate::PathExists("Cargo.toml".into()),
            ]
        );
    }

    #[test]
    fn validate_standalone_skill_missing_name() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("bad-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            indoc! {"
                ---
                depends-on: serde
                ---

                Body.
            "},
        )
        .unwrap();

        let err =
            load_skill(&skill_dir.join("SKILL.md"), false, &PredicateSet::default()).unwrap_err();
        assert!(
            err.to_string().contains("missing required `name` field"),
            "expected missing name error, got: {err}"
        );
    }

    /// A bare `SKILL.md` directory, modelled as its synthesized plugin would be
    /// (name from the directory, one `source.path = "."` group gated on the
    /// skill's own `depends-on`), contributes its skill through the ordinary
    /// plugin path.
    #[tokio::test]
    async fn bare_skill_plugin_contributes_its_skill() {
        use crate::plugins::{Plugin, PluginManifest, PluginRegistry, PluginSource, SkillGroup};

        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("my-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            indoc! {"
                ---
                name: standalone-serde
                description: Standalone serde skill
                depends-on: serde
                ---

                Body.
            "},
        )
        .unwrap();

        let plugin = PluginManifest {
            name: "my-skill".to_string(),
            version: None,
            description: None,
            predicates: pred_set("serde"),
            installations: vec![],
            hooks: vec![],
            skills: vec![SkillGroup {
                predicates: PredicateSet::default(),
                // A PM returns absolute skill dirs; the bare-skill group's "."
                // resolves to the skill's own directory.
                source: PluginSource::Path(skill_dir.clone()),
                source_label: None,
                workspace_member: false,
            }],
            mcp_servers: vec![],
            subcommands: Default::default(),
            custom_predicates: vec![],
            chained: vec![],
            requires_use: false,
        };
        let registry = PluginRegistry {
            plugins: vec![Plugin {
                canonical: crate::pm::PackageId::any_version("recs", "my-skill"),
                resolved_content: None,
                manifest: plugin,
                workspace_member: false,
                agent_plugin_root: None,
                root: PathBuf::new(),
            }],
            warnings: vec![],
            unreadable_registries: vec![],
            custom_predicates: crate::plugins::CustomPredicateRegistry::default(),
        };

        let sym = crate::config::Symposium::from_dir(tmp.path());
        let workspace = vec![crate::test_utils::workspace_crate(
            "serde".to_string(),
            semver::Version::new(1, 0, 0),
            None,
        )];
        let results = skills_applicable_to(
            &sym,
            &registry,
            &crate::pm::Workspace::fixture(std::path::PathBuf::new(), workspace),
            None,
            std::collections::HashMap::new(),
            UpdateLevel::None,
        )
        .await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].skill.name(), "standalone-serde");
        assert!(results[0].skill.predicates.references_dep("serde"));
    }

    // --- Discovery ---

    #[test]
    fn discover_skills_in_plugin_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let plugin_dir = tmp.path();

        // Create skills/my-skill/SKILL.md
        let skill_dir = plugin_dir.join("skills").join("my-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            indoc! {"
                ---
                name: my-skill
                description: A discovered skill
                depends-on: serde
                ---

                Discovered body.
            "},
        )
        .unwrap();

        let skills = discover_skills(&plugin_dir.join("skills"), false, &PredicateSet::default());

        assert_eq!(skills.len(), 1);
        let skill = skills.into_iter().next().unwrap().unwrap();
        assert_eq!(skill.frontmatter.get("name").unwrap(), "my-skill");
    }

    #[test]
    fn discover_skills_in_subdirectory() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        // Create a skill nested two levels deep: group/sub/SKILL.md
        let skill_dir = root.join("group").join("sub");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            indoc! {"
                ---
                name: nested-skill
                description: Nested skill
                depends-on: tokio
                ---

                Nested body.
            "},
        )
        .unwrap();

        let skills = discover_skills(root, false, &PredicateSet::default());

        assert_eq!(skills.len(), 1);
        let skill = skills.into_iter().next().unwrap().unwrap();
        assert_eq!(skill.frontmatter.get("name").unwrap(), "nested-skill");
    }

    #[test]
    fn discover_skills_prunes_nested() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        // Create a shallow skill: alpha/SKILL.md
        let shallow = root.join("alpha");
        fs::create_dir_all(&shallow).unwrap();
        fs::write(
            shallow.join("SKILL.md"),
            indoc! {"
                ---
                name: shallow
                description: Shallow skill
                depends-on: serde
                ---

                Shallow.
            "},
        )
        .unwrap();

        // Create a nested skill inside alpha: alpha/beta/SKILL.md
        let nested = root.join("alpha").join("beta");
        fs::create_dir_all(&nested).unwrap();
        fs::write(
            nested.join("SKILL.md"),
            indoc! {"
                ---
                name: nested
                description: Nested skill
                depends-on: serde
                ---

                Nested.
            "},
        )
        .unwrap();

        // Also create a sibling skill: gamma/SKILL.md (should be kept)
        let sibling = root.join("gamma");
        fs::create_dir_all(&sibling).unwrap();
        fs::write(
            sibling.join("SKILL.md"),
            indoc! {"
                ---
                name: sibling
                description: Sibling skill
                depends-on: tokio
                ---

                Sibling.
            "},
        )
        .unwrap();

        let skills = discover_skills(root, false, &PredicateSet::default());

        // Should find shallow + sibling, but NOT nested (pruned by shallow)
        let names: Vec<String> = skills
            .into_iter()
            .map(|r| r.unwrap().frontmatter.get("name").unwrap().clone())
            .collect();
        assert!(names.contains(&"shallow".to_string()));
        assert!(names.contains(&"sibling".to_string()));
        assert!(!names.contains(&"nested".to_string()));
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn discover_skills_no_skills_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = discover_skills(tmp.path(), false, &PredicateSet::default());
        assert!(skills.is_empty());
    }

    // --- AND composition across levels (plugin ∧ group ∧ skill) ---

    /// Each level's `depends-on` lowers to one predicate set; the skill applies when
    /// every level's set holds (AND across levels).
    fn applies(levels: &[&str], deps: &[crate::pm::PackageId]) -> bool {
        levels
            .iter()
            .all(|spec| pred_set(spec).evaluate(&mut ctx(deps)))
    }

    #[test]
    fn and_across_levels_all_satisfied() {
        let w = ws(&[("serde", "1.0.0"), ("tokio", "1.0.0")]);
        assert!(applies(&["serde", "tokio"], &w));
    }

    #[test]
    fn and_across_levels_one_missing() {
        let w = ws(&[("serde", "1.0.0")]);
        assert!(!applies(&["serde", "tokio"], &w));
    }

    #[test]
    fn and_across_levels_empty_is_vacuously_true() {
        let w = ws(&[("serde", "1.0.0")]);
        assert!(applies(&[], &w));
    }

    #[test]
    fn wildcard_level_matches_any() {
        let w = ws(&[("serde", "1.0.0")]);
        assert!(applies(&["*", "serde"], &w));
    }
}
