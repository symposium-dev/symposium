//! Skill model, frontmatter parsing, discovery, and crate advice output.
//!
//! Skills follow the [agentskills.io](https://agentskills.io/specification.md) format
//! and live inside plugin directories under `skills/*/SKILL.md`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use symposium_install::UpdateLevel;

use crate::config::Symposium;
use crate::plugins::{ParsedPlugin, PluginRegistry, PluginSource, SkillGroup};
use crate::pm::PackageManager as _;
use crate::predicate::{self, PredicateContext, PredicateSet};

fn source_display(source: &PluginSource) -> String {
    match source {
        PluginSource::Path(p) => format!("path:{}", p.display()),
        PluginSource::Git(url) => format!("git:{url}"),
    }
}

/// A parsed skill from a SKILL.md file.
#[derive(Debug, Clone)]
pub struct Skill {
    /// Frontmatter fields as key-value pairs (name, description, license, etc.).
    pub frontmatter: BTreeMap<String, String>,
    /// Skill-level activation predicates: the frontmatter `depends-on` (lowered to
    /// `any(depends-on(...))`) merged with `predicates`. ANDed with the plugin- and
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

// Install-path disambiguator identifying *where a skill's bytes live*.
//
// A skill's origin is just the on-disk path of its `SKILL.md`, hashed. Two
// references that resolve to the same file — the same crate reached through two
// chained plugins, or a `source.path` group and the standalone walk landing on
// the same bundle — produce the same hash and dedupe at sync time; skills at
// different paths stay distinct. Group discovery canonicalizes its scan dir
// before walking (the standalone walk hashes the raw path), so equivalent paths
// collapse to one string.
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

/// An applicable skill paired with the origin it was discovered through.
///
/// The plugin-, group-, and skill-level predicate sets are all evaluated during
/// collection; only skills whose every level holds end up here.
pub struct SkillWithGroupContext {
    pub skill: Skill,
    /// The hash of where the skill was discovered. Drives install-path disambiguation
    /// and dedup at sync time.
    pub origin_hash: String,
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
    update: UpdateLevel,
) -> Vec<SkillWithGroupContext> {
    let mut results = Vec::new();

    let for_crates = crate::pm::CargoPm.list_deps(workspace_crates);
    let mut ctx = PredicateContext::with_custom_predicates(&for_crates, custom_predicate_entries);

    // Skills from plugin manifests. We iterate these separately
    // because we lazily load skill groups, so there
    // is extra logic.
    for parsed in &registry.plugins {
        let plugin = &parsed.plugin;
        // Plugin-level predicates gate everything below. Evaluated before
        // group fetching to avoid wasted work. Goes through the ParsedPlugin
        // so the plugin's provenance is stamped for `workspace-member()`.
        if !parsed.applies(&mut ctx) {
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
            let skills = load_skills_for_group(sym, parsed, group, &mut ctx, update).await;
            for (skill, origin_hash) in skills {
                collect_skill_applicable_to(
                    skill,
                    origin_hash,
                    &plugin.name,
                    &mut ctx,
                    &mut results,
                );
            }
        }

        // `[[plugins]]` chained references: whenever this plugin is active and
        // an edge's own predicates hold, the referenced crate is loaded as a
        // first-class plugin and its skills contributed. Expansion recurses
        // into the loaded crate's own chained edges — a crate that names
        // another crate (the reschema'd `[package.metadata.symposium]`
        // redirect) is followed transitively — with per-plugin cycle detection.
        let mut visited = std::collections::HashSet::new();
        expand_chained_plugins(
            sym,
            parsed,
            workspace_crates,
            &mut ctx,
            update,
            &mut visited,
            0,
            &mut results,
        )
        .await;
    }

    // Standalone skills already carry their own origin hash (computed
    // from the SKILL.md's on-disk path, like every other skill).
    if !registry.standalone_skills.is_empty() {
        tracing::debug!(
            report = %crate::report::ReportEvent::PluginConsidered {
                plugin: "(standalone skills)".into(),
                matched: true,
                reason: None,
            },
        );
    }
    // Standalone skills have no defining plugin; they never count as
    // workspace members (clear any stamp left by the plugin loop).
    ctx.set_workspace_member(false);
    for entry in &registry.standalone_skills {
        collect_skill_applicable_to(
            entry.skill.clone(),
            entry.origin_hash.clone(),
            "(standalone skills)",
            &mut ctx,
            &mut results,
        );
    }

    results
}

/// Discover and load skills for a group, applying pre-fetch filtering.
///
/// Checks group-level `depends-on` predicates against `for_crates` before
/// fetching git sources, to avoid unnecessary downloads. Each returned skill is
/// paired with the origin hash it was discovered through — its group's origin
/// key combined
/// with the SKILL.md's path within that origin, one per discovered SKILL.md.
async fn load_skills_for_group(
    sym: &Symposium,
    parsed: &ParsedPlugin,
    group: &SkillGroup,
    ctx: &mut PredicateContext<'_>,
    update: UpdateLevel,
) -> Vec<(Skill, String)> {
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

    let resolved = resolve_group_dirs(sym, parsed, group, update).await;
    let skills = collect_skills_from_dirs(resolved, group);

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

/// A base directory resolved for a skill group, with the report labels for the
/// skills discovered inside it. Both `source` variants reduce to this: `Path` is
/// already on disk; `Git` is fetched via the git cache. (A crate is not a group
/// source — it becomes a plugin through a `[[plugins]]` chained reference; see
/// [`expand_chained_plugins`].)
struct ResolvedSkillDir {
    dir: PathBuf,
    /// `SkillSourceSearched` report `plugin` label.
    plugin_label: String,
    /// `SkillSourceSearched` report `source` label.
    source_label: String,
}

/// Resolve a group's `source` to the base directories to scan. Both variants
/// land on a directory to walk; the only difference is whether the base is local
/// (`Path`) or fetched (`Git` via a git cache). Skill identity is not decided
/// here — every discovered skill's origin is the hash of its on-disk `SKILL.md`
/// path (see the module-level note above `hash_origin_key`).
async fn resolve_group_dirs(
    sym: &Symposium,
    parsed: &ParsedPlugin,
    group: &SkillGroup,
    update: UpdateLevel,
) -> Vec<ResolvedSkillDir> {
    let plugin = &parsed.plugin;
    let plugin_path = parsed.path.as_path();

    match &group.source {
        PluginSource::Path(p) => {
            let plugin_dir = plugin_path.parent().unwrap_or(plugin_path);
            let dir = plugin_dir.join(p);
            let dir = dir.canonicalize().unwrap_or(dir);
            let rel = dir
                .strip_prefix(&parsed.source_dir)
                .unwrap_or(&dir)
                .display()
                .to_string();

            vec![ResolvedSkillDir {
                dir,
                plugin_label: plugin.name.clone(),
                source_label: format!("path:{rel}"),
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
            }]
        }
    }
}

/// Depth limit for `[[plugins]]` chained-reference expansion, bounding both
/// intentional chains and redirect loops that slip past cycle detection.
const MAX_CHAIN_DEPTH: usize = 10;

/// Warn when a crate-embedded plugin declares extension types the chained
/// path doesn't dispatch yet. A crate plugin's *skills* and its own further
/// `[[plugins]]` edges are wired in; its hooks, MCP servers, subcommands, and
/// custom predicates are parsed and carried but not yet routed into their
/// dispatch paths.
fn warn_undispatched_crate_features(parsed: &ParsedPlugin) {
    let p = &parsed.plugin;
    let mut kinds = Vec::new();
    if !p.hooks.is_empty() {
        kinds.push("hooks");
    }
    if !p.mcp_servers.is_empty() {
        kinds.push("mcp_servers");
    }
    if !p.subcommands.is_empty() {
        kinds.push("subcommands");
    }
    if !p.custom_predicates.is_empty() {
        kinds.push("predicates");
    }
    if !kinds.is_empty() {
        tracing::warn!(
            plugin = %p.name,
            features = %kinds.join(", "),
            "crate-embedded plugin declares extension types that are not yet dispatched \
             (only its skills and chained references are loaded today)"
        );
    }
}

/// Expand an active plugin's `[[plugins]]` chained references, recursively.
///
/// For each edge whose predicates hold (evaluated against the *owning* plugin's
/// provenance), the referenced crate is loaded as a first-class plugin via
/// [`CargoPm::load_plugin`], its own plugin-level predicates are honored, and
/// its skills are contributed with crate-origin identity. The loaded
/// crate's own chained edges are then expanded in turn — this is how a crate
/// that names another crate (a reschema'd `[package.metadata.symposium]`
/// redirect) is followed.
///
/// `visited` holds the normalized crate names already loaded on this owning
/// plugin's chain; it collapses diamonds (a crate reached two ways loads once)
/// and breaks cycles. It is scoped per top-level plugin — cross-plugin dedup
/// stays the sync layer's job (via the origin hash). `depth`/[`MAX_CHAIN_DEPTH`]
/// is a backstop.
#[allow(clippy::too_many_arguments)]
async fn expand_chained_plugins(
    sym: &Symposium,
    owner: &ParsedPlugin,
    workspace_crates: &[symposium_sdk::workspace::WorkspaceCrate],
    ctx: &mut PredicateContext<'_>,
    update: UpdateLevel,
    visited: &mut std::collections::HashSet<String>,
    depth: usize,
    results: &mut Vec<SkillWithGroupContext>,
) {
    if depth >= MAX_CHAIN_DEPTH {
        tracing::warn!(
            plugin = %owner.plugin.name,
            "chained plugin expansion exceeded depth limit ({MAX_CHAIN_DEPTH}); stopping"
        );
        return;
    }

    for chained in &owner.plugin.chained {
        // Edge predicates evaluate against the owning plugin's provenance; the
        // crate plugin's own `applies` (below) restamps its own — never a
        // workspace member — so reset before each edge's gate.
        ctx.set_workspace_member(owner.workspace_member);
        if !chained.predicates.evaluate(ctx) {
            continue;
        }

        let Some(crate_plugin) = crate::pm::CargoPm
            .load_plugin(&chained.name, workspace_crates)
            .await
        else {
            continue;
        };

        // Cycle / diamond detection on the resolved crate identity, normalized
        // so hyphen/underscore spellings of one crate collapse.
        let key = crate::crate_sources::normalize_crate_name(&crate_plugin.canonical.name);
        if !visited.insert(key) {
            tracing::debug!(
                crate_name = %chained.name,
                "chained plugin already loaded on this chain; skipping (cycle or diamond)"
            );
            continue;
        }

        // Honor the crate plugin's own plugin-level predicates (which stamp its
        // provenance: never a workspace member) before doing anything with it —
        // an inactive crate plugin shouldn't warn about undispatched features.
        if !crate_plugin.applies(ctx) {
            continue;
        }
        warn_undispatched_crate_features(&crate_plugin);

        for group in &crate_plugin.plugin.skills {
            let skills = load_skills_for_group(sym, &crate_plugin, group, ctx, update).await;
            for (skill, origin_hash) in skills {
                collect_skill_applicable_to(
                    skill,
                    origin_hash,
                    &crate_plugin.plugin.name,
                    ctx,
                    results,
                );
            }
        }

        Box::pin(expand_chained_plugins(
            sym,
            &crate_plugin,
            workspace_crates,
            ctx,
            update,
            visited,
            depth + 1,
            results,
        ))
        .await;
    }
}

/// Discover skills in each resolved base dir and stamp origins. The single
/// path all group sources funnel through, replacing the former per-source
/// `load_*_skills` functions.
fn collect_skills_from_dirs(
    resolved: Vec<ResolvedSkillDir>,
    group: &SkillGroup,
) -> Vec<(Skill, String)> {
    let mut skills = Vec::new();
    for entry in resolved {
        let discovered = discover_skills(&entry.dir, group.workspace_member, &group.predicates);
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
                Ok(skill) => {
                    let origin_hash = hash_origin_key(&(skill.path.to_string_lossy()));
                    skills.push((skill, origin_hash));
                }
                Err(e) => tracing::warn!(
                    source = %entry.source_label,
                    error = %e,
                    "failed to load skill",
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
    if !skills_dir.is_dir() {
        return Vec::new();
    }

    let mut skill_files = Vec::new();
    find_skill_files_recursive(skills_dir, &mut skill_files);
    prune_nested_skills(&mut skill_files);

    skill_files
        .into_iter()
        .map(|skill_md| load_skill(&skill_md, workspace_member, group_predicates))
        .collect()
}

/// Recursively walk a directory collecting paths to `SKILL.md` files.
///
/// Directories carrying the `.symposium` marker are skipped: the marker means
/// symposium itself installed the directory, and installed copies are never
/// sources. This matters for `.agents/skills/`, which is both a workspace
/// skill-group source and the install destination for vendor-neutral agents.
pub(crate) fn find_skill_files_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    if dir.join(crate::sync::MARKER_FILE).exists() {
        return;
    }
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
/// Standalone skills must be self-contained: all metadata (`depends-on`)
/// comes from the SKILL.md frontmatter.
/// Returns an error if `depends-on` is missing (standalone skills have
/// no group to inherit from).
pub fn load_standalone_skill(skill_md_path: &Path) -> Result<Skill> {
    let skill = load_skill(skill_md_path, false, &PredicateSet::default())?;
    if !skill.predicates.mentions_dep() {
        bail!(
            "standalone skill `{}` is missing `depends-on` in frontmatter \
             (standalone skills have no plugin group to inherit from)",
            skill.name()
        );
    }
    Ok(skill)
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
fn load_skill(
    skill_md_path: &Path,
    workspace_member: bool,
    group_predicates: &PredicateSet,
) -> Result<Skill> {
    let content = std::fs::read_to_string(skill_md_path)
        .with_context(|| format!("failed to read {}", skill_md_path.display()))?;

    let fm = if workspace_member && !content.trim_start().starts_with("---") {
        RawFrontmatter {
            fields: BTreeMap::new(),
            depends_on: None,
            predicates: None,
            body: content,
        }
    } else {
        parse_frontmatter(&content).with_context(|| {
            format!("failed to parse frontmatter in {}", skill_md_path.display())
        })?
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
        && let Some(dir_name) = skill_md_path
            .parent()
            .and_then(|dir| dir.file_name())
            .and_then(|name| name.to_str())
    {
        frontmatter.insert("name".to_string(), dir_name.to_string());
    }

    let name = frontmatter
        .get("name")
        .context("SKILL.md frontmatter missing required `name` field")?;

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
    let depends_on = match fm.depends_on.as_deref() {
        Some(s) => Some(crate::predicate::DependsOnList::parse(s)?),
        None => None,
    };
    let extra = match fm.predicates.as_deref() {
        Some(s) => PredicateSet::parse(s)?,
        None => PredicateSet::default(),
    };
    let predicates = PredicateSet::merged(depends_on, extra);

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
    origin_hash: String,
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
    results.push(SkillWithGroupContext { skill, origin_hash });
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
    let mut depends_on = None;
    let mut predicates = None;

    for (key, value) in mapping {
        let Some(key) = key.as_str() else {
            bail!("SKILL.md frontmatter keys must be strings");
        };

        let Some(value) = value.as_str() else {
            bail!("SKILL.md frontmatter field `{key}` must be a string");
        };

        match key {
            "depends-on" => depends_on = Some(value.to_string()),
            "crates" => {
                bail!("the `crates` frontmatter field has been renamed; use `depends-on` instead")
            }
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
        body: body.to_string(),
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
        assert!(fm.body.contains("# Body content"));
        assert!(fm.body.contains("Some instructions here."));
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
                description: Plugin provides crates
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
                depends-on: serde
                ---

                Standalone body.
            "},
        )
        .unwrap();

        let skill = load_standalone_skill(&skill_dir.join("SKILL.md")).unwrap();
        assert_eq!(skill.name(), "my-standalone");
        assert!(skill.predicates.references_dep("serde"));
        assert!(skill.body.contains("Standalone body."));
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
        assert_eq!(skill.body, "Plain maintainer notes.\n");

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

        let err = load_standalone_skill(&skill_dir.join("SKILL.md")).unwrap_err();
        assert!(
            err.to_string().contains("depends-on predicate"),
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
                source: PluginSource::Path(PathBuf::from("skills")),
                workspace_member: false,
            }],
            mcp_servers: vec![],
            installations: Vec::new(),
            subcommands: BTreeMap::new(),
            custom_predicates: vec![],
            chained: vec![],
        };

        let registry = PluginRegistry {
            plugins: vec![ParsedPlugin {
                canonical: PackageId::new("test", &plugin.name, ANY_VERSION),
                path: tmp.path().join("plugin.toml"),
                plugin,
                source_dir: tmp.path().to_path_buf(),
                workspace_member: false,
            }],
            standalone_skills: vec![],
            warnings: vec![],
            custom_predicates: crate::plugins::CustomPredicateRegistry::default(),
        };

        // Query for serde - should find no skills because plugin doesn't apply
        let workspace_crates = vec![symposium_sdk::workspace::WorkspaceCrate::new(
            "serde".to_string(),
            semver::Version::new(1, 0, 0),
            None,
        )];
        let skills = skills_applicable_to(
            &sym,
            &registry,
            &workspace_crates,
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
                source: PluginSource::Path(PathBuf::from("skills")),
                workspace_member: false,
            }],
            mcp_servers: vec![],
            installations: Vec::new(),
            subcommands: BTreeMap::new(),
            custom_predicates: vec![],
            chained: vec![],
        };

        let registry = PluginRegistry {
            plugins: vec![ParsedPlugin {
                canonical: PackageId::new("test", &plugin.name, ANY_VERSION),
                path: tmp.path().join("plugin.toml"),
                plugin,
                source_dir: tmp.path().to_path_buf(),
                workspace_member: false,
            }],
            standalone_skills: vec![],
            warnings: vec![],
            custom_predicates: crate::plugins::CustomPredicateRegistry::default(),
        };

        // Query for serde - should find no skills because group doesn't match
        let workspace_crates = vec![symposium_sdk::workspace::WorkspaceCrate::new(
            "serde".to_string(),
            semver::Version::new(1, 0, 0),
            None,
        )];
        let skills = skills_applicable_to(
            &sym,
            &registry,
            &workspace_crates,
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
                depends-on: serde
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
                workspace_member: false,
            }],
            mcp_servers: vec![],
            installations: Vec::new(),
            subcommands: BTreeMap::new(),
            custom_predicates: vec![],
            chained: vec![],
        };

        let registry = PluginRegistry {
            plugins: vec![ParsedPlugin {
                canonical: PackageId::new("test", &plugin.name, ANY_VERSION),
                path: tmp.path().join("plugin.toml"),
                plugin,
                source_dir: tmp.path().to_path_buf(),
                workspace_member: false,
            }],
            standalone_skills: vec![],
            warnings: vec![],
            custom_predicates: crate::plugins::CustomPredicateRegistry::default(),
        };

        let workspace_crates = vec![symposium_sdk::workspace::WorkspaceCrate::new(
            "serde".to_string(),
            semver::Version::new(1, 0, 0),
            None,
        )];
        let skills = skills_applicable_to(
            &sym,
            &registry,
            &workspace_crates,
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
                depends-on: serde
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
                    Predicate::DependsOn("serde".into(), None),
                    Predicate::Shell("false".into()),
                ],
            },
            hooks: vec![],
            skills: vec![SkillGroup {
                predicates: pred_set("serde"),
                source: PluginSource::Path(skill_dir.to_path_buf()),
                workspace_member: false,
            }],
            mcp_servers: vec![],
            installations: Vec::new(),
            subcommands: Default::default(),
            custom_predicates: vec![],
            chained: vec![],
        };

        let registry = PluginRegistry {
            plugins: vec![ParsedPlugin {
                canonical: PackageId::new("test", &plugin.name, ANY_VERSION),
                path: tmp.path().join("plugin.toml"),
                plugin,
                source_dir: PathBuf::from(".".to_string()),
                workspace_member: false,
            }],
            standalone_skills: vec![],
            warnings: vec![],
            custom_predicates: crate::plugins::CustomPredicateRegistry::default(),
        };

        let workspace = vec![symposium_sdk::workspace::WorkspaceCrate::new(
            "serde".into(),
            semver::Version::new(1, 0, 0),
            None,
        )];
        let skills = skills_applicable_to(
            &sym,
            &registry,
            &workspace,
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
                depends-on: serde
                ---

                Body.
            "},
        )
        .unwrap();

        let plugin = Plugin {
            name: "p".into(),
            predicates: PredicateSet {
                predicates: vec![
                    Predicate::DependsOn("serde".into(), None),
                    Predicate::Shell("true".into()),
                ],
            },
            hooks: vec![],
            skills: vec![SkillGroup {
                predicates: PredicateSet {
                    predicates: vec![
                        Predicate::DependsOn("serde".into(), None),
                        Predicate::Shell("true".into()),
                    ],
                },
                source: PluginSource::Path(skill_dir.to_path_buf()),
                workspace_member: false,
            }],
            mcp_servers: vec![],
            installations: Vec::new(),
            subcommands: Default::default(),
            custom_predicates: vec![],
            chained: vec![],
        };

        let registry = PluginRegistry {
            plugins: vec![ParsedPlugin {
                canonical: PackageId::new("test", &plugin.name, ANY_VERSION),
                path: tmp.path().join("plugin.toml"),
                plugin,
                source_dir: PathBuf::from(".".to_string()),
                workspace_member: false,
            }],
            standalone_skills: vec![],
            warnings: vec![],
            custom_predicates: crate::plugins::CustomPredicateRegistry::default(),
        };

        let workspace = vec![symposium_sdk::workspace::WorkspaceCrate::new(
            "serde".into(),
            semver::Version::new(1, 0, 0),
            None,
        )];
        let skills = skills_applicable_to(
            &sym,
            &registry,
            &workspace,
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
                Predicate::DependsOn("serde".into(), None),
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

        let err = load_standalone_skill(&skill_dir.join("SKILL.md")).unwrap_err();
        assert!(
            err.to_string().contains("missing required `name` field"),
            "expected missing name error, got: {err}"
        );
    }

    #[test]
    fn standalone_skill_requires_depends_on() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("no-depends-on");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            indoc! {"
                ---
                name: no-depends-on
                description: Missing depends-on
                ---

                Body.
            "},
        )
        .unwrap();

        let err = load_standalone_skill(&skill_dir.join("SKILL.md")).unwrap_err();
        assert!(
            err.to_string().contains("missing `depends-on`"),
            "expected depends-on error, got: {err}"
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
                depends-on: serde
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
                origin_hash: "test-myskill".to_string(),
            }],
            warnings: vec![],
            custom_predicates: crate::plugins::CustomPredicateRegistry::default(),
        };

        let sym = crate::config::Symposium::from_dir(tmp.path());
        let workspace = vec![symposium_sdk::workspace::WorkspaceCrate::new(
            "serde".to_string(),
            semver::Version::new(1, 0, 0),
            None,
        )];
        let results = skills_applicable_to(
            &sym,
            &registry,
            &workspace,
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
