use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::Level;

#[derive(Debug, Deserialize, Clone)]
pub struct Settings {
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

/// Configuration for hook behavior.
#[derive(Debug, Deserialize, Clone)]
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

#[derive(Debug, Deserialize, Clone)]
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

impl Default for Settings {
    fn default() -> Self {
        Self {
            logging: LoggingConfig::default(),
            cache_dir: None,
            defaults: DefaultsConfig::default(),
            plugin_source: Vec::new(),
            hooks: HooksConfig::default(),
        }
    }
}

/// Controls which built-in plugin sources are enabled.
#[derive(Debug, Deserialize, Clone)]
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
#[derive(Debug, Deserialize, Clone)]
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

const BUILTIN_RECOMMENDATIONS_URL: &str = "https://github.com/symposium-dev/recommendations";

/// Application configuration: parsed settings + resolved directory paths.
///
/// Thread `&Config` through all call sites instead of using global state.
/// For the full application context including DB handle, see `Symposium`.
pub struct Config {
    pub settings: Settings,
    config_dir: PathBuf,
    cache_dir: PathBuf,
}

impl Config {
    /// Production constructor: resolves paths from environment.
    ///
    /// Resolution order for config dir:
    /// 1. `SYMPOSIUM_HOME` env var
    /// 2. `XDG_CONFIG_HOME/symposium`
    /// 3. `~/.symposium`
    pub fn from_environment() -> Self {
        let config_dir = resolve_config_dir_from_env();
        let _ = fs::create_dir_all(&config_dir);

        let settings = load_config_from(&config_dir);

        let cache_dir = resolve_cache_dir(&settings, &config_dir);
        let _ = fs::create_dir_all(&cache_dir);

        Self {
            settings,
            config_dir,
            cache_dir,
        }
    }

    /// Test constructor: everything rooted under a single directory.
    ///
    /// Creates `config.toml` from the provided config if not already present.
    pub fn from_dir(root: &Path) -> Self {
        let config_dir = root.to_path_buf();
        let _ = fs::create_dir_all(&config_dir);

        let settings = load_config_from(&config_dir);

        let cache_dir = if let Some(ref dir) = settings.cache_dir {
            dir.clone()
        } else {
            config_dir.join("cache")
        };
        let _ = fs::create_dir_all(&cache_dir);

        Self {
            settings,
            config_dir,
            cache_dir,
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

    /// Returns the effective list of plugin sources, including built-in defaults.
    pub fn plugin_sources(&self) -> Vec<PluginSourceConfig> {
        let c = &self.settings;
        let mut sources = Vec::new();

        if c.defaults.symposium_recommendations {
            sources.push(PluginSourceConfig {
                name: "symposium-recommendations".to_string(),
                git: Some(BUILTIN_RECOMMENDATIONS_URL.to_string()),
                path: None,
                auto_update: true,
            });
        }

        if c.defaults.user_plugins {
            sources.push(PluginSourceConfig {
                name: "user-plugins".to_string(),
                git: None,
                path: Some("plugins".to_string()),
                auto_update: true,
            });
        }

        sources.extend(c.plugin_source.clone());
        sources
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
        match self.settings.logging.level.to_lowercase().as_str() {
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
fn resolve_cache_dir(config: &Settings, config_dir: &Path) -> PathBuf {
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
fn load_config_from(config_dir: &Path) -> Settings {
    let path = config_dir.join("config.toml");
    match fs::read_to_string(&path) {
        Ok(contents) => toml::from_str(&contents).unwrap_or_else(|e| {
            eprintln!("warning: failed to parse {}: {e}", path.display());
            Settings::default()
        }),
        Err(_) => Settings::default(),
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

/// Full application context: `Config` + convenience accessors.
///
/// Wraps `Config` with `Deref` so all config methods are available directly.
pub struct Symposium {
    config: Config,
}

impl std::ops::Deref for Symposium {
    type Target = Config;
    fn deref(&self) -> &Config {
        &self.config
    }
}

impl Symposium {
    /// Create from a `Config`.
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Production constructor: resolves paths from environment.
    pub fn from_environment() -> Self {
        Self::new(Config::from_environment())
    }

    /// Test constructor: everything rooted under a single directory.
    pub fn from_dir(root: &std::path::Path) -> Self {
        Self::new(Config::from_dir(root))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;

    #[test]
    fn parse_empty_config() {
        let config: Settings = toml::from_str("").unwrap();
        assert!(config.defaults.symposium_recommendations);
        assert!(config.defaults.user_plugins);
        assert!(config.plugin_source.is_empty());
    }

    #[test]
    fn parse_defaults_disable_recommendations() {
        let config: Settings = toml::from_str(indoc! {"
            [defaults]
            symposium-recommendations = false
        "})
        .unwrap();
        assert!(!config.defaults.symposium_recommendations);
        assert!(config.defaults.user_plugins);
    }

    #[test]
    fn parse_defaults_disable_user_plugins() {
        let config: Settings = toml::from_str(indoc! {"
            [defaults]
            user-plugins = false
        "})
        .unwrap();
        assert!(config.defaults.symposium_recommendations);
        assert!(!config.defaults.user_plugins);
    }

    #[test]
    fn parse_plugin_source_git() {
        let config: Settings = toml::from_str(indoc! {r#"
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
        let config: Settings = toml::from_str(indoc! {r#"
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
        let config: Settings = toml::from_str(indoc! {r#"
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
        let cfg = Config::from_dir(tmp.path());
        assert!(cfg.settings.defaults.symposium_recommendations);
        assert_eq!(cfg.config_dir(), tmp.path());
        assert_eq!(cfg.cache_dir(), tmp.path().join("cache"));
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
        let cfg = Config::from_dir(tmp.path());
        assert!(!cfg.settings.defaults.symposium_recommendations);
    }
}
