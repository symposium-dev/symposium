use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::Level;

// ---------------------------------------------------------------------------
// User configuration (~/.symposium/config.toml)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    /// Agent preference and defaults.
    #[serde(default)]
    pub agent: AgentConfig,

    #[serde(default)]
    pub logging: LoggingConfig,

    /// Override the cache directory.
    pub cache_dir: Option<PathBuf>,

    /// Default plugin sources that are always included unless disabled.
    #[serde(default)]
    pub defaults: DefaultsConfig,

    /// User-defined plugin sources (git repos or local paths).
    #[serde(default, rename = "plugin-source")]
    pub plugin_source: Vec<PluginSourceConfig>,

    /// Hook behavior settings.
    #[serde(default)]
    pub hooks: HooksConfig,
}

/// Agent preference and default behaviors.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AgentConfig {
    /// Which agent the user uses (e.g., "claude", "copilot", "gemini").
    pub name: Option<String>,

    /// Default on/off for newly discovered extensions.
    #[serde(default = "default_true", rename = "sync-default")]
    pub sync_default: bool,

    /// Automatically run `sync --workspace` when Cargo.lock changes.
    #[serde(default, rename = "auto-sync")]
    pub auto_sync: bool,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            name: None,
            sync_default: true,
            auto_sync: false,
        }
    }
}

/// Configuration for hook behavior.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct HooksConfig {
    /// Number of prompts before re-nudging about an unloaded crate skill.
    /// Set to 0 to disable nudges entirely.
    #[serde(default = "default_nudge_interval", rename = "nudge-interval")]
    pub nudge_interval: i64,
}

impl Default for HooksConfig {
    fn default() -> Self {
        Self {
            nudge_interval: default_nudge_interval(),
        }
    }
}

fn default_nudge_interval() -> i64 {
    50
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LoggingConfig {
    #[serde(default = "default_level")]
    pub level: String,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_level(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            agent: AgentConfig::default(),
            logging: LoggingConfig::default(),
            cache_dir: None,
            defaults: DefaultsConfig::default(),
            plugin_source: Vec::new(),
            hooks: HooksConfig::default(),
        }
    }
}

/// Controls which built-in plugin sources are enabled.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DefaultsConfig {
    /// Include the `symposium-dev/recommendations` git source (default: true).
    #[serde(default = "default_true", rename = "symposium-recommendations")]
    pub symposium_recommendations: bool,

    /// Include `~/.symposium/plugins/` as a local source (default: true).
    #[serde(default = "default_true", rename = "user-plugins")]
    pub user_plugins: bool,
}

impl Default for DefaultsConfig {
    fn default() -> Self {
        Self {
            symposium_recommendations: true,
            user_plugins: true,
        }
    }
}

/// A configured plugin source — either a git repository or a local path.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PluginSourceConfig {
    /// Display name for this source.
    pub name: String,

    /// GitHub URL (fetched as tarball, cached locally).
    #[serde(default)]
    pub git: Option<String>,

    /// Local directory path (relative to config dir, or absolute).
    #[serde(default)]
    pub path: Option<String>,

    /// Whether to auto-update on startup (git sources only, default: true).
    #[serde(default = "default_true", rename = "auto-update")]
    pub auto_update: bool,
}

// ---------------------------------------------------------------------------
// Project configuration (.symposium/config.toml)
// ---------------------------------------------------------------------------

/// Project-level configuration stored at `.symposium/config.toml`.
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ProjectConfig {
    /// Optional project-level agent override.
    #[serde(default)]
    pub agent: Option<AgentConfig>,

    /// If true, ignore user-level plugin sources entirely.
    /// The project's own defaults and plugin sources are still active.
    #[serde(default, rename = "self-contained")]
    pub self_contained: bool,

    /// Controls which built-in plugin sources are enabled at the project level.
    /// When `self-contained = true`, these replace the user-level defaults.
    /// When `self-contained = false`, these are merged with the user-level defaults
    /// (project `false` overrides user `true`).
    #[serde(default)]
    pub defaults: Option<DefaultsConfig>,

    /// Project-level plugin sources (git repos or local paths).
    /// Paths are relative to the project root.
    #[serde(default, rename = "plugin-source")]
    pub plugin_source: Vec<PluginSourceConfig>,

    /// Crate skills discovered from workspace dependencies. Key = crate name, value = enabled.
    #[serde(default)]
    pub skills: BTreeMap<String, bool>,

    /// Workflow extensions. Key = workflow name, value = enabled.
    #[serde(default)]
    pub workflows: BTreeMap<String, bool>,
}

impl ProjectConfig {
    /// Path to the project config file.
    pub fn path(project_root: &Path) -> PathBuf {
        project_root.join(".symposium").join("config.toml")
    }

    /// Load from a `.symposium/config.toml` file, or return None if not found.
    pub fn load(project_root: &Path) -> Option<Self> {
        let path = Self::path(project_root);
        let contents = fs::read_to_string(&path).ok()?;
        match toml::from_str(&contents) {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!(
                    "warning: failed to parse {}: {e}",
                    path.display()
                );
                None
            }
        }
    }

    /// Write this config from scratch to `.symposium/config.toml`.
    ///
    /// Use this only for initial creation. For updates, prefer the
    /// format-preserving `update_*` functions below.
    pub fn save(&self, project_root: &Path) -> anyhow::Result<()> {
        let dir = project_root.join(".symposium");
        fs::create_dir_all(&dir)?;
        let path = dir.join("config.toml");
        let contents = toml::to_string_pretty(self)?;
        fs::write(&path, contents)?;
        Ok(())
    }

    /// Format-preserving update of the `[skills]` table.
    ///
    /// Adds new entries, removes stale ones, and preserves existing
    /// values and any user comments.
    pub fn update_skills(
        project_root: &Path,
        new_skills: &BTreeMap<String, bool>,
    ) -> anyhow::Result<()> {
        let path = Self::path(project_root);
        let contents = fs::read_to_string(&path).unwrap_or_default();
        let mut doc: toml_edit::DocumentMut = contents
            .parse()
            .unwrap_or_else(|_| toml_edit::DocumentMut::new());

        // Ensure [skills] table exists
        if !doc.contains_key("skills") {
            doc["skills"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let table = doc["skills"]
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("'skills' is not a table"))?;

        // Remove entries not in new_skills
        let existing_keys: Vec<String> = table.iter().map(|(k, _)| k.to_string()).collect();
        for key in &existing_keys {
            if !new_skills.contains_key(key) {
                table.remove(key);
            }
        }

        // Add new entries (preserve existing values)
        for (name, default) in new_skills {
            if !table.contains_key(name) {
                table[name] = toml_edit::value(*default);
            }
        }

        let dir = project_root.join(".symposium");
        fs::create_dir_all(&dir)?;
        fs::write(&path, doc.to_string())?;
        Ok(())
    }

    /// Format-preserving update of `[agent] name`.
    pub fn set_agent_name(project_root: &Path, agent_name: &str) -> anyhow::Result<()> {
        let path = Self::path(project_root);
        let contents = fs::read_to_string(&path).unwrap_or_default();
        let mut doc: toml_edit::DocumentMut = contents
            .parse()
            .unwrap_or_else(|_| toml_edit::DocumentMut::new());

        if !doc.contains_key("agent") {
            doc["agent"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        doc["agent"]["name"] = toml_edit::value(agent_name);

        let dir = project_root.join(".symposium");
        fs::create_dir_all(&dir)?;
        fs::write(&path, doc.to_string())?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Merged configuration view
// ---------------------------------------------------------------------------

/// Resolved agent name from merged user + project configs.
pub fn resolve_agent_name(user: &Config, project: Option<&ProjectConfig>) -> Option<String> {
    if let Some(proj) = project {
        if let Some(ref agent) = proj.agent {
            if agent.name.is_some() {
                return agent.name.clone();
            }
        }
    }
    user.agent.name.clone()
}

/// Resolved sync-default from merged user + project configs.
pub fn resolve_sync_default(user: &Config, project: Option<&ProjectConfig>) -> bool {
    if let Some(proj) = project {
        if let Some(ref agent) = proj.agent {
            return agent.sync_default;
        }
    }
    user.agent.sync_default
}

/// A plugin source together with its base directory for resolving relative paths.
#[derive(Debug, Clone)]
pub struct ResolvedPluginSource {
    pub source: PluginSourceConfig,
    /// Directory to resolve relative `path` values against.
    /// For user sources this is the user config dir; for project sources
    /// this is the project root.
    pub base_dir: PathBuf,
}

const BUILTIN_RECOMMENDATIONS_URL: &str = "https://github.com/symposium-dev/recommendations";

/// Full application context: parsed config + resolved directory paths.
///
/// Thread `&Symposium` through all call sites instead of using global state.
#[derive(Clone)]
pub struct Symposium {
    pub config: Config,
    config_dir: PathBuf,
    cache_dir: PathBuf,
    home_dir: PathBuf,
}

impl Symposium {
    /// Production constructor: resolves paths from environment.
    ///
    /// Resolution order for config dir:
    /// 1. `SYMPOSIUM_HOME` env var
    /// 2. `XDG_CONFIG_HOME/symposium`
    /// 3. `~/.symposium`
    pub fn from_environment() -> Self {
        let home_dir = dirs::home_dir().expect("could not determine home directory");
        let config_dir = resolve_config_dir_from_env();
        let _ = fs::create_dir_all(&config_dir);

        let config = load_config_from(&config_dir);

        let cache_dir = resolve_cache_dir(&config, &config_dir);
        let _ = fs::create_dir_all(&cache_dir);

        Self {
            config,
            config_dir,
            cache_dir,
            home_dir,
        }
    }

    /// Test constructor: everything rooted under a single directory.
    ///
    /// Creates `config.toml` from the provided config if not already present.
    pub fn from_dir(root: &Path) -> Self {
        let config_dir = root.to_path_buf();
        let _ = fs::create_dir_all(&config_dir);

        let config = load_config_from(&config_dir);

        let cache_dir = if let Some(ref dir) = config.cache_dir {
            dir.clone()
        } else {
            config_dir.join("cache")
        };
        let _ = fs::create_dir_all(&cache_dir);

        // In test mode, use the root as the home directory so that
        // global hook registration writes into the tempdir.
        let home_dir = root.to_path_buf();

        Self {
            config,
            config_dir,
            cache_dir,
            home_dir,
        }
    }

    /// Initialize logging. Call once at startup.
    pub fn init_logging(&self) {
        use std::fs::OpenOptions;
        use tracing_subscriber::EnvFilter;
        use tracing_subscriber::fmt;

        let logs = self.logs_dir();
        let now = chrono::Local::now();
        let filename = now.format("symposium-%Y%m%d-%H%M%S.log").to_string();
        let log_path = logs.join(&filename);

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .expect("failed to open log file");

        let level = self.log_level();
        let filter = EnvFilter::new(level.as_str());

        fmt()
            .with_env_filter(filter)
            .with_writer(file)
            .with_ansi(false)
            .init();
    }

    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    pub fn home_dir(&self) -> &Path {
        &self.home_dir
    }

    /// Returns the effective list of plugin sources, including built-in defaults.
    ///
    /// When a project config is provided, its sources are unioned with user sources.
    /// If the project is `self-contained`, user sources are excluded.
    /// `project_root` is used to resolve relative paths in project sources.
    pub fn plugin_sources(
        &self,
        project: Option<&ProjectConfig>,
        project_root: Option<&Path>,
    ) -> Vec<ResolvedPluginSource> {
        let self_contained = project.is_some_and(|p| p.self_contained);

        let mut sources = Vec::new();

        // Resolve which defaults are active
        let effective_recommendations;
        let effective_user_plugins;

        if self_contained {
            // Self-contained: only project defaults matter
            let proj_defaults = project.and_then(|p| p.defaults.as_ref());
            effective_recommendations = proj_defaults
                .map_or(true, |d| d.symposium_recommendations);
            effective_user_plugins = proj_defaults
                .map_or(true, |d| d.user_plugins);
        } else {
            // Merge: project can override user defaults (false wins)
            let user_rec = self.config.defaults.symposium_recommendations;
            let user_up = self.config.defaults.user_plugins;
            let proj_defaults = project.and_then(|p| p.defaults.as_ref());
            effective_recommendations = user_rec
                && proj_defaults.map_or(true, |d| d.symposium_recommendations);
            effective_user_plugins = user_up
                && proj_defaults.map_or(true, |d| d.user_plugins);
        }

        // Built-in defaults
        if effective_recommendations {
            sources.push(ResolvedPluginSource {
                source: PluginSourceConfig {
                    name: "symposium-recommendations".to_string(),
                    git: Some(BUILTIN_RECOMMENDATIONS_URL.to_string()),
                    path: None,
                    auto_update: true,
                },
                base_dir: self.config_dir.clone(),
            });
        }

        if effective_user_plugins {
            sources.push(ResolvedPluginSource {
                source: PluginSourceConfig {
                    name: "user-plugins".to_string(),
                    git: None,
                    path: Some("plugins".to_string()),
                    auto_update: true,
                },
                base_dir: self.config_dir.clone(),
            });
        }

        // User-level sources (unless self-contained)
        if !self_contained {
            for s in &self.config.plugin_source {
                sources.push(ResolvedPluginSource {
                    source: s.clone(),
                    base_dir: self.config_dir.clone(),
                });
            }
        }

        // Project-level sources
        if let (Some(proj), Some(root)) = (project, project_root) {
            for s in &proj.plugin_source {
                sources.push(ResolvedPluginSource {
                    source: s.clone(),
                    base_dir: root.to_path_buf(),
                });
            }
        }

        sources
    }

    /// Write the user config to disk.
    pub fn save_config(&self) -> anyhow::Result<()> {
        let path = self.config_dir.join("config.toml");
        let contents = toml::to_string_pretty(&self.config)?;
        fs::write(&path, contents)?;
        Ok(())
    }

    #[cfg(test)]
    pub fn plugins_dir(&self) -> PathBuf {
        let dir = self.config_dir.join("plugins");
        let _ = fs::create_dir_all(&dir);
        dir
    }

    fn logs_dir(&self) -> PathBuf {
        let dir = self.config_dir.join("logs");
        let _ = fs::create_dir_all(&dir);
        dir
    }

    fn log_level(&self) -> Level {
        match self.config.logging.level.to_lowercase().as_str() {
            "trace" => Level::TRACE,
            "debug" => Level::DEBUG,
            "info" => Level::INFO,
            "warn" => Level::WARN,
            "error" => Level::ERROR,
            other => {
                eprintln!("warning: unknown log level '{other}', defaulting to info");
                Level::INFO
            }
        }
    }
}

/// Resolve config dir from environment variables.
fn resolve_config_dir_from_env() -> PathBuf {
    if let Ok(home) = env::var("SYMPOSIUM_HOME") {
        PathBuf::from(home)
    } else if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg).join("symposium")
    } else {
        default_home()
    }
}

/// Resolve cache dir from config and environment.
fn resolve_cache_dir(config: &Config, config_dir: &Path) -> PathBuf {
    if let Some(ref dir) = config.cache_dir {
        return dir.clone();
    }
    if let Ok(home) = env::var("SYMPOSIUM_HOME") {
        return PathBuf::from(home).join("cache");
    }
    if let Ok(xdg) = env::var("XDG_CACHE_HOME") {
        return PathBuf::from(xdg).join("symposium");
    }
    config_dir.join("cache")
}

/// Load config from a config directory.
fn load_config_from(config_dir: &Path) -> Config {
    let path = config_dir.join("config.toml");
    match fs::read_to_string(&path) {
        Ok(contents) => toml::from_str(&contents).unwrap_or_else(|e| {
            eprintln!("warning: failed to parse {}: {e}", path.display());
            Config::default()
        }),
        Err(_) => Config::default(),
    }
}

/// Returns the default symposium home directory (~/.symposium).
fn default_home() -> PathBuf {
    dirs::home_dir()
        .expect("could not determine home directory")
        .join(".symposium")
}

fn default_true() -> bool {
    true
}

fn default_level() -> String {
    "info".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;

    #[test]
    fn parse_empty_config() {
        let config: Config = toml::from_str("").unwrap();
        assert!(config.defaults.symposium_recommendations);
        assert!(config.defaults.user_plugins);
        assert!(config.plugin_source.is_empty());
    }

    #[test]
    fn parse_defaults_disable_recommendations() {
        let config: Config = toml::from_str(indoc! {"
            [defaults]
            symposium-recommendations = false
        "})
        .unwrap();
        assert!(!config.defaults.symposium_recommendations);
        assert!(config.defaults.user_plugins);
    }

    #[test]
    fn parse_defaults_disable_user_plugins() {
        let config: Config = toml::from_str(indoc! {"
            [defaults]
            user-plugins = false
        "})
        .unwrap();
        assert!(config.defaults.symposium_recommendations);
        assert!(!config.defaults.user_plugins);
    }

    #[test]
    fn parse_plugin_source_git() {
        let config: Config = toml::from_str(indoc! {r#"
            [[plugin-source]]
            name = "my-org"
            git = "https://github.com/my-org/plugins"
            auto-update = false
        "#})
        .unwrap();
        assert_eq!(config.plugin_source.len(), 1);
        assert_eq!(config.plugin_source[0].name, "my-org");
        assert_eq!(
            config.plugin_source[0].git.as_deref(),
            Some("https://github.com/my-org/plugins")
        );
        assert!(!config.plugin_source[0].auto_update);
    }

    #[test]
    fn parse_plugin_source_path() {
        let config: Config = toml::from_str(indoc! {r#"
            [[plugin-source]]
            name = "local"
            path = "my-plugins"
        "#})
        .unwrap();
        assert_eq!(config.plugin_source.len(), 1);
        assert_eq!(config.plugin_source[0].path.as_deref(), Some("my-plugins"));
        assert!(config.plugin_source[0].auto_update); // default true
    }

    #[test]
    fn parse_multiple_plugin_sources() {
        let config: Config = toml::from_str(indoc! {r#"
            [defaults]
            symposium-recommendations = false

            [[plugin-source]]
            name = "org-a"
            git = "https://github.com/a/plugins"

            [[plugin-source]]
            name = "org-b"
            git = "https://github.com/b/plugins"
            auto-update = false

            [[plugin-source]]
            name = "local"
            path = "extras"
        "#})
        .unwrap();
        assert!(!config.defaults.symposium_recommendations);
        assert_eq!(config.plugin_source.len(), 3);
        assert_eq!(config.plugin_source[0].name, "org-a");
        assert_eq!(config.plugin_source[1].name, "org-b");
        assert_eq!(config.plugin_source[2].name, "local");
    }

    #[test]
    fn from_dir_creates_default_config() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(tmp.path());
        assert!(sym.config.defaults.symposium_recommendations);
        assert_eq!(sym.config_dir(), tmp.path());
        assert_eq!(sym.cache_dir(), tmp.path().join("cache"));
    }

    #[test]
    fn from_dir_reads_config_file() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("config.toml"),
            indoc! {"
                [defaults]
                symposium-recommendations = false
            "},
        )
        .unwrap();
        let sym = Symposium::from_dir(tmp.path());
        assert!(!sym.config.defaults.symposium_recommendations);
    }

    #[test]
    fn parse_agent_config() {
        let config: Config = toml::from_str(indoc! {r#"
            [agent]
            name = "claude"
            sync-default = false
            auto-sync = true
        "#})
        .unwrap();
        assert_eq!(config.agent.name.as_deref(), Some("claude"));
        assert!(!config.agent.sync_default);
        assert!(config.agent.auto_sync);
    }

    #[test]
    fn parse_agent_config_defaults() {
        let config: Config = toml::from_str("").unwrap();
        assert!(config.agent.name.is_none());
        assert!(config.agent.sync_default); // default true
        assert!(!config.agent.auto_sync); // default false
    }

    #[test]
    fn parse_project_config() {
        let config: ProjectConfig = toml::from_str(indoc! {r#"
            [agent]
            name = "claude"

            [skills]
            tokio = true
            serde = false

            [workflows]
            rtk = true
        "#})
        .unwrap();
        assert_eq!(
            config.agent.as_ref().and_then(|a| a.name.as_deref()),
            Some("claude")
        );
        assert_eq!(config.skills.get("tokio"), Some(&true));
        assert_eq!(config.skills.get("serde"), Some(&false));
        assert_eq!(config.workflows.get("rtk"), Some(&true));
    }

    #[test]
    fn project_config_save_load_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let config = ProjectConfig {
            agent: Some(AgentConfig {
                name: Some("claude".to_string()),
                sync_default: true,
                auto_sync: false,
            }),
            skills: [("tokio".to_string(), true), ("serde".to_string(), false)]
                .into_iter()
                .collect(),
            workflows: BTreeMap::new(),
            ..Default::default()
        };
        config.save(tmp.path()).unwrap();
        let loaded = ProjectConfig::load(tmp.path()).unwrap();
        assert_eq!(
            loaded.agent.as_ref().and_then(|a| a.name.as_deref()),
            Some("claude")
        );
        assert_eq!(loaded.skills.get("tokio"), Some(&true));
        assert_eq!(loaded.skills.get("serde"), Some(&false));
    }

    #[test]
    fn resolve_agent_name_project_overrides_user() {
        let user = Config {
            agent: AgentConfig {
                name: Some("gemini".to_string()),
                ..Default::default()
            },
            ..Config::default()
        };
        let project = ProjectConfig {
            agent: Some(AgentConfig {
                name: Some("claude".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            resolve_agent_name(&user, Some(&project)),
            Some("claude".to_string())
        );
    }

    #[test]
    fn resolve_agent_name_falls_back_to_user() {
        let user = Config {
            agent: AgentConfig {
                name: Some("gemini".to_string()),
                ..Default::default()
            },
            ..Config::default()
        };
        assert_eq!(
            resolve_agent_name(&user, None),
            Some("gemini".to_string())
        );
    }

    #[test]
    fn update_skills_preserves_comments() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".symposium");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            indoc! {r#"
                # Project config for my-app
                [skills]
                # We use tokio for async
                tokio = true
                serde = false
            "#},
        )
        .unwrap();

        // Add a new skill, remove nothing
        let mut skills = BTreeMap::new();
        skills.insert("tokio".to_string(), true);
        skills.insert("serde".to_string(), false);
        skills.insert("anyhow".to_string(), true);
        ProjectConfig::update_skills(tmp.path(), &skills).unwrap();

        let result = std::fs::read_to_string(dir.join("config.toml")).unwrap();
        // Comments should be preserved
        assert!(result.contains("# Project config for my-app"));
        assert!(result.contains("# We use tokio for async"));
        // Existing values preserved
        assert!(result.contains("tokio = true"));
        assert!(result.contains("serde = false"));
        // New skill added
        assert!(result.contains("anyhow = true"));
    }

    #[test]
    fn update_skills_removes_stale() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".symposium");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            indoc! {r#"
                [skills]
                tokio = true
                old-crate = true
            "#},
        )
        .unwrap();

        let mut skills = BTreeMap::new();
        skills.insert("tokio".to_string(), true);
        ProjectConfig::update_skills(tmp.path(), &skills).unwrap();

        let result = std::fs::read_to_string(dir.join("config.toml")).unwrap();
        assert!(result.contains("tokio = true"));
        assert!(!result.contains("old-crate"));
    }

    #[test]
    fn set_agent_name_preserves_existing_content() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".symposium");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            indoc! {r#"
                # My project
                [skills]
                tokio = true
            "#},
        )
        .unwrap();

        ProjectConfig::set_agent_name(tmp.path(), "claude").unwrap();

        let result = std::fs::read_to_string(dir.join("config.toml")).unwrap();
        assert!(result.contains("# My project"));
        assert!(result.contains("tokio = true"));
        assert!(result.contains(r#"name = "claude""#));
    }
}
