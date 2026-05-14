//! Skill model, frontmatter parsing, discovery, and crate advice output.
//!
//! Skills follow the [agentskills.io](https://agentskills.io/specification.md) format
//! and live inside plugin directories under `skills/*/SKILL.md`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::config::Symposium;
use crate::plugins::{ParsedPlugin, PluginRegistry, PluginSource, SkillGroup};
use crate::predicate::{self, Predicate, PredicateSet};

/// A parsed skill from a SKILL.md file.
#[derive(Debug, Clone)]
pub struct Skill {
    /// Frontmatter fields as key-value pairs (name, description, license, etc.).
    pub frontmatter: BTreeMap<String, String>,
    /// Crate predicates this skill advises on (skill-level; ANDed with group-level).
    pub crates: Vec<Predicate>,
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
///   of a specific crate version (`source = "crate"` / `source.crate_path`).
///   Identity is `(name, version)` only: two plugins targeting the same
///   crate version produce the same logical skills regardless of which
///   plugin pointed at them.
/// - `Git { repo, commit_sha, skill_path }` — the skill came from a
///   `source.git` group. Identity is the triple `(repo, commit_sha,
///   skill_path)`: the repo is in `<owner>/<repo>` form (no `tree/<ref>`
///   prefix), the commit SHA is the resolved hash that was actually
///   fetched, and `skill_path` is the SKILL.md's path within the repo
///   tree. Two plugins that pointed at the same repo via different URL
///   forms (root URL vs. a `tree/<ref>/<subpath>` URL) collapse to one
///   install if they end up loading the same SKILL.md from the same
///   commit; different SKILL.md files within one repo stay distinct.
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
        /// `<owner>/<repo>` — the repository identity, stripped of any
        /// branch / subpath URL components.
        repo: String,
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

/// A skill paired with all predicate sets from its lineage and the origin
/// it was discovered through.
///
/// Each predicate set must match (AND across sets). Within a set,
/// any predicate matching suffices (OR within a set). An empty
/// `predicate_sets` vec means "always matches".
pub struct SkillWithGroupContext {
    pub skill: Skill,
    /// Accumulated predicate sets: [plugin.crates, group.crates, skill.crates].
    /// All predicate sets must match for the skill to apply.
    pub predicate_sets: Vec<PredicateSet>,
    /// Where the skill was discovered. Drives install-path disambiguation
    /// and dedup at sync time.
    pub origin: SkillOrigin,
}

impl SkillWithGroupContext {
    /// Check whether this skill matches the given workspace dependencies.
    ///
    /// Every predicate set must match. An empty vec is vacuously true.
    pub fn matches_workspace(&self, deps: &[(String, semver::Version)]) -> bool {
        self.predicate_sets.iter().all(|ps| ps.matches(deps))
    }
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
    workspace_crates: &[crate::crate_sources::WorkspaceCrate],
) -> Vec<SkillWithGroupContext> {
    let mut results = Vec::new();

    let for_crates: Vec<(String, semver::Version)> = workspace_crates
        .iter()
        .map(|wc| (wc.name.clone(), wc.version.clone()))
        .collect();

    // Skills from plugin manifests. We iterate these separately
    // because we lazily load skill groups, so there
    // is extra logic.
    for parsed in &registry.plugins {
        let plugin = &parsed.plugin;
        // First check if plugin applies to these crates
        if !plugin.applies_to_crates(&for_crates) {
            continue;
        }

        for group in &plugin.skills {
            let (group_crates, skills) =
                load_skills_for_group(sym, parsed, group, workspace_crates, &for_crates).await;

            for (skill, origin) in skills {
                collect_skill_applicable_to(
                    &skill,
                    origin,
                    &plugin.crates,
                    &group_crates,
                    &for_crates,
                    &mut results,
                );
            }
        }
    }

    // Standalone skills already carry their own `SkillOrigin` (computed
    // from the plugin source name and the skill's path within that source).
    let empty = PredicateSet { predicates: vec![] };
    for entry in &registry.standalone_skills {
        collect_skill_applicable_to(
            &entry.skill,
            entry.origin.clone(),
            &empty,
            &empty,
            &for_crates,
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
/// - `Git` → `SkillOrigin::Git { repo, commit_sha, skill_path }`, one
///   per discovered SKILL.md (path varies per skill within the group).
/// - `Path` → `SkillOrigin::Source { source_name, skill_path }`, with
///   `skill_path` computed per discovered SKILL.md relative to the
///   plugin source root. Two plugins in the same source pointing at
///   the same on-disk skill therefore produce the same origin.
async fn load_skills_for_group(
    sym: &Symposium,
    parsed: &ParsedPlugin,
    group: &SkillGroup,
    workspace_crates: &[crate::crate_sources::WorkspaceCrate],
    for_crates: &[(String, semver::Version)],
) -> (PredicateSet, Vec<(Skill, SkillOrigin)>) {
    let plugin = &parsed.plugin;
    let plugin_path = parsed.path.as_path();
    let group_crates = group
        .crates
        .clone()
        .unwrap_or_else(|| PredicateSet { predicates: vec![] });

    // Pre-fetch filtering: skip groups whose crate predicates don't match any target.
    if !group_crates.predicates.is_empty() && !group_crates.matches(for_crates) {
        tracing::debug!(plugin = %plugin_path.display(), "skill group crates don't match, skipping");
        return (group_crates, Vec::new());
    }

    let skills = match &group.source {
        PluginSource::CratePath(source) => {
            load_crate_path_skills(
                source.as_str(),
                &plugin.crates,
                &group_crates,
                group,
                workspace_crates,
                for_crates,
            )
            .await
        }
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

    (group_crates, skills)
}

/// Resolve crate-path predicates, fetch each matched crate, and discover
/// skills inside the configured `crate_path`. One `SkillOrigin::Crate`
/// per matched crate.
async fn load_crate_path_skills(
    crate_path: &str,
    plugin_crates: &PredicateSet,
    group_crates: &PredicateSet,
    group: &SkillGroup,
    workspace_crates: &[crate::crate_sources::WorkspaceCrate],
    for_crates: &[(String, semver::Version)],
) -> Vec<(Skill, SkillOrigin)> {
    let matched = predicate::union_matched_crates(&[plugin_crates, group_crates], for_crates);
    let mut skills = Vec::new();
    for (name, _version) in &matched {
        match crate::crate_sources::RustCrateFetch::new(name, workspace_crates)
            .fetch()
            .await
        {
            Ok(result) => {
                // The fetch result holds the canonical name/version
                // (hyphen/underscore normalized for path deps, exact
                // version from cargo metadata for registry deps), so
                // two plugins resolving the same crate produce the
                // same `SkillOrigin::Crate` and dedupe at sync time.
                let version = match semver::Version::parse(&result.version) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!(
                            crate_name = %name,
                            version = %result.version,
                            error = %e,
                            "skipping crate-source skills: unparseable version"
                        );
                        continue;
                    }
                };
                let origin = SkillOrigin::Crate {
                    name: result.name.clone(),
                    version,
                };
                let dir = result.path.join(crate_path);
                for skill_result in discover_skills(&dir, group) {
                    match skill_result {
                        Ok(skill) => skills.push((skill, origin.clone())),
                        Err(e) => tracing::warn!(
                            crate_name = %name,
                            error = %e,
                            "failed to load skill from crate source"
                        ),
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    crate_name = %name,
                    error = %e,
                    "failed to fetch crate source for skills"
                );
            }
        }
    }
    skills
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
    let Some((gh, cache_dir, commit_sha)) = fetch_git_skill_source(sym, url).await else {
        return Vec::new();
    };
    let mut skills = Vec::new();
    for result in discover_skills(&cache_dir, group) {
        match result {
            Ok(skill) => {
                let skill_path = skill_path_within_repo(&cache_dir, &skill.path, &gh.path);
                let origin = SkillOrigin::Git {
                    repo: format!("{}/{}", gh.owner, gh.repo),
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
    let mut skills = Vec::new();
    for result in discover_skills(dir, group) {
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
/// SHA from the cache meta. Returns `None` (with a warning) on failure.
async fn fetch_git_skill_source(
    sym: &Symposium,
    git_url: &str,
) -> Option<(crate::installation::git::GitHubSource, PathBuf, String)> {
    let gh = match crate::installation::git::parse_github_url(git_url) {
        Ok(gh) => gh,
        Err(e) => {
            tracing::warn!(git = %git_url, error = %e, "failed to parse git URL");
            return None;
        }
    };
    let cache_mgr = crate::installation::git::GitCacheManager::new(sym, "plugins");
    let cache_dir = match cache_mgr
        .get_or_fetch(&gh, git_url, crate::plugins::UpdateLevel::None)
        .await
    {
        Ok(p) => p,
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
    Some((gh, cache_dir, meta.commit_sha))
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
    if skill.crates.is_empty() {
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

    // Parse skill-level crates predicates (comma-separated).
    // This is independent of group-level — both layers are ANDed at match time.
    let crates = if let Some(ref crates_str) = fm.crates {
        predicate::parse_comma_separated(crates_str)?
    } else {
        Vec::new()
    };

    // Warn if no crates at either level — the skill won't match anything,
    // but we don't fail so a misconfigured plugin can't bring down the tool.
    if crates.is_empty() && group.crates.is_none() {
        tracing::warn!(
            skill = %name,
            "skill has no `crates` in SKILL.md frontmatter or plugin [[skills]] group"
        );
    }

    let skill = Skill {
        frontmatter,
        crates,
        body: fm.body,
        path: skill_md_path.to_path_buf(),
    };
    tracing::debug!(name = %skill.name(), path = %skill_md_path.display(), "skill loaded");
    Ok(skill)
}

/// Filter a single skill by crate constraints, pushing it onto `results`
/// if it matches with its origin attached.
fn collect_skill_applicable_to(
    skill: &Skill,
    origin: SkillOrigin,
    plugin_crates: &PredicateSet,
    group_crates: &PredicateSet,
    for_crates: &[(String, semver::Version)],
    results: &mut Vec<SkillWithGroupContext>,
) {
    let mut predicate_sets = Vec::new();
    if !plugin_crates.predicates.is_empty() {
        predicate_sets.push(plugin_crates.clone());
    }
    if !group_crates.predicates.is_empty() {
        predicate_sets.push(group_crates.clone());
    }
    if !skill.crates.is_empty() {
        predicate_sets.push(PredicateSet {
            predicates: skill.crates.clone(),
        });
    }

    let entry = SkillWithGroupContext {
        skill: skill.clone(),
        predicate_sets,
        origin,
    };

    if !entry.matches_workspace(for_crates) {
        return;
    }
    results.push(entry);
}

/// Raw frontmatter fields extracted from a SKILL.md file.
/// `crates` is comma-separated on a single line.
#[derive(Debug)]
struct RawFrontmatter {
    fields: BTreeMap<String, String>,
    /// Raw `crates` value (comma-separated predicate string).
    crates: Option<String>,
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

    for (key, value) in mapping {
        let Some(key) = key.as_str() else {
            bail!("SKILL.md frontmatter keys must be strings");
        };

        let Some(value) = value.as_str() else {
            bail!("SKILL.md frontmatter field `{key}` must be a string");
        };

        if key == "crates" {
            crates = Some(value.to_string());
        } else {
            fields.insert(key.to_string(), value.to_string());
        }
    }

    Ok(RawFrontmatter {
        fields,
        crates,
        body: body.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use std::fs;

    /// Parse a predicate string for use in test fixtures.
    fn pred(s: &str) -> Predicate {
        crate::predicate::parse(s).unwrap()
    }

    fn pred_set(s: &str) -> PredicateSet {
        PredicateSet::parse(s).unwrap()
    }

    // --- SkillOrigin identity ---

    #[test]
    fn skill_origin_git_dedups_when_repo_sha_and_path_match() {
        // Two `Git` origins that point at the same repo, the same
        // commit SHA, and the same path within that commit are the
        // *same* skill regardless of which URL form (or which plugin)
        // brought us there.
        let a = SkillOrigin::Git {
            repo: "foo/bar".into(),
            commit_sha: "deadbeef".into(),
            skill_path: "skills/code-review".into(),
        };
        let b = SkillOrigin::Git {
            repo: "foo/bar".into(),
            commit_sha: "deadbeef".into(),
            skill_path: "skills/code-review".into(),
        };
        assert_eq!(a, b);
        assert_eq!(a.short_hash(), b.short_hash());
    }

    #[test]
    fn skill_origin_git_distinct_paths_stay_distinct() {
        let a = SkillOrigin::Git {
            repo: "foo/bar".into(),
            commit_sha: "deadbeef".into(),
            skill_path: "skills/a".into(),
        };
        let b = SkillOrigin::Git {
            repo: "foo/bar".into(),
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
            repo: "foo/bar".into(),
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
        assert_eq!(skill.crates.len(), 1);
        assert!(skill.crates[0].references_crate("serde"));
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
        assert_eq!(skill.crates.len(), 2);
        assert!(skill.crates[0].references_crate("serde"));
        assert!(skill.crates[1].references_crate("tokio"));
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
            crates: Some(pred_set("tokio")),
            ..Default::default()
        };
        let skill = load_skill(&skill_md, &defaults).unwrap();

        // Skill has no crates in frontmatter, so it's empty at skill level.
        // The plugin default provides the crates scope.
        assert!(skill.crates.is_empty());
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
            crates: Some(pred_set("tokio")),
            ..Default::default()
        };
        let skill = load_skill(&skill_md, &defaults).unwrap();

        // Skill-level crates specializes (ANDs with) plugin defaults
        assert_eq!(skill.crates.len(), 1);
        assert!(skill.crates[0].references_crate("serde"));
        assert!(!skill.crates[0].references_crate("tokio"));
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
        assert!(skill.crates.is_empty());
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
            crates: Some(pred_set("serde")),
            ..Default::default()
        };
        let skill = load_skill(&skill_md, &defaults).unwrap();
        assert!(skill.crates.is_empty()); // skill-level is empty
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
        assert!(skill.crates[0].references_crate("serde"));
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
            err.to_string().contains("failed to parse predicate"),
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
            crates: pred_set("other-crate"),
            hooks: vec![],
            skills: vec![SkillGroup {
                crates: Some(pred_set("serde")), // Group targets serde
                source: PluginSource::default(),
            }],
            mcp_servers: vec![],
            installations: Vec::new(),
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
        };

        // Query for serde - should find no skills because plugin doesn't apply
        let workspace_crates = vec![crate::crate_sources::WorkspaceCrate {
            name: "serde".to_string(),
            version: semver::Version::new(1, 0, 0),
            path: None,
        }];
        let skills = skills_applicable_to(&sym, &registry, &workspace_crates).await;

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
            crates: pred_set("*"), // Plugin applies to all
            hooks: vec![],
            skills: vec![SkillGroup {
                crates: Some(pred_set("other-crate")), // But group targets other-crate
                source: PluginSource::default(),
            }],
            mcp_servers: vec![],
            installations: Vec::new(),
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
        };

        // Query for serde - should find no skills because group doesn't match
        let workspace_crates = vec![crate::crate_sources::WorkspaceCrate {
            name: "serde".to_string(),
            version: semver::Version::new(1, 0, 0),
            path: None,
        }];
        let skills = skills_applicable_to(&sym, &registry, &workspace_crates).await;

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
            crates: pred_set("serde"), // Plugin targets serde
            hooks: vec![],
            skills: vec![SkillGroup {
                crates: Some(pred_set("serde")), // Group also targets serde
                source: PluginSource::Path(skill_dir.to_path_buf()),
            }],
            mcp_servers: vec![],
            installations: Vec::new(),
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
        };

        let workspace_crates = vec![crate::crate_sources::WorkspaceCrate {
            name: "serde".to_string(),
            version: semver::Version::new(1, 0, 0),
            path: None,
        }];
        let skills = skills_applicable_to(&sym, &registry, &workspace_crates).await;

        assert_eq!(
            skills.len(),
            1,
            "Should find one skill when all levels match"
        );
        assert_eq!(skills[0].skill.name(), "serde-basics");
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
        };

        let sym = crate::config::Symposium::from_dir(tmp.path());
        let workspace = vec![crate::crate_sources::WorkspaceCrate {
            name: "serde".to_string(),
            version: semver::Version::new(1, 0, 0),
            path: None,
        }];
        let results = skills_applicable_to(&sym, &registry, &workspace).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].skill.name(), "standalone-serde");
        // No group context for standalone skills
        assert_eq!(results[0].predicate_sets.len(), 1); // just skill-level crates
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

    // --- AND composition tests for matches_workspace ---

    fn v(s: &str) -> semver::Version {
        semver::Version::parse(s).unwrap()
    }

    fn resolved(skill_crates: Vec<Predicate>, group: Vec<Predicate>) -> SkillWithGroupContext {
        let mut predicate_sets = Vec::new();
        if !group.is_empty() {
            predicate_sets.push(PredicateSet { predicates: group });
        }
        if !skill_crates.is_empty() {
            predicate_sets.push(PredicateSet {
                predicates: skill_crates,
            });
        }
        SkillWithGroupContext {
            skill: Skill {
                frontmatter: BTreeMap::new(),
                crates: vec![],
                body: String::new(),
                path: PathBuf::new(),
            },
            predicate_sets,
            origin: SkillOrigin::Source {
                source_name: "test".to_string(),
                skill_path: String::new(),
            },
        }
    }

    #[test]
    fn matches_workspace_both_sets_satisfied() {
        let entry = resolved(vec![pred("tokio")], vec![pred("serde")]);
        let ws = vec![("serde".into(), v("1.0.0")), ("tokio".into(), v("1.0.0"))];
        assert!(entry.matches_workspace(&ws));
    }

    #[test]
    fn matches_workspace_skill_set_missing() {
        let entry = resolved(vec![pred("tokio")], vec![pred("serde")]);
        let ws = vec![("serde".into(), v("1.0.0"))];
        assert!(!entry.matches_workspace(&ws));
    }

    #[test]
    fn matches_workspace_group_only() {
        let entry = resolved(vec![], vec![pred("serde")]);
        let ws = vec![("serde".into(), v("1.0.0"))];
        assert!(entry.matches_workspace(&ws));
    }

    #[test]
    fn matches_workspace_skill_only() {
        let entry = resolved(vec![pred("serde")], vec![]);
        let ws = vec![("serde".into(), v("1.0.0"))];
        assert!(entry.matches_workspace(&ws));
    }

    #[test]
    fn matches_workspace_empty_predicate_sets() {
        let entry = resolved(vec![], vec![]);
        let ws = vec![("serde".into(), v("1.0.0"))];
        assert!(entry.matches_workspace(&ws));
    }

    #[test]
    fn matches_workspace_wildcard() {
        let entry = resolved(vec![], vec![pred("*")]);
        let ws = vec![("serde".into(), v("1.0.0"))];
        assert!(entry.matches_workspace(&ws));
    }
}
