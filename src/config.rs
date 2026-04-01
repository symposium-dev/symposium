use serde::Deserialize;
use std::cell::RefCell;
use std::env;
use std::fs;
use std::path::PathBuf;
use tracing::Level;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
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

impl Default for Config {
    fn default() -> Self {
        Self {
            logging: LoggingConfig::default(),
            cache_dir: None,
            defaults: DefaultsConfig::default(),
            plugin_source: Vec::new(),
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

/// Initialize logging and config. Call once at startup.
pub fn init() {
    use std::fs::OpenOptions;
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::fmt;

    let logs = logs_dir();
    let now = chrono::Local::now();
    let filename = now.format("symposium-%Y%m%d-%H%M%S.log").to_string();
    let log_path = logs.join(&filename);

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .expect("failed to open log file");

    let level = log_level();
    let filter = EnvFilter::new(level.as_str());

    fmt()
        .with_env_filter(filter)
        .with_writer(file)
        .with_ansi(false)
        .init();
}

/// Returns the config directory, creating it if needed.
///
/// Resolution order:
/// 1. `SYMPOSIUM_HOME` env var
/// 2. `XDG_CONFIG_HOME/symposium`
/// 3. `~/.symposium`
pub fn config_dir() -> PathBuf {
    let dir = if let Ok(home) = env::var("SYMPOSIUM_HOME") {
        PathBuf::from(home)
    } else if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg).join("symposium")
    } else {
        default_home()
    };
    let _ = fs::create_dir_all(&dir);
    dir
}

/// Returns the cache directory, creating it if needed.
///
/// Resolution order:
/// 1. `cache_dir` in config.toml (if set)
/// 2. `SYMPOSIUM_HOME/cache`
/// 3. `XDG_CACHE_HOME/symposium`
/// 4. `~/.symposium/cache`
pub fn cache_dir() -> PathBuf {
    let dir = with_config(|c| {
        if let Some(ref dir) = c.cache_dir {
            return dir.clone();
        }
        if let Ok(home) = env::var("SYMPOSIUM_HOME") {
            return PathBuf::from(home).join("cache");
        }
        if let Ok(xdg) = env::var("XDG_CACHE_HOME") {
            return PathBuf::from(xdg).join("symposium");
        }
        default_home().join("cache")
    });
    let _ = fs::create_dir_all(&dir);
    dir
}

/// Returns the effective list of plugin sources, including built-in defaults.
pub fn plugin_sources() -> Vec<PluginSourceConfig> {
    with_config(|c| {
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
    })
}

pub fn plugins_dir() -> PathBuf {
    let dir = config_dir().join("plugins");
    let _ = fs::create_dir_all(&dir);
    dir
}

fn with_config<T>(f: impl FnOnce(&Config) -> T) -> T {
    CONFIG.with(|cell| {
        let mut opt = cell.borrow_mut();
        if opt.is_none() {
            *opt = Some(load_config());
        }
        f(opt.as_ref().unwrap())
    })
}

fn load_config() -> Config {
    let path = config_path();
    match fs::read_to_string(&path) {
        Ok(contents) => toml::from_str(&contents).unwrap_or_else(|e| {
            eprintln!("warning: failed to parse {}: {e}", path.display());
            Config::default()
        }),
        Err(_) => Config::default(),
    }
}

/// Returns the path to the config file.
fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

/// Returns the logs directory, creating it if needed.
///
/// Resolution order:
/// 1. `SYMPOSIUM_HOME/logs`
/// 2. `XDG_DATA_HOME/symposium/logs`
/// 3. `~/.symposium/logs`
fn logs_dir() -> PathBuf {
    let dir = if let Ok(home) = env::var("SYMPOSIUM_HOME") {
        PathBuf::from(home).join("logs")
    } else if let Ok(xdg) = env::var("XDG_DATA_HOME") {
        PathBuf::from(xdg).join("symposium").join("logs")
    } else {
        default_home().join("logs")
    };
    let _ = fs::create_dir_all(&dir);
    dir
}

/// Returns the configured log level.
fn log_level() -> Level {
    with_config(|c| match c.logging.level.to_lowercase().as_str() {
        "trace" => Level::TRACE,
        "debug" => Level::DEBUG,
        "info" => Level::INFO,
        "warn" => Level::WARN,
        "error" => Level::ERROR,
        other => {
            eprintln!("warning: unknown log level '{other}', defaulting to info");
            Level::INFO
        }
    })
}

/// Returns the default symposium home directory (~/.symposium).
fn default_home() -> PathBuf {
    dirs::home_dir()
        .expect("could not determine home directory")
        .join(".symposium")
}

const BUILTIN_RECOMMENDATIONS_URL: &str = "https://github.com/symposium-dev/recommendations";

thread_local! {
    static CONFIG: RefCell<Option<Config>> = const { RefCell::new(None) };
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
}
