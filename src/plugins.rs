use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::Symposium;
use crate::git_source::UpdateLevel;
use crate::hook::HookEvent;
use crate::hook_schema::HookAgent;

use sacp::schema::McpServer;

/// An MCP server entry in a plugin manifest.
pub type McpServerEntry = McpServer;

/// Source declaration for remote plugin artifacts.
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct PluginSource {
    /// Path on the local filesystem.
    pub path: Option<PathBuf>,

    /// GitHub URL pointing to a directory in a repository.
    pub git: Option<String>,
}

/// A `[[skills]]` entry from a plugin manifest.
///
/// Each group declares which crates it advises on (`crates`) and
/// optionally a remote source for the skill files.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillGroup {
    /// Crate predicates this group advises on (e.g., `"serde"` or `["serde", "serde_json>=1.0"]`).
    #[serde(default, deserialize_with = "deserialize_string_or_vec_opt")]
    pub crates: Option<Vec<crate::predicate::Predicate>>,
    /// Remote source for skills.
    #[serde(default)]
    pub source: PluginSource,
}

/// Deserialize a field that accepts either a single string or a vec of strings,
/// and parse each as a `Predicate`. Returns `None` if absent, `Some(vec)` otherwise.
fn deserialize_string_or_vec_opt<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<Vec<crate::predicate::Predicate>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct StringOrVec;

    impl<'de> de::Visitor<'de> for StringOrVec {
        type Value = Option<Vec<crate::predicate::Predicate>>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string or array of predicate strings")
        }

        fn visit_none<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Self::Value, E> {
            let pred = crate::predicate::parse(v).map_err(de::Error::custom)?;
            Ok(Some(vec![pred]))
        }

        fn visit_seq<A: de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut preds = Vec::new();
            while let Some(s) = seq.next_element::<String>()? {
                let pred = crate::predicate::parse(&s).map_err(de::Error::custom)?;
                preds.push(pred);
            }
            Ok(Some(preds))
        }
    }

    deserializer.deserialize_any(StringOrVec)
}

/// A parsed plugin with its path and manifest.
#[derive(Debug, Clone)]
pub struct ParsedPlugin {
    /// The path from which the plugin was parsed.
    pub path: PathBuf,

    /// The parsed plugin manifest.
    pub plugin: Plugin,
}

/// A loaded plugin manifest with hooks and skill groups.
///
/// This is a table of contents — it describes what skills and hooks are
/// available, but does not load skill content. The skills layer handles
/// discovery and loading.
#[derive(Debug, Clone, Serialize)]
pub struct Plugin {
    pub name: String,
    pub installation: Option<Installation>,
    pub hooks: Vec<Hook>,
    pub skills: Vec<SkillGroup>,
    /// MCP servers to register for this plugin.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<McpServerEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Installation {
    pub commands: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Hook {
    pub name: String,
    pub event: HookEvent,
    pub matcher: Option<String>,
    pub command: String,
    #[serde(default)]
    pub format: HookFormat,
}

/// The wire format a plugin hook expects for input/output.
///
/// This is distinct from `HookAgent` because:
/// - `Symposium` is a wire format but not an agent (no CLI invokes hooks
///   in symposium format natively).
/// - Not all agents have hook wire formats (e.g., Goose uses MCP extensions,
///   OpenCode uses JS plugins), so only agents with shell-hook JSON formats
///   appear here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HookFormat {
    /// Symposium canonical format (default).
    Symposium,
    /// A specific agent's wire format.
    Claude,
    Codex,
    Copilot,
    Gemini,
    Kiro,
}

impl Default for HookFormat {
    fn default() -> Self {
        HookFormat::Symposium
    }
}

impl HookFormat {
    /// Convert to the corresponding HookAgent, if this is an agent format.
    pub fn as_agent(&self) -> Option<HookAgent> {
        match self {
            HookFormat::Symposium => None,
            HookFormat::Claude => Some(HookAgent::Claude),
            HookFormat::Codex => Some(HookAgent::Codex),
            HookFormat::Copilot => Some(HookAgent::Copilot),
            HookFormat::Gemini => Some(HookAgent::Gemini),
            HookFormat::Kiro => Some(HookAgent::Kiro),
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct ProviderInfo {
    pub name: String,
    pub source_type: &'static str,
    pub git_url: Option<String>,
    pub path: Option<String>,
    pub plugins: Vec<PluginInfo>,
}

#[derive(Debug, serde::Serialize)]
pub struct PluginInfo {
    pub name: String,
    pub hooks_count: usize,
    pub skill_groups_count: usize,
}

/// Loaded plugin registry: plugins from TOML manifests and standalone skills
/// discovered directly in plugin source directories.
#[derive(Debug)]
pub struct PluginRegistry {
    /// Plugins loaded from `.toml` manifest files.
    pub plugins: Vec<ParsedPlugin>,
    /// Skills discovered as standalone directories containing a `SKILL.md`
    /// file directly in a plugin source directory (no TOML manifest needed).
    pub standalone_skills: Vec<crate::skills::Skill>,
}

/// Raw scan results from a plugin source directory.
#[derive(Debug)]
struct SourceDirContents {
    plugins: Vec<Result<ParsedPlugin>>,
    /// Paths to discovered `SKILL.md` files (after recursive search and pruning).
    skill_files: Vec<PathBuf>,
}

/// Raw TOML manifest deserialized from a plugin `.toml` file.
#[derive(Debug, Deserialize)]
struct PluginManifest {
    name: String,
    #[serde(default)]
    installation: Option<Installation>,
    #[serde(default)]
    hooks: Vec<Hook>,
    #[serde(default)]
    skills: Vec<SkillGroup>,
    #[serde(default)]
    mcp_servers: Vec<McpServerEntry>,
}

/// Fetch/update git-based plugin sources.
///
/// Ensure git-based plugin sources are up to date.
///
/// `update` controls freshness checking behavior (see `UpdateLevel`).
/// Only refreshes sources with `auto-update = true` (unless `update` is `Fetch`).
/// Path-based sources are skipped (no fetching needed).
pub async fn ensure_plugin_sources(sym: &Symposium, update: UpdateLevel) {
    let sources = sym.plugin_sources();

    for resolved in &sources {
        let source = &resolved.source;
        if !matches!(update, UpdateLevel::Fetch) && !source.auto_update {
            tracing::debug!(source = %source.name, "skipping (auto-update disabled)");
            continue;
        }

        let Some(ref git_url) = source.git else {
            tracing::debug!(source = %source.name, "skipping (can only auto-update git)");
            continue;
        };

        tracing::debug!(source = %source.name, url = %git_url, "ensuring plugin source");

        match fetch_plugin_source(sym, git_url, update).await {
            Ok(path) => {
                tracing::debug!(source = %source.name, path = %path.display(), "plugin source ready");
            }
            Err(e) => {
                tracing::warn!(source = %source.name, git_url = %git_url, error = %e, "failed to fetch plugin source");
            }
        }
    }
}

/// Load all plugins from all configured plugin source directories,
/// discarding load errors with warnings.
///
/// Use `load_registry()` instead if you also need standalone skills.
pub fn load_all_plugins(sym: &Symposium) -> Vec<ParsedPlugin> {
    load_registry(sym).plugins
}

/// Sync plugin sources.
///
/// If `provider` is Some, sync only that provider (ignores auto-update).
/// If `provider` is None, sync all sources with auto-update = true.
pub async fn sync_plugin_source(sym: &Symposium, provider: Option<&str>) -> Result<Vec<String>> {
    let sources = sym.plugin_sources();
    let mut synced = Vec::new();

    for resolved in &sources {
        let source = &resolved.source;
        if let Some(name) = provider {
            if source.name != name {
                continue;
            }
        } else if !source.auto_update {
            tracing::debug!(source = %source.name, "skipping (auto-update disabled)");
            continue;
        }

        if let Some(ref git_url) = source.git {
            tracing::debug!(source = %source.name, url = %git_url, "syncing plugin source");
            match fetch_plugin_source(sym, git_url, UpdateLevel::Fetch).await {
                Ok(path) => {
                    tracing::info!(source = %source.name, path = %path.display(), "synced");
                    synced.push(source.name.clone());
                }
                Err(e) => {
                    tracing::warn!(source = %source.name, error = %e, "failed to sync");
                }
            }
        } else {
            tracing::debug!(source = %source.name, "skipping path-based source");
        }
    }

    Ok(synced)
}

/// List all providers and their plugins.
pub fn list_plugins(sym: &Symposium) -> Vec<ProviderInfo> {
    let sources = sym.plugin_sources();
    let mut providers = Vec::new();

    for resolved in &sources {
        let source = &resolved.source;
        let source_path = resolve_plugin_source_dir(sym, resolved);
        let plugins: Vec<PluginInfo> = source_path
            .and_then(|p| scan_source_dir(&p).ok())
            .map(|c| c.plugins)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|r| r.ok())
            .map(|ParsedPlugin { path: _, plugin: p }| PluginInfo {
                name: p.name,
                hooks_count: p.hooks.len(),
                skill_groups_count: p.skills.len(),
            })
            .collect();

        providers.push(ProviderInfo {
            name: source.name.clone(),
            source_type: if source.git.is_some() { "git" } else { "path" },
            git_url: source.git.clone(),
            path: source.path.clone(),
            plugins,
        });
    }

    providers
}

/// Find a plugin by name across all sources.
pub fn find_plugin(sym: &Symposium, name: &str) -> Option<ParsedPlugin> {
    let sources = sym.plugin_sources();

    for resolved in &sources {
        let source_path = resolve_plugin_source_dir(sym, resolved);
        if let Some(ref path) = source_path {
            if let Ok(contents) = scan_source_dir(path) {
                for result in contents.plugins {
                    if let Ok(parsed_plugin) = result {
                        if parsed_plugin.plugin.name == name {
                            return Some(parsed_plugin);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Resolve the directories for all configured plugin sources.
///
/// For `path` sources: resolves relative to the source's `base_dir`, or uses absolute paths as-is.
/// For `git` sources: computes the cache path under `~/.symposium/cache/plugin-sources/`.
///
/// Does no network I/O — just computes paths.
fn resolve_plugin_source_dirs(
    sym: &Symposium,
    sources: &[crate::config::ResolvedPluginSource],
) -> Vec<PathBuf> {
    let cache_base = sym.cache_dir().join("plugin-sources");

    let mut dirs = Vec::new();
    for resolved in sources {
        if let Some(dir) = resolve_one_source(&resolved.source, &resolved.base_dir, &cache_base) {
            dirs.push(dir);
        }
    }
    dirs
}

fn resolve_plugin_source_dir(
    sym: &Symposium,
    resolved: &crate::config::ResolvedPluginSource,
) -> Option<PathBuf> {
    let cache_base = sym.cache_dir().join("plugin-sources");
    resolve_one_source(&resolved.source, &resolved.base_dir, &cache_base)
}

fn resolve_one_source(
    source: &crate::config::PluginSourceConfig,
    base_dir: &Path,
    cache_base: &Path,
) -> Option<PathBuf> {
    if let Some(ref path) = source.path {
        let p = PathBuf::from(path);
        if p.is_absolute() {
            return Some(p);
        } else {
            return Some(base_dir.join(p));
        }
    } else if let Some(ref git_url) = source.git {
        match crate::git_source::parse_github_url(git_url) {
            Ok(gh) => return Some(cache_base.join(gh.cache_key())),
            Err(e) => {
                tracing::warn!(source = %source.name, error = %e, "bad plugin source URL");
            }
        }
    }
    None
}

/// Fetch a plugin source repository, returning the cached directory path.
async fn fetch_plugin_source(
    sym: &Symposium,
    git_url: &str,
    update: UpdateLevel,
) -> Result<PathBuf> {
    use crate::git_source;

    let source = git_source::parse_github_url(git_url)?;
    let cache_mgr = git_source::PluginCacheManager::new(sym, "plugin-sources");
    cache_mgr.get_or_fetch(&source, git_url, update).await
}

/// Scan all configured plugin source directories and load the registry.
///
/// Discovers TOML plugin manifests and standalone skill directories,
/// then loads both into a `PluginRegistry`.
pub fn load_registry(sym: &Symposium) -> PluginRegistry {
    let sources = sym.plugin_sources();
    let mut plugins = Vec::new();
    let mut standalone_skills = Vec::new();

    for dir in resolve_plugin_source_dirs(sym, &sources) {
        match scan_source_dir(&dir) {
            Ok(contents) => {
                for result in contents.plugins {
                    match result {
                        Ok(p) => plugins.push(p),
                        Err(e) => tracing::warn!(error = %e, "failed to load plugin"),
                    }
                }
                for skill_md in contents.skill_files {
                    match crate::skills::load_standalone_skill(&skill_md) {
                        Ok(skill) => standalone_skills.push(skill),
                        Err(e) => tracing::warn!(
                            path = %skill_md.display(),
                            error = %e,
                            "failed to load standalone skill"
                        ),
                    }
                }
            }
            Err(e) => {
                tracing::warn!(dir = %dir.display(), error = %e, "failed to scan plugin source dir");
            }
        }
    }

    PluginRegistry {
        plugins,
        standalone_skills,
    }
}

/// Scan a plugin source directory for TOML plugin manifests and standalone skills.
///
/// Discovery rules:
/// 1. Plugin = directory with `SYMPOSIUM.toml` file
/// 2. Skill = directory with `SKILL.md` file
/// 3. Plugin takes precedence over skill in the same directory
/// 4. Once a directory is claimed as plugin/skill, don't recurse into it
fn scan_source_dir<P: AsRef<Path>>(dir: P) -> Result<SourceDirContents> {
    let mut plugins = Vec::new();
    let mut skill_files = Vec::new();

    let dir = dir.as_ref();

    // A plugin source should *contain* plugins/skills, not *be* one.
    if dir.join("SYMPOSIUM.toml").is_file() {
        anyhow::bail!(
            "plugin source root contains SYMPOSIUM.toml — it should contain subdirectories with plugins, not be a plugin itself: {}",
            dir.display()
        );
    }
    if dir.join("SKILL.md").is_file() {
        anyhow::bail!(
            "plugin source root contains SKILL.md — it should contain subdirectories with skills, not be a skill itself: {}",
            dir.display()
        );
    }

    discover_in_directory(dir, &mut plugins, &mut skill_files)?;

    Ok(SourceDirContents {
        plugins,
        skill_files,
    })
}

/// Recursively discover plugins and skills with precedence and pruning.
fn discover_in_directory(
    dir: &Path,
    plugins: &mut Vec<Result<ParsedPlugin>>,
    skill_files: &mut Vec<PathBuf>,
) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        // Check what this directory contains (plugin takes precedence)
        if let Some(discovered) = discover_directory_type(&path)? {
            match discovered {
                DirectoryType::Plugin(toml_path) => {
                    let plugin = load_plugin(&toml_path)
                        .with_context(|| format!("loading plugin from `{}`", toml_path.display()));

                    tracing::debug!(
                        path = %toml_path.display(),
                        plugin = ?plugin,
                        "loaded plugin",
                    );

                    plugins.push(plugin);
                }
                DirectoryType::Skill(skill_md_path) => {
                    tracing::debug!(
                        path = %skill_md_path.display(),
                        "found standalone skill",
                    );
                    skill_files.push(skill_md_path);
                }
            }
            // Don't recurse - directory is claimed
        } else {
            // Directory doesn't contain plugin/skill, recurse into it
            discover_in_directory(&path, plugins, skill_files)?;
        }
    }

    Ok(())
}

/// What type of directory this is (plugin or skill).
enum DirectoryType {
    Plugin(PathBuf), // Path to SYMPOSIUM.toml
    Skill(PathBuf),  // Path to SKILL.md file
}

/// Determine if a directory contains a plugin or skill.
/// Returns None if it contains neither.
/// SYMPOSIUM.toml takes precedence over SKILL.md.
fn discover_directory_type(dir: &Path) -> Result<Option<DirectoryType>> {
    // Check for SYMPOSIUM.toml (the only valid plugin manifest)
    let symposium_toml = dir.join("SYMPOSIUM.toml");
    if symposium_toml.is_file() {
        return Ok(Some(DirectoryType::Plugin(symposium_toml)));
    }

    // Check for SKILL.md
    let skill_md = dir.join("SKILL.md");
    if skill_md.is_file() {
        return Ok(Some(DirectoryType::Skill(skill_md)));
    }

    Ok(None)
}

/// Result of validating a single item in a plugin source directory.
#[derive(Debug)]
pub struct ValidationResult {
    /// Path to the validated file (TOML manifest or SKILL.md).
    pub path: PathBuf,
    /// What kind of item was validated.
    pub kind: ValidationKind,
    /// `Ok(())` if valid, `Err` with the validation error.
    pub result: Result<()>,
}

/// The kind of item that was validated.
#[derive(Debug)]
pub enum ValidationKind {
    Plugin,
    Skill,
}

impl std::fmt::Display for ValidationKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationKind::Plugin => write!(f, "plugin"),
            ValidationKind::Skill => write!(f, "skill"),
        }
    }
}

/// Validate a directory as a plugin source.
///
/// Scans for TOML plugin manifests and standalone SKILL.md files,
/// attempts to load each, and returns validation results for all items found.
pub fn validate_source_dir(dir: &Path) -> Result<Vec<ValidationResult>> {
    let contents = scan_source_dir(dir)?;
    let mut results = Vec::new();

    for plugin_result in contents.plugins {
        let (path, result) = match plugin_result {
            Ok(parsed) => (parsed.path, Ok(())),
            Err(e) => {
                // Extract the path from the error context if possible,
                // otherwise use a placeholder.
                let path = dir.join("<unknown>.toml");
                (path, Err(e))
            }
        };
        results.push(ValidationResult {
            path,
            kind: ValidationKind::Plugin,
            result,
        });
    }

    for skill_md in contents.skill_files {
        let result = crate::skills::load_standalone_skill(&skill_md).map(|_| ());
        results.push(ValidationResult {
            path: skill_md,
            kind: ValidationKind::Skill,
            result,
        });
    }

    Ok(results)
}

/// Collect all crate names referenced in predicates across a plugin source directory.
///
/// Scans TOML plugin manifests (skill group `crates`) and
/// standalone SKILL.md files, returning deduplicated crate names.
/// Items that fail to load are silently skipped.
pub fn collect_crate_names_in_source_dir(dir: &Path) -> Result<Vec<String>> {
    let contents = scan_source_dir(dir)?;
    let mut names = std::collections::BTreeSet::new();

    for plugin_result in contents.plugins.into_iter().flatten() {
        for group in &plugin_result.plugin.skills {
            if let Some(preds) = &group.crates {
                for pred in preds {
                    pred.collect_crate_names(&mut names);
                }
            }
        }
    }

    for skill_md in contents.skill_files {
        if let Ok(skill) = crate::skills::load_standalone_skill(&skill_md) {
            for pred in &skill.crates {
                pred.collect_crate_names(&mut names);
            }
        }
    }

    Ok(names.into_iter().collect())
}

/// Check whether a crate name exists on crates.io.
pub async fn check_crate_exists(crate_name: &str) -> bool {
    let client = match crates_io_api::AsyncClient::new(
        "symposium (https://github.com/symposium-dev/symposium)",
        std::time::Duration::from_millis(1000),
    ) {
        Ok(c) => c,
        Err(_) => return false,
    };
    client.get_crate(crate_name).await.is_ok()
}

/// Load a single plugin from a TOML manifest.
///
/// `local_dir` is the containing directory when the manifest lives inside a
/// plugin directory (used as fallback skill directory when no `source.git`).
pub fn load_plugin(manifest_path: &Path) -> Result<ParsedPlugin> {
    let content = fs::read_to_string(manifest_path)?;
    let manifest: PluginManifest = toml::from_str(&content)?;

    Ok(ParsedPlugin {
        path: manifest_path.to_path_buf(),
        plugin: Plugin {
            name: manifest.name,
            installation: manifest.installation,
            hooks: manifest.hooks,
            skills: manifest.skills,
            mcp_servers: manifest.mcp_servers,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;

    fn from_str(s: &str) -> Result<Plugin> {
        let manifest: PluginManifest = toml::from_str(s)?;
        Ok(Plugin {
            name: manifest.name,
            installation: manifest.installation,
            hooks: manifest.hooks,
            skills: manifest.skills,
            mcp_servers: manifest.mcp_servers,
        })
    }

    const SAMPLE: &str = indoc! {r#"
        name = "example-plugin"

        [installation]
        summary = "Download and install helper"
        commands = ["wget https://example.org/bin/tool"]

        [[hooks]]
        name = "test"
        event = "PreToolUse"
        command = "echo open"
    "#};

    #[test]
    fn parse_sample() {
        let plugin = from_str(SAMPLE).expect("parse");
        assert_eq!(plugin.name, "example-plugin");
        assert_eq!(plugin.hooks.len(), 1);
        assert!(plugin.skills.is_empty());
    }

    #[test]
    fn parse_manifest_with_source_git_under_skills() {
        let toml = indoc! {r#"
            name = "remote-plugin"

            [[skills]]
            crates = ["serde"]
            source.git = "https://github.com/org/repo/tree/main/serde"
        "#};
        let plugin = from_str(toml).expect("parse");
        assert_eq!(plugin.name, "remote-plugin");
        assert_eq!(plugin.skills.len(), 1);
        let group = &plugin.skills[0];
        let cr = group.crates.as_ref().unwrap();
        assert_eq!(cr.len(), 1);
        assert!(cr[0].references_crate("serde"));
        assert_eq!(
            group.source.git.as_ref().map(|s| s.as_str()),
            Some("https://github.com/org/repo/tree/main/serde")
        );
    }

    #[test]
    fn parse_manifest_crates_as_string() {
        let toml = indoc! {r#"
            name = "string-crates"

            [[skills]]
            crates = "serde"
        "#};
        let plugin = from_str(toml).expect("parse");
        let group = &plugin.skills[0];
        let cr = group.crates.as_ref().unwrap();
        assert_eq!(cr.len(), 1);
        assert!(cr[0].references_crate("serde"));
    }

    #[test]
    fn scan_source_dir_finds_plugins_and_standalone_skills() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        // Create a plugin directory
        let plugin_dir = dir.join("my-plugin");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(
            plugin_dir.join("SYMPOSIUM.toml"),
            indoc! {r#"
                name = "my-plugin"

                [[hooks]]
                name = "test"
                event = "PreToolUse"
                command = "echo hi"
            "#},
        )
        .unwrap();

        // Create a standalone skill directory
        let skill_dir = dir.join("assert-struct");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            indoc! {"
                ---
                name: assert-struct
                description: Check struct layout
                crates: serde
                ---

                Use this skill.
            "},
        )
        .unwrap();

        // Create a random directory without SYMPOSIUM.toml or SKILL.md (should be ignored)
        std::fs::create_dir_all(dir.join("not-a-plugin-or-skill")).unwrap();

        let contents = scan_source_dir(dir).unwrap();
        assert_eq!(contents.plugins.len(), 1);
        assert_eq!(
            contents.plugins[0].as_ref().unwrap().plugin.name,
            "my-plugin"
        );
        assert_eq!(contents.skill_files.len(), 1);
        assert!(contents.skill_files[0].ends_with("assert-struct/SKILL.md"));
    }

    #[test]
    fn scan_source_dir_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let contents = scan_source_dir(tmp.path()).unwrap();
        assert!(contents.plugins.is_empty());
        assert!(contents.skill_files.is_empty());
    }

    #[test]
    fn scan_source_dir_missing() {
        let contents = scan_source_dir("/nonexistent/path/abc123").unwrap();
        assert!(contents.plugins.is_empty());
        assert!(contents.skill_files.is_empty());
    }

    #[test]
    fn scan_source_dir_rejects_root_level_skill() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        std::fs::write(
            dir.join("SKILL.md"),
            indoc! {"
                ---
                name: root-skill
                crates: serde
                ---

                Root level skill.
            "},
        )
        .unwrap();

        let err = scan_source_dir(dir).unwrap_err();
        assert!(
            err.to_string().contains("plugin source root contains SKILL.md"),
            "expected root SKILL.md error, got: {err}"
        );
    }

    #[test]
    fn scan_source_dir_rejects_root_level_plugin() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        std::fs::write(
            dir.join("SYMPOSIUM.toml"),
            indoc! {r#"
                name = "root-plugin"
            "#},
        )
        .unwrap();

        let err = scan_source_dir(dir).unwrap_err();
        assert!(
            err.to_string().contains("plugin source root contains SYMPOSIUM.toml"),
            "expected root SYMPOSIUM.toml error, got: {err}"
        );
    }

    #[test]
    fn scan_source_dir_plugin_takes_precedence_over_skill() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        // Create a directory with both SYMPOSIUM.toml and SKILL.md
        let mixed_dir = dir.join("mixed");
        std::fs::create_dir_all(&mixed_dir).unwrap();

        // SYMPOSIUM.toml should take precedence
        std::fs::write(
            mixed_dir.join("SYMPOSIUM.toml"),
            indoc! {r#"
                name = "mixed-plugin"
            "#},
        )
        .unwrap();

        // This SKILL.md should be ignored due to SYMPOSIUM.toml precedence
        std::fs::write(
            mixed_dir.join("SKILL.md"),
            indoc! {"
                ---
                name: ignored-skill
                crates: serde
                ---

                This should be ignored.
            "},
        )
        .unwrap();

        let contents = scan_source_dir(dir).unwrap();
        assert_eq!(contents.plugins.len(), 1);
        assert_eq!(
            contents.plugins[0].as_ref().unwrap().plugin.name,
            "mixed-plugin"
        );
        assert!(contents.skill_files.is_empty(), "Skill should be ignored due to SYMPOSIUM.toml precedence");
    }

    #[test]
    fn scan_source_dir_symposium_toml_precedence() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        // Create a directory with both SYMPOSIUM.toml and other .toml files
        let plugin_dir = dir.join("precedence-test");
        std::fs::create_dir_all(&plugin_dir).unwrap();

        // SYMPOSIUM.toml should take precedence
        std::fs::write(
            plugin_dir.join("SYMPOSIUM.toml"),
            indoc! {r#"
                name = "preferred-plugin"
            "#},
        )
        .unwrap();

        // This other .toml should be ignored due to SYMPOSIUM.toml precedence
        std::fs::write(
            plugin_dir.join("other.toml"),
            indoc! {r#"
                name = "ignored-plugin"
            "#},
        )
        .unwrap();

        let contents = scan_source_dir(dir).unwrap();
        assert_eq!(contents.plugins.len(), 1);
        assert_eq!(
            contents.plugins[0].as_ref().unwrap().plugin.name,
            "preferred-plugin"
        );
        assert!(contents.skill_files.is_empty());
    }

    #[test]
    fn scan_source_dir_pruning_behavior() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        // Create the example structure:
        // foo/
        //     SYMPOSIUM.toml
        //     bar/
        //         SKILL.md
        // baz/
        //     SKILL.md
        //     qux/
        //         SYMPOSIUM.toml
        //         SKILL.md

        let foo_dir = dir.join("foo");
        std::fs::create_dir_all(&foo_dir).unwrap();
        std::fs::write(
            foo_dir.join("SYMPOSIUM.toml"),
            indoc! {r#"
                name = "foo-plugin"
            "#},
        )
        .unwrap();

        let foo_bar_dir = foo_dir.join("bar");
        std::fs::create_dir_all(&foo_bar_dir).unwrap();
        std::fs::write(
            foo_bar_dir.join("SKILL.md"),
            indoc! {"
                ---
                name: foo-bar-skill
                crates: serde
                ---

                Should be pruned.
            "},
        )
        .unwrap();

        let baz_dir = dir.join("baz");
        std::fs::create_dir_all(&baz_dir).unwrap();
        std::fs::write(
            baz_dir.join("SKILL.md"),
            indoc! {"
                ---
                name: baz-skill
                crates: tokio
                ---

                Should be found.
            "},
        )
        .unwrap();

        let baz_qux_dir = baz_dir.join("qux");
        std::fs::create_dir_all(&baz_qux_dir).unwrap();
        std::fs::write(
            baz_qux_dir.join("SYMPOSIUM.toml"),
            indoc! {r#"
                name = "qux-plugin"
            "#},
        )
        .unwrap();
        std::fs::write(
            baz_qux_dir.join("SKILL.md"),
            indoc! {"
                ---
                name: qux-skill
                crates: anyhow
                ---

                Should be pruned.
            "},
        )
        .unwrap();

        let contents = scan_source_dir(dir).unwrap();

        // Should find foo/SYMPOSIUM.toml as a plugin
        assert_eq!(contents.plugins.len(), 1);
        assert_eq!(
            contents.plugins[0].as_ref().unwrap().plugin.name,
            "foo-plugin"
        );

        // Should find only baz/SKILL.md (foo/bar/SKILL.md and baz/qux/* are pruned)
        assert_eq!(contents.skill_files.len(), 1);
        assert!(contents.skill_files[0].ends_with("baz/SKILL.md"));
    }

    #[test]
    fn validate_source_dir_mixed() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        // Valid TOML plugin in subdirectory
        let good_dir = dir.join("good-plugin");
        std::fs::create_dir_all(&good_dir).unwrap();
        std::fs::write(
            good_dir.join("SYMPOSIUM.toml"),
            indoc! {r#"
                name = "good-plugin"
            "#},
        )
        .unwrap();

        // Invalid TOML plugin in subdirectory
        let bad_dir = dir.join("bad-plugin");
        std::fs::create_dir_all(&bad_dir).unwrap();
        std::fs::write(bad_dir.join("SYMPOSIUM.toml"), "not valid toml {{{").unwrap();

        // Valid standalone skill
        let skill_dir = dir.join("my-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            indoc! {"
                ---
                name: my-skill
                description: A skill
                crates: serde
                ---

                Body.
            "},
        )
        .unwrap();

        // Invalid standalone skill (missing name)
        let bad_skill = dir.join("bad-skill");
        std::fs::create_dir_all(&bad_skill).unwrap();
        std::fs::write(
            bad_skill.join("SKILL.md"),
            indoc! {"
                ---
                description: No name
                crates: serde
                ---

                Body.
            "},
        )
        .unwrap();

        let results = validate_source_dir(dir).unwrap();
        let ok_count = results.iter().filter(|r| r.result.is_ok()).count();
        let err_count = results.iter().filter(|r| r.result.is_err()).count();
        assert_eq!(results.len(), 4);
        assert_eq!(ok_count, 2);
        assert_eq!(err_count, 2);
    }

    #[test]
    fn collect_crate_names_from_source_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        // TOML plugin with skill groups referencing crates (in subdirectory)
        let plugin_dir = dir.join("my-plugin");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(
            plugin_dir.join("SYMPOSIUM.toml"),
            indoc! {r#"
                name = "my-plugin"

                [[skills]]
                crates = ["serde", "serde_json>=1.0"]
            "#},
        )
        .unwrap();

        // Standalone skill referencing another crate
        let skill_dir = dir.join("my-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            indoc! {"
                ---
                name: my-skill
                description: A skill
                crates: anyhow
                ---

                Body.
            "},
        )
        .unwrap();

        let names = collect_crate_names_in_source_dir(dir).unwrap();
        // BTreeSet means sorted output
        assert_eq!(names, vec!["anyhow", "serde", "serde_json"]);
    }

    #[test]
    fn collect_crate_names_skips_invalid_items() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        // Invalid TOML in subdirectory (skipped)
        let bad_dir = dir.join("bad-plugin");
        std::fs::create_dir_all(&bad_dir).unwrap();
        std::fs::write(bad_dir.join("SYMPOSIUM.toml"), "not valid {{{").unwrap();

        // Valid standalone skill
        let skill_dir = dir.join("good-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            indoc! {"
                ---
                name: good
                description: Good skill
                crates: serde
                ---

                Body.
            "},
        )
        .unwrap();

        // Invalid standalone skill (missing crates, skipped)
        let bad_skill = dir.join("bad-skill");
        std::fs::create_dir_all(&bad_skill).unwrap();
        std::fs::write(
            bad_skill.join("SKILL.md"),
            indoc! {"
                ---
                name: bad
                ---

                Body.
            "},
        )
        .unwrap();

        let names = collect_crate_names_in_source_dir(dir).unwrap();
        // Only the valid skill's crate name
        assert_eq!(names, vec!["serde"]);
    }

    #[tokio::test]
    async fn check_crate_exists_on_crates_io() {
        assert!(check_crate_exists("serde").await);
        assert!(!check_crate_exists("this-crate-definitely-does-not-exist-zzz").await);
    }

    #[test]
    fn parse_manifest_with_multiple_skill_groups() {
        let toml = indoc! {r#"
            name = "multi-group"

            [[skills]]
            crates = ["serde"]

            [[skills]]
            crates = ["tokio"]
        "#};
        let plugin = from_str(toml).expect("parse");
        assert_eq!(plugin.name, "multi-group");
        assert_eq!(plugin.skills.len(), 2);
        assert!(plugin.skills[0].crates.as_ref().unwrap()[0].references_crate("serde"));
        assert!(plugin.skills[1].crates.as_ref().unwrap()[0].references_crate("tokio"));
    }

    #[test]
    fn parse_manifest_with_no_mcp_servers() {
        let plugin = from_str(SAMPLE).expect("parse");
        assert!(plugin.mcp_servers.is_empty());
    }

    #[test]
    fn mcp_entry_stdio() {
        let entry: McpServerEntry = toml::from_str(indoc! {r#"
            name = "my-server"
            command = "/usr/local/bin/my-server"
            args = ["--stdio"]
            env = []
        "#})
        .expect("parse");
        expect_test::expect![[r#"
            Stdio(
                McpServerStdio {
                    name: "my-server",
                    command: "/usr/local/bin/my-server",
                    args: [
                        "--stdio",
                    ],
                    env: [],
                    meta: None,
                },
            )"#]]
        .assert_eq(&format!("{entry:#?}"));
    }

    #[test]
    fn mcp_entry_http() {
        let entry: McpServerEntry = toml::from_str(indoc! {r#"
            type = "http"
            name = "my-server"
            url = "http://localhost:8080/mcp"
            headers = []
        "#})
        .expect("parse");
        expect_test::expect![[r#"
            Http(
                McpServerHttp {
                    name: "my-server",
                    url: "http://localhost:8080/mcp",
                    headers: [],
                    meta: None,
                },
            )"#]]
        .assert_eq(&format!("{entry:#?}"));
    }

    #[test]
    fn mcp_entry_sse() {
        let entry: McpServerEntry = toml::from_str(indoc! {r#"
            type = "sse"
            name = "my-server"
            url = "http://localhost:8080/sse"
            headers = []
        "#})
        .expect("parse");
        expect_test::expect![[r#"
            Sse(
                McpServerSse {
                    name: "my-server",
                    url: "http://localhost:8080/sse",
                    headers: [],
                    meta: None,
                },
            )"#]]
        .assert_eq(&format!("{entry:#?}"));
    }
}
