//! Skill model, frontmatter parsing, discovery, and crate advice output.
//!
//! Skills follow the [agentskills.io](https://agentskills.io/specification.md) format
//! and live inside plugin directories under `skills/*/SKILL.md`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::config::Symposium;
use crate::plugins::{ParsedPlugin, PluginRegistry, SkillGroup};
use crate::predicate::{self, Predicate};

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
    /// Check whether this skill advises on the given crate.
    ///
    /// Returns `true` if any skill-level `crates` predicate references the crate,
    /// or if the skill has no skill-level `crates` (inheriting from the group).
    pub fn advises_on(&self, crate_name: &str) -> bool {
        self.crates.is_empty() || self.crates.iter().any(|p| p.references_crate(crate_name))
    }

    /// Return the skill name from frontmatter, or "unknown".
    pub fn name(&self) -> &str {
        self.frontmatter
            .get("name")
            .map_or("unknown", |s| s.as_str())
    }

    /// Return the crate names referenced by this skill's `crates` predicates.
    pub fn crate_names(&self) -> Vec<String> {
        let mut names = std::collections::BTreeSet::new();
        for pred in &self.crates {
            pred.collect_crate_names(&mut names);
        }
        names.into_iter().collect()
    }
}

/// A skill paired with its group's crate predicates, for display purposes.
pub struct SkillWithGroupContext {
    pub skill: Skill,
    /// Group-level crate predicates (used when the skill has none of its own).
    pub group_crates: Vec<Predicate>,
}

impl SkillWithGroupContext {
    /// Return the effective crate names this skill applies to.
    ///
    /// Uses skill-level crates if present, otherwise falls back to group-level.
    pub fn effective_crate_names(&self) -> Vec<String> {
        let mut names = std::collections::BTreeSet::new();
        let preds = if !self.skill.crates.is_empty() {
            &self.skill.crates
        } else {
            &self.group_crates
        };
        for pred in preds {
            pred.collect_crate_names(&mut names);
        }
        names.into_iter().collect()
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
    for_crates: &[(String, semver::Version)],
) -> Vec<SkillWithGroupContext> {
    let mut results = Vec::new();

    // Skills from plugin manifests. We iterate these separately
    // because we lazily load skill groups, so there
    // is extra logic.
    for ParsedPlugin { path, plugin } in &registry.plugins {
        // First check if plugin applies to these crates
        if !plugin.applies_to_crates(for_crates) {
            continue;
        }

        for group in &plugin.skills {
            let (group_crates, skills) = load_skills_for_group(sym, path, group, for_crates).await;

            collect_skills_applicable_to(&skills, &group_crates, for_crates, &mut results);
        }
    }

    // Standalone skills -- these are already loaded as part of the plugin
    // registry.
    collect_skills_applicable_to(&registry.standalone_skills, &[], for_crates, &mut results);

    results
}

/// Discover and load skills for a group, applying pre-fetch filtering.
///
/// Checks group-level `crates` predicates against `for_crates` before
/// fetching git sources, to avoid unnecessary downloads.
async fn load_skills_for_group(
    sym: &Symposium,
    plugin_path: &Path,
    group: &SkillGroup,
    for_crates: &[(String, semver::Version)],
) -> (Vec<Predicate>, Vec<Skill>) {
    let group_crates = group.crates.as_deref().unwrap_or_default();

    // Pre-fetch filtering: skip groups whose crate predicates don't match any target.
    if !group_crates.is_empty() && !group_crates.iter().any(|p| p.matches(for_crates)) {
        tracing::debug!(plugin = %plugin_path.display(), "skill group crates don't match, skipping");
        return (group_crates.to_vec(), Vec::new());
    }

    let Some(dir) = resolve_skill_dir(sym, plugin_path, group).await else {
        return (group_crates.to_vec(), Vec::new());
    };

    let mut skills = Vec::new();
    for result in discover_skills(&dir, group) {
        match result {
            Ok(skill) => skills.push(skill),
            Err(e) => {
                tracing::warn!(plugin = %plugin_path.display(), error = %e, "failed to load skill")
            }
        }
    }

    (group_crates.to_vec(), skills)
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

/// Fetch a skill group's git source, returning the cached directory path.
async fn fetch_skill_source(sym: &Symposium, git_url: &str) -> Result<PathBuf> {
    let source = crate::git_source::parse_github_url(git_url)?;
    let cache_mgr = crate::git_source::PluginCacheManager::new(sym, "plugins");
    cache_mgr
        .get_or_fetch(&source, git_url, crate::git_source::UpdateLevel::None)
        .await
}

/// Resolve the skill directory for a group, fetching from git if needed.
///
/// Returns `None` if the group has no source and the plugin has no local dir.
async fn resolve_skill_dir(
    sym: &Symposium,
    plugin_path: &Path,
    group: &SkillGroup,
) -> Option<PathBuf> {
    if let Some(path) = &group.source.path {
        return Some(plugin_path.join(path));
    }

    if let Some(git_source) = &group.source.git {
        match fetch_skill_source(sym, &git_source).await {
            Ok(path) => return Some(path),
            Err(e) => {
                tracing::warn!(git = %git_source, error = %e, "failed to fetch skill source");
                return None;
            }
        }
    }

    None
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
    if let Some(name) = frontmatter.get_mut("name") {
        if let Some(unquoted) = name.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            *name = unquoted.to_string();
        }
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

/// Filter skills by crate constraints, collecting matches with group context.
fn collect_skills_applicable_to(
    skills: &[Skill],
    group_crates: &[Predicate],
    for_crates: &[(String, semver::Version)],
    results: &mut Vec<SkillWithGroupContext>,
) {
    for skill in skills {
        if !skill_matches(skill, group_crates, for_crates) {
            continue;
        }
        results.push(SkillWithGroupContext {
            skill: skill.clone(),
            group_crates: group_crates.to_vec(),
        });
    }
}

/// Check whether a skill matches the target crates.
///
/// If both skill-level and group-level `crates` are present, BOTH must match
/// (AND composition). If only one level has `crates`, that level alone decides.
/// Returns false if neither level has any crate predicates.
fn skill_matches(
    skill: &Skill,
    group_crates: &[Predicate],
    for_crates: &[(String, semver::Version)],
) -> bool {
    let skill_ok = if skill.crates.is_empty() {
        None
    } else {
        Some(skill.crates.iter().any(|p| p.matches(for_crates)))
    };
    let group_ok = if group_crates.is_empty() {
        None
    } else {
        Some(group_crates.iter().any(|p| p.matches(for_crates)))
    };
    match (skill_ok, group_ok) {
        (Some(s), Some(g)) => s && g, // AND: both must match
        (Some(s), None) => s,
        (None, Some(g)) => g,
        (None, None) => false,
    }
}

/// Raw frontmatter fields extracted from a SKILL.md file.
/// `crates` is comma-separated on a single line.
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

    let mut fields = BTreeMap::new();
    let mut crates = None;

    for line in frontmatter_text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim().to_string();
            if key == "crates" {
                crates = Some(value);
            } else if key == "applies-when" {
                // Ignored — applies-when is no longer supported.
            } else {
                fields.insert(key.to_string(), value);
            }
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
            crates: Some(vec![pred("tokio")]),
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
            crates: Some(vec![pred("tokio")]),
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
            crates: Some(vec![pred("serde")]),
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
                crates: >=not_valid!!
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
            crates: vec![pred("other-crate")],
            installation: None,
            hooks: vec![],
            skills: vec![SkillGroup {
                crates: Some(vec![pred("serde")]), // Group targets serde
                source: PluginSource::default(),
            }],
            mcp_servers: vec![],
        };

        let registry = PluginRegistry {
            plugins: vec![ParsedPlugin {
                path: tmp.path().join("plugin.toml"),
                plugin,
            }],
            standalone_skills: vec![],
        };

        // Query for serde - should find no skills because plugin doesn't apply
        let workspace_crates = vec![("serde".to_string(), semver::Version::new(1, 0, 0))];
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
            crates: vec![pred("*")], // Plugin applies to all
            installation: None,
            hooks: vec![],
            skills: vec![SkillGroup {
                crates: Some(vec![pred("other-crate")]), // But group targets other-crate
                source: PluginSource::default(),
            }],
            mcp_servers: vec![],
        };

        let registry = PluginRegistry {
            plugins: vec![ParsedPlugin {
                path: tmp.path().join("plugin.toml"),
                plugin,
            }],
            standalone_skills: vec![],
        };

        // Query for serde - should find no skills because group doesn't match
        let workspace_crates = vec![("serde".to_string(), semver::Version::new(1, 0, 0))];
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
            crates: vec![pred("serde")], // Plugin targets serde
            installation: None,
            hooks: vec![],
            skills: vec![SkillGroup {
                crates: Some(vec![pred("serde")]), // Group also targets serde
                source: PluginSource {
                    path: Some(skill_dir.to_path_buf()),
                    git: None,
                },
            }],
            mcp_servers: vec![],
        };

        let registry = PluginRegistry {
            plugins: vec![ParsedPlugin {
                path: tmp.path().join("plugin.toml"),
                plugin,
            }],
            standalone_skills: vec![],
        };

        // Query for serde - should find the skill because all levels match
        let workspace_crates = vec![("serde".to_string(), semver::Version::new(1, 0, 0))];
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
            standalone_skills: vec![skill],
        };

        let sym = crate::config::Symposium::from_dir(tmp.path());
        let workspace = vec![("serde".to_string(), semver::Version::new(1, 0, 0))];
        let results = skills_applicable_to(&sym, &registry, &workspace).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].skill.name(), "standalone-serde");
        // No group context for standalone skills
        assert!(results[0].group_crates.is_empty());
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

    // --- AND composition tests for skill_matches ---

    fn v(s: &str) -> semver::Version {
        semver::Version::parse(s).unwrap()
    }

    #[test]
    fn skill_matches_and_both_present_both_satisfied() {
        // Skill crates: tokio, Group crates: serde → requires BOTH
        let skill = Skill {
            frontmatter: BTreeMap::new(),
            crates: vec![pred("tokio")],
            body: String::new(),
            path: PathBuf::new(),
        };
        let group = vec![pred("serde")];
        let ws = vec![("serde".into(), v("1.0.0")), ("tokio".into(), v("1.0.0"))];
        assert!(skill_matches(&skill, &group, &ws));
    }

    #[test]
    fn skill_matches_and_skill_missing_from_workspace() {
        // Skill crates: tokio, Group crates: serde, workspace has only serde → AND fails
        let skill = Skill {
            frontmatter: BTreeMap::new(),
            crates: vec![pred("tokio")],
            body: String::new(),
            path: PathBuf::new(),
        };
        let group = vec![pred("serde")];
        let ws = vec![("serde".into(), v("1.0.0"))];
        assert!(!skill_matches(&skill, &group, &ws));
    }

    #[test]
    fn skill_matches_no_skill_crates_uses_group() {
        // Skill has no crates, group has serde → only serde required
        let skill = Skill {
            frontmatter: BTreeMap::new(),
            crates: vec![],
            body: String::new(),
            path: PathBuf::new(),
        };
        let group = vec![pred("serde")];
        let ws = vec![("serde".into(), v("1.0.0"))];
        assert!(skill_matches(&skill, &group, &ws));
    }

    #[test]
    fn skill_matches_skill_crates_no_group() {
        // Skill has serde, group has nothing → only serde required
        let skill = Skill {
            frontmatter: BTreeMap::new(),
            crates: vec![pred("serde")],
            body: String::new(),
            path: PathBuf::new(),
        };
        let ws = vec![("serde".into(), v("1.0.0"))];
        assert!(skill_matches(&skill, &[], &ws));
    }

    #[test]
    fn skill_matches_neither_level_returns_false() {
        let skill = Skill {
            frontmatter: BTreeMap::new(),
            crates: vec![],
            body: String::new(),
            path: PathBuf::new(),
        };
        let ws = vec![("serde".into(), v("1.0.0"))];
        assert!(!skill_matches(&skill, &[], &ws));
    }
}
