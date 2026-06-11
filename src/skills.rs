//! Skill model, frontmatter parsing, discovery, and crate advice output.
//!
//! Skills follow the [agentskills.io](https://agentskills.io/specification.md) format
//! and live inside plugin directories under `skills/*/SKILL.md`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::config::Symposium;
use crate::plugins::{ParsedPlugin, PluginRegistry, PluginSource, SkillGroup};
use crate::predicate::{self, PredicateContext, PredicateSet};

fn source_display(source: &PluginSource) -> String {
    match source {
        PluginSource::None => "none".into(),
        PluginSource::Path(p) => format!("path:{}", p.display()),
        PluginSource::Git(url) => format!("git:{url}"),
        PluginSource::Crate => "crate".into(),
    }
}

/// A parsed skill from a SKILL.md file.
#[derive(Debug, Clone)]
pub struct Skill {
    /// Frontmatter fields as key-value pairs (name, description, license, etc.).
    pub frontmatter: BTreeMap<String, String>,
    /// Skill-level activation predicates: the frontmatter `crates` (lowered to
    /// `any(crate(...))`) merged with `predicates`. ANDed with the plugin- and
    /// group-level sets.
    pub predicates: PredicateSet,
    /// The body content (everything after frontmatter).
    pub body: String,
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

/// Where a skill came from.
///
/// Two skills with equal `SkillOrigin` and equal name install to the same
/// directory and dedupe; everything else installs independently. This lets
/// two plugins that legitimately both supply a same-named skill from
/// different sources coexist, while collapsing the case where two plugins
/// just happen to point at the same logical bundle.
///
/// What matters for identity is *where the skill bytes live*, not which
/// plugin manifest pointed at them. Two plugins in the same registry
/// source that both reference the same on-disk skill therefore dedupe.
///
/// - `Crate { name, version }` — the skill came from walking the source tree
///   of a specific crate version (`source = "crate"`).
///   Identity is `(name, version)` only: two plugins targeting the same
///   crate version produce the same logical skills regardless of which
///   plugin pointed at them.
/// - `Git { source, commit_sha, skill_path }` — the skill came from a
///   `source.git` group. Identity is `(source, commit_sha, skill_path)`:
///   `source` is the parsed [`GitSource`](symposium_install::git::GitSource)
///   which normalizes equivalent URLs to the same value, the commit SHA
///   is the resolved hash that was actually fetched, and `skill_path` is
///   the SKILL.md's path within the repo tree. Two URLs that refer to
///   the same repository normalize to the same `GitSource` and therefore
///   deduplicate. Different SKILL.md files within one repo stay distinct.
/// - `Source { source_name, skill_path }` — the skill came from a
///   plugin's `source.path` group, or from a standalone `SKILL.md` in
///   a registry source. `source_name` is the registry source's
///   display name (e.g. `"user-plugins"`); `skill_path` is the
///   SKILL.md's parent directory relative to the source root, with
///   forward slashes. Two plugins in the same source pointing at the
///   same on-disk skill bundle therefore produce the same origin.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
pub enum SkillOrigin {
    Crate {
        name: String,
        version: semver::Version,
    },
    Git {
        /// The parsed git source (carries normalized repo identity).
        source: symposium_install::git::GitSource,
        /// Commit SHA actually fetched (from the cache meta).
        commit_sha: String,
        /// SKILL.md's path within the repo tree, normalized to forward
        /// slashes.
        skill_path: String,
    },
    Source {
        /// Registry source's display name from the user config.
        source_name: String,
        /// SKILL.md's parent directory, relative to the source root,
        /// normalized to forward slashes.
        skill_path: String,
    },
}

impl SkillOrigin {
    /// Short, readable disambiguator embedded into install paths.
    ///
    /// For `Crate` we expand to `<name>-<version>` — readable on disk
    /// and already collision-free within a workspace. The other
    /// variants don't have a clean printable form, so we hash:
    /// 8-hex-char prefix of SHA-256 over the JSON-serialized origin.
    /// Hash collisions there would manifest as a name clash at install
    /// time, not silent data loss.
    pub fn short_hash(&self) -> String {
        match self {
            SkillOrigin::Crate { name, version } => format!("{name}-{version}"),
            SkillOrigin::Git { .. } | SkillOrigin::Source { .. } => {
                use sha2::{Digest, Sha256};
                let bytes = serde_json::to_vec(self).expect("SkillOrigin always serializes");
                let digest = Sha256::digest(&bytes);
                let mut out = String::with_capacity(8);
                for byte in &digest[..4] {
                    use std::fmt::Write;
                    write!(out, "{byte:02x}").unwrap();
                }
                out
            }
        }
    }
}

/// An applicable skill paired with the origin it was discovered through.
///
/// The plugin-, group-, and skill-level predicate sets are all evaluated during
/// collection; only skills whose every level holds end up here.
pub struct SkillWithGroupContext {
    pub skill: Skill,
    /// Where the skill was discovered. Drives install-path disambiguation
    /// and dedup at sync time.
    pub origin: SkillOrigin,
}

/// Resolve all applicable skills from the registry.
///
/// Resolve all skills applicable to the given crates.
///
/// `for_crates` is the set of crate name/version pairs to match against.
/// For `crate --list`, this is the full workspace deps.
/// For `crate <name>`, this is a single-element slice with the resolved crate.
pub async fn skills_applicable_to(
    sym: &Symposium,
    registry: &PluginRegistry,
    workspace_crates: &[symposium_sdk::workspace::WorkspaceCrate],
    custom_predicate_entries: std::collections::HashMap<String, predicate::ResolvedPredicateEntry>,
) -> Vec<SkillWithGroupContext> {
    let mut results = Vec::new();

    let for_crates = crate::crate_sources::crate_pairs(workspace_crates);
    let mut ctx = PredicateContext::with_custom_predicates(&for_crates, custom_predicate_entries);

    // Skills from plugin manifests. We iterate these separately
    // because we lazily load skill groups, so there
    // is extra logic.
    for parsed in &registry.plugins {
        let plugin = &parsed.plugin;
        // Plugin-level predicates gate everything below. Evaluated before
        // group fetching to avoid wasted work.
        if !plugin.predicates.evaluate(&mut ctx) {
            tracing::debug!(
                report = %crate::report::ReportEvent::PluginConsidered {
                    plugin: plugin.name.clone(),
                    matched: false,
                    reason: Some("plugin-level predicates not satisfied".into()),
                },
            );
            continue;
        }

        tracing::debug!(
            report = %crate::report::ReportEvent::PluginConsidered {
                plugin: plugin.name.clone(),
                matched: true,
                reason: None,
            },
        );

        for group in &plugin.skills {
            let skills =
                load_skills_for_group(sym, parsed, group, workspace_crates, &mut ctx).await;
            for (skill, origin) in skills {
                collect_skill_applicable_to(skill, origin, &plugin.name, &mut ctx, &mut results);
            }
        }
    }

    // Standalone skills already carry their own `SkillOrigin` (computed
    // from the plugin source name and the skill's path within that source).
    if !registry.standalone_skills.is_empty() {
        tracing::debug!(
            report = %crate::report::ReportEvent::PluginConsidered {
                plugin: "(standalone skills)".into(),
                matched: true,
                reason: None,
            },
        );
    }
    for entry in &registry.standalone_skills {
        collect_skill_applicable_to(
            entry.skill.clone(),
            entry.origin.clone(),
            "(standalone skills)",
            &mut ctx,
            &mut results,
        );
    }

    results
}

/// Discover and load skills for a group, applying pre-fetch filtering.
///
/// Checks group-level `crates` predicates against `for_crates` before
/// fetching git sources, to avoid unnecessary downloads. Each returned
/// skill is paired with the `SkillOrigin` it was discovered through:
///
/// - `CratePath` → `SkillOrigin::Crate { name, version }`, one per
///   matched crate (canonical name/version from the fetch result).
/// - `Git` → `SkillOrigin::Git { source, commit_sha, skill_path }`, one
///   per discovered SKILL.md (path varies per skill within the group).
/// - `Path` → `SkillOrigin::Source { source_name, skill_path }`, with
///   `skill_path` computed per discovered SKILL.md relative to the
///   plugin source root. Two plugins in the same source pointing at
///   the same on-disk skill therefore produce the same origin.
async fn load_skills_for_group(
    sym: &Symposium,
    parsed: &ParsedPlugin,
    group: &SkillGroup,
    workspace_crates: &[symposium_sdk::workspace::WorkspaceCrate],
    ctx: &mut PredicateContext<'_>,
) -> Vec<(Skill, SkillOrigin)> {
    let plugin = &parsed.plugin;
    let plugin_path = parsed.path.as_path();

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
        tracing::debug!(plugin = %plugin_path.display(), "skill group predicates failed, skipping");
        tracing::debug!(
            report = %crate::report::ReportEvent::SkillGroupConsidered {
                plugin: plugin.name.clone(),
                group_crates: predicates_display,
                source: Some(source_display(&group.source)),
                matched: false,
                skills_found: None,
                reason: Some("group predicates not satisfied".into()),
            },
        );
        return Vec::new();
    }

    let skills = match &group.source {
        PluginSource::Crate => load_crate_skills(plugin, group, workspace_crates, ctx).await,
        PluginSource::Git(url) => load_git_skills(sym, group, url).await,
        PluginSource::Path(p) => {
            let plugin_dir = plugin_path.parent().unwrap_or(plugin_path);
            let dir = plugin_dir.join(p);
            load_path_skills(&dir, group, parsed)
        }
        PluginSource::None => {
            // No source — nothing to discover. Kept distinct from the
            // path branch so we don't synthesize a bogus skill dir.
            Vec::new()
        }
    };

    tracing::debug!(
        report = %crate::report::ReportEvent::SkillGroupConsidered {
            plugin: plugin.name.clone(),
            group_crates: predicates_display,
            source: Some(source_display(&group.source)),
            matched: true,
            skills_found: Some(skills.len()),
            reason: None,
        },
    );

    skills
}

/// Resolve crate source predicates, fetch each matched crate, read its
/// `[package.metadata.symposium]`, and discover skills. Follows redirects
/// recursively with cycle detection.
///
/// The crates to fetch are the *witness* of the plugin- and group-level
/// predicate sets: the concrete crates that participate in satisfying the gate
/// (see [`predicate::union_matched_crates`]).
async fn load_crate_skills(
    plugin: &crate::plugins::Plugin,
    group: &SkillGroup,
    workspace_crates: &[symposium_sdk::workspace::WorkspaceCrate],
    ctx: &mut PredicateContext<'_>,
) -> Vec<(Skill, SkillOrigin)> {
    let matched = predicate::union_matched_crates(&[&plugin.predicates, &group.predicates], ctx);
    let mut skills = Vec::new();
    for (name, _version) in &matched {
        let mut visited = std::collections::HashSet::new();
        fetch_and_resolve_skills(
            name,
            None,
            workspace_crates,
            group,
            &mut visited,
            &mut skills,
            0,
        )
        .await;
    }
    skills
}

const MAX_REDIRECT_DEPTH: usize = 10;

/// Recursively fetch a crate and resolve its skills via metadata.
async fn fetch_and_resolve_skills(
    crate_name: &str,
    version_spec: Option<&str>,
    workspace_crates: &[symposium_sdk::workspace::WorkspaceCrate],
    group: &SkillGroup,
    visited: &mut std::collections::HashSet<String>,
    skills: &mut Vec<(Skill, SkillOrigin)>,
    depth: usize,
) {
    if depth >= MAX_REDIRECT_DEPTH {
        tracing::warn!(
            crate_name = %crate_name,
            "redirect chain exceeded depth limit ({MAX_REDIRECT_DEPTH}); stopping"
        );
        return;
    }

    let normalized = crate::crate_sources::normalize_crate_name(crate_name);
    let visit_key = format!("{normalized}@{}", version_spec.unwrap_or("*"));
    if !visited.insert(visit_key) {
        tracing::warn!(
            crate_name = %crate_name,
            "cycle detected in crate skill metadata redirects; skipping"
        );
        return;
    }

    let mut fetcher = crate::crate_sources::RustCrateFetch::new(crate_name, workspace_crates);
    if let Some(vs) = version_spec {
        fetcher = fetcher.version(vs);
    }

    let result = match fetcher.fetch().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(
                crate_name = %crate_name,
                error = %e,
                "failed to fetch crate source for skills"
            );
            return;
        }
    };

    let version = match semver::Version::parse(&result.version) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                crate_name = %crate_name,
                version = %result.version,
                error = %e,
                "skipping crate-source skills: unparseable version"
            );
            return;
        }
    };

    let origin = SkillOrigin::Crate {
        name: result.name.clone(),
        version,
    };

    let cargo_toml_path = result.path.join("Cargo.toml");
    let metadata = match crate::crate_metadata::parse_crate_metadata(&cargo_toml_path) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(
                crate_name = %crate_name,
                error = %e,
                "failed to parse crate metadata; falling back to default skills/ path"
            );
            None
        }
    };

    match metadata {
        None => {
            let dir = result.path.join(crate::plugins::CRATE_DEFAULT_SKILLS_PATH);
            let discovered = discover_skills(&dir, group);
            tracing::debug!(
                report = %crate::report::ReportEvent::SkillSourceSearched {
                    plugin: format!("crate:{crate_name}"),
                    source: format!("crate_path:{}", crate::plugins::CRATE_DEFAULT_SKILLS_PATH),
                    path: dir.display().to_string(),
                    skills_found: discovered.iter().filter(|r| r.is_ok()).count(),
                },
            );
            for skill_result in discovered {
                match skill_result {
                    Ok(skill) => skills.push((skill, origin.clone())),
                    Err(e) => tracing::warn!(
                        crate_name = %crate_name,
                        error = %e,
                        "failed to load skill from crate source"
                    ),
                }
            }
        }
        Some(meta) => {
            for source in &meta.skills {
                match source {
                    crate::crate_metadata::SkillSource::Path(p) => {
                        let dir = result.path.join(p);
                        let discovered = discover_skills(&dir, group);
                        tracing::debug!(
                            report = %crate::report::ReportEvent::SkillSourceSearched {
                                plugin: format!("crate:{crate_name}"),
                                source: format!("crate_path:{p}"),
                                path: dir.display().to_string(),
                                skills_found: discovered.iter().filter(|r| r.is_ok()).count(),
                            },
                        );
                        for skill_result in discovered {
                            match skill_result {
                                Ok(skill) => skills.push((skill, origin.clone())),
                                Err(e) => tracing::warn!(
                                    crate_name = %crate_name,
                                    path = %p,
                                    error = %e,
                                    "failed to load skill from crate source"
                                ),
                            }
                        }
                    }
                    crate::crate_metadata::SkillSource::Crate {
                        name: target_name,
                        version: target_version,
                    } => {
                        Box::pin(fetch_and_resolve_skills(
                            target_name,
                            target_version.as_deref(),
                            workspace_crates,
                            group,
                            visited,
                            skills,
                            depth + 1,
                        ))
                        .await;
                    }
                }
            }
        }
    }
}

/// Fetch a `source.git` group's tree and build a per-skill `Git` origin
/// keyed on `(owner/repo, commit_sha, skill_path-within-repo)` so two
/// plugins that loaded the same SKILL.md at the same commit collapse —
/// even if their `source.git` URLs differed.
async fn load_git_skills(
    sym: &Symposium,
    group: &SkillGroup,
    url: &str,
) -> Vec<(Skill, SkillOrigin)> {
    let Some((cache_dir, source, commit_sha)) = fetch_git_skill_source(sym, url).await else {
        return Vec::new();
    };
    let discovered = discover_skills(&cache_dir, group);
    tracing::debug!(
        report = %crate::report::ReportEvent::SkillSourceSearched {
            plugin: source.repo_id(),
            source: format!("git:{url}"),
            path: cache_dir.display().to_string(),
            skills_found: discovered.iter().filter(|r| r.is_ok()).count(),
        },
    );
    let mut skills = Vec::new();
    for result in discovered {
        match result {
            Ok(skill) => {
                let skill_path = skill_path_within_repo(&cache_dir, &skill.path, source.subpath());
                let origin = SkillOrigin::Git {
                    source: source.clone(),
                    commit_sha: commit_sha.clone(),
                    skill_path,
                };
                skills.push((skill, origin));
            }
            Err(e) => {
                tracing::warn!(git = %url, error = %e, "failed to load git-sourced skill");
            }
        }
    }
    skills
}

/// Discover skills under a local `source.path` directory and stamp each
/// with a `Source` origin keyed on `(source_name, skill-path-relative-to-source-root)`.
/// Two plugins in the same source pointing at the same on-disk skill
/// therefore produce the same origin.
fn load_path_skills(
    dir: &Path,
    group: &SkillGroup,
    parsed: &ParsedPlugin,
) -> Vec<(Skill, SkillOrigin)> {
    let discovered = discover_skills(dir, group);
    tracing::debug!(
        report = %crate::report::ReportEvent::SkillSourceSearched {
            plugin: parsed.plugin.name.clone(),
            source: format!("path:{}", dir.strip_prefix(&parsed.source_dir).unwrap_or(dir).display()),
            path: dir.display().to_string(),
            skills_found: discovered.iter().filter(|r| r.is_ok()).count(),
        },
    );
    let mut skills = Vec::new();
    for result in discovered {
        match result {
            Ok(skill) => {
                let origin = SkillOrigin::Source {
                    source_name: parsed.source_name.clone(),
                    skill_path: skill_path_relative_to(&parsed.source_dir, &skill.path),
                };
                skills.push((skill, origin));
            }
            Err(e) => {
                tracing::warn!(plugin = %parsed.path.display(), error = %e, "failed to load skill")
            }
        }
    }
    skills
}

/// SKILL.md's parent directory relative to a given root, normalized to
/// forward slashes. Canonicalizes both ends first so that two routes
/// to the same on-disk skill (e.g. `plugin-a/../shared/the-skill` vs.
/// the standalone walk's direct `shared/the-skill`) collapse to the
/// same string.
///
/// Falls back to the unnormalized path on canonicalization failure
/// (better than panicking) — that just degrades dedup to per-route
/// without losing correctness.
fn skill_path_relative_to(root: &Path, skill_md: &Path) -> String {
    let skill_dir = skill_md.parent().unwrap_or(skill_md);
    let canonical_skill =
        std::fs::canonicalize(skill_dir).unwrap_or_else(|_| skill_dir.to_path_buf());
    let canonical_root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    canonical_skill
        .strip_prefix(&canonical_root)
        .unwrap_or(&canonical_skill)
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

/// Compute a SKILL.md's path relative to the *repository root*, using
/// the cache directory (which holds the subtree the user pointed at via
/// `tree/<ref>/<subpath>`) and the source's intra-repo subpath.
///
/// Returned with forward-slash separators so the value is stable across
/// platforms — it's part of the `SkillOrigin::Git` identity.
fn skill_path_within_repo(cache_dir: &Path, skill_md: &Path, source_subpath: &str) -> String {
    let skill_dir = skill_md.parent().unwrap_or(skill_md);
    let rel = skill_dir
        .strip_prefix(cache_dir)
        .unwrap_or(skill_dir)
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/");
    if source_subpath.is_empty() {
        rel
    } else if rel.is_empty() {
        source_subpath.to_string()
    } else {
        format!("{source_subpath}/{rel}")
    }
}

/// Fetch a `source.git` group's tarball and look up the resolved commit
/// SHA from the cache meta. Returns `(cache_dir, parsed_source, commit_sha)`
/// or `None` (with a warning) on failure.
async fn fetch_git_skill_source(
    sym: &Symposium,
    git_url: &str,
) -> Option<(PathBuf, symposium_install::git::GitSource, String)> {
    let cache_mgr = symposium_install::git::GitCacheManager::new(&sym.install_context(), "plugins");
    let (cache_dir, source) = match cache_mgr
        .fetch_url_parsed(git_url, symposium_install::UpdateLevel::None)
        .await
    {
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
pub(crate) fn discover_skills(skills_dir: &Path, group: &SkillGroup) -> Vec<Result<Skill>> {
    if !skills_dir.is_dir() {
        return Vec::new();
    }

    let mut skill_files = Vec::new();
    find_skill_files_recursive(skills_dir, &mut skill_files);
    prune_nested_skills(&mut skill_files);

    skill_files
        .into_iter()
        .map(|skill_md| load_skill(&skill_md, group))
        .collect()
}

/// Recursively walk a directory collecting paths to `SKILL.md` files.
pub(crate) fn find_skill_files_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            find_skill_files_recursive(&path, out);
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

/// Load a standalone skill from a SKILL.md file (no plugin group context).
///
/// Standalone skills must be self-contained: all metadata (crates)
/// comes from the SKILL.md frontmatter.
/// Returns an error if `crates` is missing (standalone skills have
/// no group to inherit from).
pub fn load_standalone_skill(skill_md_path: &Path) -> Result<Skill> {
    let skill = load_skill(skill_md_path, &SkillGroup::default())?;
    if !skill.predicates.mentions_crate() {
        bail!(
            "standalone skill `{}` is missing `crates` in frontmatter \
             (standalone skills have no plugin group to inherit from)",
            skill.name()
        );
    }
    Ok(skill)
}

/// Load a single skill from a SKILL.md file.
///
/// A skill should have `crates` at either the skill level or
/// the group level (or both). If neither provides it, a warning is logged
/// but loading succeeds (the skill simply won't match any crate query).
fn load_skill(skill_md_path: &Path, group: &SkillGroup) -> Result<Skill> {
    let content = std::fs::read_to_string(skill_md_path)
        .with_context(|| format!("failed to read {}", skill_md_path.display()))?;

    let fm = parse_frontmatter(&content)
        .with_context(|| format!("failed to parse frontmatter in {}", skill_md_path.display()))?;

    let mut frontmatter = fm.fields;

    // Strip surrounding quotes from name if present (YAML scalars may be quoted)
    if let Some(name) = frontmatter.get_mut("name")
        && let Some(unquoted) = name.strip_prefix('"').and_then(|s| s.strip_suffix('"'))
    {
        *name = unquoted.to_string();
    }

    let name = frontmatter
        .get("name")
        .context("SKILL.md frontmatter missing required `name` field")?;

    // Validate description per agentskills.io spec
    // (https://agentskills.io/specification.md): required, non-empty, max 1024 chars.
    let desc = frontmatter
        .get("description")
        .context("SKILL.md frontmatter missing required `description` field")?;
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

    // Merge the skill-level `crates` (crate atoms, OR-combined) with the
    // frontmatter `predicates` (function-call syntax) into one set, ANDed with
    // the plugin- and group-level sets at match time.
    let crates = match fm.crates.as_deref() {
        Some(s) => Some(crate::predicate::CrateList::parse(s)?),
        None => None,
    };
    let extra = match fm.predicates.as_deref() {
        Some(s) => PredicateSet::parse(s)?,
        None => PredicateSet::default(),
    };
    let predicates = PredicateSet::merged(crates, extra);

    // Warn if no crate is referenced at either level — the skill won't match
    // any crate query, but we don't fail so a misconfigured plugin can't bring
    // down the tool.
    if !predicates.mentions_crate() && !group.predicates.mentions_crate() {
        tracing::warn!(
            skill = %name,
            "skill references no crate in SKILL.md frontmatter or its plugin [[skills]] group"
        );
    }

    let skill = Skill {
        frontmatter,
        predicates,
        body: fm.body,
        path: skill_md_path.to_path_buf(),
    };
    tracing::debug!(name = %skill.name(), path = %skill_md_path.display(), "skill loaded");
    Ok(skill)
}

/// Evaluate the skill-level predicate set and collect the skill if it holds.
///
/// Plugin- and group-level predicates have already been evaluated by callers as
/// a pre-filter, so only the skill-level set is checked here.
fn collect_skill_applicable_to(
    skill: Skill,
    origin: SkillOrigin,
    plugin_name: &str,
    ctx: &mut PredicateContext,
    results: &mut Vec<SkillWithGroupContext>,
) {
    if !skill.predicates.evaluate(ctx) {
        tracing::debug!(
            report = %crate::report::ReportEvent::SkillConsidered {
                skill: skill.name().to_string(),
                plugin: plugin_name.to_string(),
                matched: false,
                reason: Some("skill-level predicates not satisfied".into()),
            },
        );
        return;
    }

    tracing::debug!(
        report = %crate::report::ReportEvent::SkillConsidered {
            skill: skill.name().to_string(),
            plugin: plugin_name.to_string(),
            matched: true,
            reason: None,
        },
    );
    results.push(SkillWithGroupContext { skill, origin });
}

/// Raw frontmatter fields extracted from a SKILL.md file.
/// `crates` is comma-separated on a single line.
#[derive(Debug)]
struct RawFrontmatter {
    fields: BTreeMap<String, String>,
    /// Raw `crates` value (comma-separated predicate string).
    crates: Option<String>,
    /// Raw `predicates` value (comma-separated predicate expressions).
    predicates: Option<String>,
    body: String,
}

/// Parse SKILL.md content: extract `---`-fenced frontmatter and body.
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
    let body_start = end + 4; // "\n---".len()
    let body = after_first_fence
        .get(body_start..)
        .unwrap_or("")
        .strip_prefix('\n')
        .unwrap_or(after_first_fence.get(body_start..).unwrap_or(""));

    let yaml: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(frontmatter_text).context("frontmatter is not valid YAML")?;
    let mapping = yaml
        .as_mapping()
        .context("SKILL.md frontmatter must be a YAML mapping")?;

    let mut fields = BTreeMap::new();
    let mut crates = None;
    let mut predicates = None;

    for (key, value) in mapping {
        let Some(key) = key.as_str() else {
            bail!("SKILL.md frontmatter keys must be strings");
        };

        let Some(value) = value.as_str() else {
            bail!("SKILL.md frontmatter field `{key}` must be a string");
        };

        match key {
            "crates" => crates = Some(value.to_string()),
            "predicates" => predicates = Some(value.to_string()),
            _ => {
                fields.insert(key.to_string(), value.to_string());
            }
        }
    }

    Ok(RawFrontmatter {
        fields,
        crates,
        predicates,
        body: body.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use std::fs;

    use crate::predicate::Predicate;

    /// Build a predicate set from crate atoms (the `crates` field form).
    fn pred_set(s: &str) -> PredicateSet {
        PredicateSet::from_crates(s).unwrap()
    }

    fn ctx(crates: &[(String, semver::Version)]) -> PredicateContext<'_> {
        PredicateContext::new(crates)
    }

    // --- SkillOrigin identity ---

    #[test]
    fn skill_origin_git_dedups_when_repo_sha_and_path_match() {
        // Two `Git` origins that point at the same repo, the same
        // commit SHA, and the same path within that commit are the
        // *same* skill regardless of which URL form (or which plugin)
        // brought us there.
        let a = SkillOrigin::Git {
            source: symposium_install::git::GitSource::GitHub {
                owner: "foo".into(),
                repo: "bar".into(),
                git_ref: String::new(),
                subpath: String::new(),
            },
            commit_sha: "deadbeef".into(),
            skill_path: "skills/code-review".into(),
        };
        let b = SkillOrigin::Git {
            source: symposium_install::git::GitSource::GitHub {
                owner: "foo".into(),
                repo: "bar".into(),
                git_ref: String::new(),
                subpath: String::new(),
            },
            commit_sha: "deadbeef".into(),
            skill_path: "skills/code-review".into(),
        };
        assert_eq!(a, b);
        assert_eq!(a.short_hash(), b.short_hash());
    }

    #[test]
    fn skill_origin_git_distinct_paths_stay_distinct() {
        let a = SkillOrigin::Git {
            source: symposium_install::git::GitSource::GitHub {
                owner: "foo".into(),
                repo: "bar".into(),
                git_ref: String::new(),
                subpath: String::new(),
            },
            commit_sha: "deadbeef".into(),
            skill_path: "skills/a".into(),
        };
        let b = SkillOrigin::Git {
            source: symposium_install::git::GitSource::GitHub {
                owner: "foo".into(),
                repo: "bar".into(),
                git_ref: String::new(),
                subpath: String::new(),
            },
            commit_sha: "deadbeef".into(),
            skill_path: "skills/b".into(),
        };
        assert_ne!(a, b);
    }

    #[test]
    fn skill_origin_kinds_are_distinct() {
        // Distinct variants must not collide even with similar field values.
        let c = SkillOrigin::Crate {
            name: "foo".into(),
            version: semver::Version::new(1, 0, 0),
        };
        let s = SkillOrigin::Source {
            source_name: "foo".into(),
            skill_path: "1.0.0".into(),
        };
        let g = SkillOrigin::Git {
            source: symposium_install::git::GitSource::GitHub {
                owner: "foo".into(),
                repo: "bar".into(),
                git_ref: String::new(),
                subpath: String::new(),
            },
            commit_sha: "deadbeef".into(),
            skill_path: "1.0.0".into(),
        };
        assert_ne!(c, s);
        assert_ne!(c, g);
        assert_ne!(s, g);
        assert_ne!(c.short_hash(), s.short_hash());
        assert_ne!(c.short_hash(), g.short_hash());
        assert_ne!(s.short_hash(), g.short_hash());
    }

    // --- Frontmatter parsing ---

    #[test]
    fn parse_frontmatter_basic() {
        let content = indoc! {"
            ---
            name: my-skill
            description: A test skill
            crates: serde
            ---

            # Body content

            Some instructions here.
        "};
        let fm = parse_frontmatter(content).unwrap();
        assert_eq!(fm.fields.get("name").unwrap(), "my-skill");
        assert_eq!(fm.fields.get("description").unwrap(), "A test skill");
        assert_eq!(fm.crates.as_deref(), Some("serde"));
        assert!(fm.body.contains("# Body content"));
        assert!(fm.body.contains("Some instructions here."));
    }

    #[test]
    fn parse_frontmatter_comma_separated_crates() {
        let content = indoc! {"
            ---
            name: multi
            crates: serde, serde_json>=1.0, toml
            ---

            Body.
        "};
        let fm = parse_frontmatter(content).unwrap();
        assert_eq!(fm.crates.as_deref(), Some("serde, serde_json>=1.0, toml"));
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
                crates: serde
                ---

                Use serde like this.
            "},
        )
        .unwrap();

        let defaults = SkillGroup::default();
        let skill = load_skill(&skill_md, &defaults).unwrap();

        assert_eq!(skill.frontmatter.get("name").unwrap(), "test-skill");
        assert!(skill.predicates.references_crate("serde"));
        assert!(skill.body.contains("Use serde like this."));
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
                crates: serde, tokio>=1.0
                ---

                Body.
            "},
        )
        .unwrap();

        let defaults = SkillGroup::default();
        let skill = load_skill(&skill_md, &defaults).unwrap();
        assert!(skill.predicates.references_crate("serde"));
        assert!(skill.predicates.references_crate("tokio"));
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

        let defaults = SkillGroup {
            predicates: pred_set("tokio"),
            ..Default::default()
        };
        let skill = load_skill(&skill_md, &defaults).unwrap();

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
                crates: serde
                ---

                Body.
            "},
        )
        .unwrap();

        let defaults = SkillGroup {
            predicates: pred_set("tokio"),
            ..Default::default()
        };
        let skill = load_skill(&skill_md, &defaults).unwrap();

        // Skill-level crates specializes (ANDs with) plugin defaults
        assert!(skill.predicates.references_crate("serde"));
        assert!(!skill.predicates.references_crate("tokio"));
    }

    #[test]
    fn load_skill_missing_crates_warns_but_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_md = tmp.path().join("SKILL.md");
        fs::write(
            &skill_md,
            indoc! {"
                ---
                name: no-crates
                description: Missing crates
                ---

                Body.
            "},
        )
        .unwrap();

        let defaults = SkillGroup::default();
        // No longer an error — just a warning. The skill loads but won't match anything.
        let skill = load_skill(&skill_md, &defaults).unwrap();
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
                description: Plugin provides crates
                ---

                Body.
            "},
        )
        .unwrap();

        // Plugin defaults provide crates, so the skill doesn't need its own
        let defaults = SkillGroup {
            predicates: pred_set("serde"),
            ..Default::default()
        };
        let skill = load_skill(&skill_md, &defaults).unwrap();
        assert!(skill.predicates.is_empty()); // skill-level is empty
        assert_eq!(skill.frontmatter.get("name").unwrap(), "no-own-crates");
    }

    // --- Standalone skills ---

    #[test]
    fn load_standalone_skill_self_contained() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("my-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            indoc! {"
                ---
                name: my-standalone
                description: A standalone skill
                crates: serde
                ---

                Standalone body.
            "},
        )
        .unwrap();

        let skill = load_standalone_skill(&skill_dir.join("SKILL.md")).unwrap();
        assert_eq!(skill.name(), "my-standalone");
        assert!(skill.predicates.references_crate("serde"));
        assert!(skill.body.contains("Standalone body."));
    }

    #[test]
    fn validate_standalone_skill_bad_crates() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("bad-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            indoc! {"
                ---
                name: bad
                description: Bad crates skill
                crates: \">=not_valid!!\"
                ---

                Body.
            "},
        )
        .unwrap();

        let err = load_standalone_skill(&skill_dir.join("SKILL.md")).unwrap_err();
        assert!(
            err.to_string().contains("crate predicate"),
            "expected parse error, got: {err}"
        );
    }

    // --- Multi-level crate filtering tests ---

    #[tokio::test]
    async fn test_plugin_level_filtering_blocks_skills() {
        use crate::plugins::{ParsedPlugin, Plugin, PluginRegistry, PluginSource, SkillGroup};
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let sym = crate::config::Symposium::from_dir(tmp.path());

        // Create a plugin that only applies to "other-crate"
        let plugin = Plugin {
            name: "other-crate-plugin".to_string(),
            predicates: pred_set("other-crate"),
            hooks: vec![],
            skills: vec![SkillGroup {
                predicates: pred_set("serde"), // Group targets serde
                source: PluginSource::default(),
            }],
            mcp_servers: vec![],
            installations: Vec::new(),
            subcommands: BTreeMap::new(),
            custom_predicates: vec![],
        };

        let registry = PluginRegistry {
            plugins: vec![ParsedPlugin {
                path: tmp.path().join("plugin.toml"),
                plugin,
                source_name: "test".to_string(),
                source_dir: tmp.path().to_path_buf(),
            }],
            standalone_skills: vec![],
            warnings: vec![],
            custom_predicates: crate::plugins::CustomPredicateRegistry::default(),
        };

        // Query for serde - should find no skills because plugin doesn't apply
        let workspace_crates = vec![symposium_sdk::workspace::WorkspaceCrate {
            name: "serde".to_string(),
            version: semver::Version::new(1, 0, 0),
            path: None,
        }];
        let skills = skills_applicable_to(
            &sym,
            &registry,
            &workspace_crates,
            std::collections::HashMap::new(),
        )
        .await;

        assert!(
            skills.is_empty(),
            "Plugin should be filtered out at plugin level"
        );
    }

    #[tokio::test]
    async fn test_group_level_filtering_blocks_skills() {
        use crate::plugins::{ParsedPlugin, Plugin, PluginRegistry, PluginSource, SkillGroup};
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let sym = crate::config::Symposium::from_dir(tmp.path());

        // Create a plugin with wildcard that has a group targeting different crate
        let plugin = Plugin {
            name: "wildcard-plugin".to_string(),
            predicates: pred_set("*"), // Plugin applies to all
            hooks: vec![],
            skills: vec![SkillGroup {
                predicates: pred_set("other-crate"), // But group targets other-crate
                source: PluginSource::default(),
            }],
            mcp_servers: vec![],
            installations: Vec::new(),
            subcommands: BTreeMap::new(),
            custom_predicates: vec![],
        };

        let registry = PluginRegistry {
            plugins: vec![ParsedPlugin {
                path: tmp.path().join("plugin.toml"),
                plugin,
                source_name: "test".to_string(),
                source_dir: tmp.path().to_path_buf(),
            }],
            standalone_skills: vec![],
            warnings: vec![],
            custom_predicates: crate::plugins::CustomPredicateRegistry::default(),
        };

        // Query for serde - should find no skills because group doesn't match
        let workspace_crates = vec![symposium_sdk::workspace::WorkspaceCrate {
            name: "serde".to_string(),
            version: semver::Version::new(1, 0, 0),
            path: None,
        }];
        let skills = skills_applicable_to(
            &sym,
            &registry,
            &workspace_crates,
            std::collections::HashMap::new(),
        )
        .await;

        assert!(
            skills.is_empty(),
            "Skills should be filtered out at group level"
        );
    }

    #[tokio::test]
    async fn test_all_levels_match_allows_skills() {
        use crate::plugins::{ParsedPlugin, Plugin, PluginRegistry, PluginSource, SkillGroup};
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
                crates: serde
                ---

                Use derive macros.
            "},
        )
        .unwrap();

        // Create a plugin where all levels match serde
        let plugin = Plugin {
            name: "serde-plugin".to_string(),
            predicates: pred_set("serde"), // Plugin targets serde
            hooks: vec![],
            skills: vec![SkillGroup {
                predicates: pred_set("serde"), // Group also targets serde
                source: PluginSource::Path(skill_dir.to_path_buf()),
            }],
            mcp_servers: vec![],
            installations: Vec::new(),
            subcommands: BTreeMap::new(),
            custom_predicates: vec![],
        };

        let registry = PluginRegistry {
            plugins: vec![ParsedPlugin {
                path: tmp.path().join("plugin.toml"),
                plugin,
                source_name: "test".to_string(),
                source_dir: tmp.path().to_path_buf(),
            }],
            standalone_skills: vec![],
            warnings: vec![],
            custom_predicates: crate::plugins::CustomPredicateRegistry::default(),
        };

        let workspace_crates = vec![symposium_sdk::workspace::WorkspaceCrate {
            name: "serde".to_string(),
            version: semver::Version::new(1, 0, 0),
            path: None,
        }];
        let skills = skills_applicable_to(
            &sym,
            &registry,
            &workspace_crates,
            std::collections::HashMap::new(),
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
        use crate::plugins::{ParsedPlugin, Plugin, PluginRegistry, PluginSource, SkillGroup};
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
                crates: serde
                ---

                Use derive macros.
            "},
        )
        .unwrap();

        // Plugin matches by crates, but its shell predicate fails.
        let plugin = Plugin {
            name: "p".into(),
            predicates: PredicateSet {
                predicates: vec![
                    Predicate::Crate("serde".into(), None),
                    Predicate::Shell("false".into()),
                ],
            },
            hooks: vec![],
            skills: vec![SkillGroup {
                predicates: pred_set("serde"),
                source: PluginSource::Path(skill_dir.to_path_buf()),
            }],
            mcp_servers: vec![],
            installations: Vec::new(),
            subcommands: Default::default(),
            custom_predicates: vec![],
        };

        let registry = PluginRegistry {
            plugins: vec![ParsedPlugin {
                path: tmp.path().join("plugin.toml"),
                plugin,
                source_name: "test".to_string(),
                source_dir: PathBuf::from(".".to_string()),
            }],
            standalone_skills: vec![],
            warnings: vec![],
            custom_predicates: crate::plugins::CustomPredicateRegistry::default(),
        };

        let workspace = vec![symposium_sdk::workspace::WorkspaceCrate {
            name: "serde".into(),
            version: semver::Version::new(1, 0, 0),
            path: None,
        }];
        let skills = skills_applicable_to(
            &sym,
            &registry,
            &workspace,
            std::collections::HashMap::new(),
        )
        .await;
        assert!(
            skills.is_empty(),
            "plugin predicate=false should filter out skills"
        );
    }

    #[tokio::test]
    async fn predicate_pass_allows_skill() {
        use crate::plugins::{ParsedPlugin, Plugin, PluginRegistry, PluginSource, SkillGroup};
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
                crates: serde
                ---

                Body.
            "},
        )
        .unwrap();

        let plugin = Plugin {
            name: "p".into(),
            predicates: PredicateSet {
                predicates: vec![
                    Predicate::Crate("serde".into(), None),
                    Predicate::Shell("true".into()),
                ],
            },
            hooks: vec![],
            skills: vec![SkillGroup {
                predicates: PredicateSet {
                    predicates: vec![
                        Predicate::Crate("serde".into(), None),
                        Predicate::Shell("true".into()),
                    ],
                },
                source: PluginSource::Path(skill_dir.to_path_buf()),
            }],
            mcp_servers: vec![],
            installations: Vec::new(),
            subcommands: Default::default(),
            custom_predicates: vec![],
        };

        let registry = PluginRegistry {
            plugins: vec![ParsedPlugin {
                path: tmp.path().join("plugin.toml"),
                plugin,
                source_name: "test".to_string(),
                source_dir: PathBuf::from(".".to_string()),
            }],
            standalone_skills: vec![],
            warnings: vec![],
            custom_predicates: crate::plugins::CustomPredicateRegistry::default(),
        };

        let workspace = vec![symposium_sdk::workspace::WorkspaceCrate {
            name: "serde".into(),
            version: semver::Version::new(1, 0, 0),
            path: None,
        }];
        let skills = skills_applicable_to(
            &sym,
            &registry,
            &workspace,
            std::collections::HashMap::new(),
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
                crates: serde
                predicates: shell(command -v rg), path_exists(Cargo.toml)
                ---

                Body.
            "},
        )
        .unwrap();

        let skill = load_skill(&skill_md, &SkillGroup::default()).unwrap();
        // `crates: serde` lowers to a leading `crate(serde)`, then the two
        // function-call predicates.
        assert_eq!(
            skill.predicates.predicates,
            vec![
                Predicate::Crate("serde".into(), None),
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
                crates: serde
                ---

                Body.
            "},
        )
        .unwrap();

        let err = load_standalone_skill(&skill_dir.join("SKILL.md")).unwrap_err();
        assert!(
            err.to_string().contains("missing required `name` field"),
            "expected missing name error, got: {err}"
        );
    }

    #[test]
    fn standalone_skill_requires_crates() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("no-crates");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            indoc! {"
                ---
                name: no-crates
                description: Missing crates
                ---

                Body.
            "},
        )
        .unwrap();

        let err = load_standalone_skill(&skill_dir.join("SKILL.md")).unwrap_err();
        assert!(
            err.to_string().contains("missing `crates`"),
            "expected crates error, got: {err}"
        );
    }

    #[tokio::test]
    async fn list_includes_standalone_skills() {
        use crate::plugins::PluginRegistry;

        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("my-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            indoc! {"
                ---
                name: standalone-serde
                description: Standalone serde skill
                crates: serde
                ---

                Body.
            "},
        )
        .unwrap();

        let skill = load_standalone_skill(&skill_dir.join("SKILL.md")).unwrap();
        let registry = PluginRegistry {
            plugins: Vec::new(),
            standalone_skills: vec![crate::plugins::StandaloneSkill {
                skill,
                origin: SkillOrigin::Source {
                    source_name: "test".to_string(),
                    skill_path: "my-skill".to_string(),
                },
            }],
            warnings: vec![],
            custom_predicates: crate::plugins::CustomPredicateRegistry::default(),
        };

        let sym = crate::config::Symposium::from_dir(tmp.path());
        let workspace = vec![symposium_sdk::workspace::WorkspaceCrate {
            name: "serde".to_string(),
            version: semver::Version::new(1, 0, 0),
            path: None,
        }];
        let results = skills_applicable_to(
            &sym,
            &registry,
            &workspace,
            std::collections::HashMap::new(),
        )
        .await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].skill.name(), "standalone-serde");
        assert!(results[0].skill.predicates.references_crate("serde"));
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
                crates: serde
                ---

                Discovered body.
            "},
        )
        .unwrap();

        let defaults = SkillGroup::default();
        let skills = discover_skills(&plugin_dir.join("skills"), &defaults);

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
                crates: tokio
                ---

                Nested body.
            "},
        )
        .unwrap();

        let defaults = SkillGroup::default();
        let skills = discover_skills(root, &defaults);

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
                crates: serde
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
                crates: serde
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
                crates: tokio
                ---

                Sibling.
            "},
        )
        .unwrap();

        let defaults = SkillGroup::default();
        let skills = discover_skills(root, &defaults);

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
        let defaults = SkillGroup::default();
        let skills = discover_skills(tmp.path(), &defaults);
        assert!(skills.is_empty());
    }

    // --- AND composition across levels (plugin ∧ group ∧ skill) ---

    fn v(s: &str) -> semver::Version {
        semver::Version::parse(s).unwrap()
    }

    /// Each level's `crates` lowers to one predicate set; the skill applies when
    /// every level's set holds (AND across levels).
    fn applies(levels: &[&str], ws: &[(String, semver::Version)]) -> bool {
        levels
            .iter()
            .all(|spec| pred_set(spec).evaluate(&mut ctx(ws)))
    }

    #[test]
    fn and_across_levels_all_satisfied() {
        let ws = vec![("serde".into(), v("1.0.0")), ("tokio".into(), v("1.0.0"))];
        assert!(applies(&["serde", "tokio"], &ws));
    }

    #[test]
    fn and_across_levels_one_missing() {
        let ws = vec![("serde".into(), v("1.0.0"))];
        assert!(!applies(&["serde", "tokio"], &ws));
    }

    #[test]
    fn and_across_levels_empty_is_vacuously_true() {
        let ws = vec![("serde".into(), v("1.0.0"))];
        assert!(applies(&[], &ws));
    }

    #[test]
    fn wildcard_level_matches_any() {
        let ws = vec![("serde".into(), v("1.0.0"))];
        assert!(applies(&["*", "serde"], &ws));
    }
}
