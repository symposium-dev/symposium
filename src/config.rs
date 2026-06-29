use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::Level;

// ---------------------------------------------------------------------------
// User configuration (~/.symposium/config.toml)
// ---------------------------------------------------------------------------

/// Auto-update behavior for the symposium binary.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum AutoUpdate {
    /// Never check for or install updates.
    Off,
    /// Print a warning when a newer version is available.
    Warn,
    /// Automatically install updates and re-exec into the new version.
    #[default]
    On,
}

impl AutoUpdate {
    fn is_default(&self) -> bool {
        matches!(self, AutoUpdate::On)
    }
}

impl std::fmt::Display for AutoUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AutoUpdate::Off => write!(f, "off"),
            AutoUpdate::Warn => write!(f, "warn"),
            AutoUpdate::On => write!(f, "on"),
        }
    }
}

/// Where agent hooks are installed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum HookScope {
    /// Install hooks in the user's home directory (e.g., `~/.claude/settings.json`).
    #[default]
    Global,
    /// Install hooks in the project directory (e.g., `<project>/.claude/settings.json`).
    Project,
}

impl HookScope {
    fn is_default(&self) -> bool {
        matches!(self, HookScope::Global)
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    /// Automatically run `sync` when hooks are invoked.
    #[serde(default = "default_true", rename = "auto-sync")]
    pub auto_sync: bool,

    /// Propagate user-authored skills from `.agents/skills/` to each
    /// configured agent's skill directory (e.g. `.claude/skills/`,
    /// `.kiro/skills/`). Skills that symposium itself installed into
    /// `.agents/skills/` are not propagated.
    #[serde(default = "default_true", rename = "agents-syncing")]
    pub agents_syncing: bool,

    /// How many seconds after a successful sync we skip re-checking a skill
    /// directory. Set to 0 to disable debouncing (useful in tests).
    #[serde(default = "default_sync_debounce_secs", rename = "sync-debounce-secs")]
    pub sync_debounce_secs: u64,

    /// Where to install agent hooks.
    #[serde(
        default,
        rename = "hook-scope",
        skip_serializing_if = "HookScope::is_default"
    )]
    pub hook_scope: HookScope,

    /// Auto-update behavior for the symposium binary.
    #[serde(
        default,
        rename = "auto-update",
        skip_serializing_if = "AutoUpdate::is_default"
    )]
    pub auto_update: AutoUpdate,

    /// Opt-in usage telemetry. Off by default.
    #[serde(default, skip_serializing_if = "TelemetryConfig::is_default")]
    pub telemetry: TelemetryConfig,

    /// Agents configured for this user.
    #[serde(default, rename = "agent")]
    pub agents: Vec<AgentEntry>,

    #[serde(default)]
    pub logging: LoggingConfig,

    /// Default plugin sources that are always included unless disabled.
    #[serde(default)]
    pub defaults: DefaultsConfig,

    /// User-defined plugin sources (git repos or local paths).
    #[serde(default, rename = "plugin-source")]
    pub plugin_source: Vec<PluginSourceConfig>,
}

/// An `[[agent]]` entry — just identifies an agent by name.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AgentEntry {
    /// Agent name (e.g., "claude", "copilot", "gemini").
    pub name: String,
}

/// Opt-in usage telemetry settings.
///
/// Telemetry is recorded as a local, append-only JSON-lines event log under
/// `<config-dir>/telemetry/` that the user can inspect and share manually.
/// Nothing is uploaded automatically.
#[derive(Debug, Default, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct TelemetryConfig {
    /// Record anonymous usage events (session starts, prompts, tool usage)
    /// to the local event log. Off by default.
    #[serde(default)]
    pub enabled: bool,
}

impl TelemetryConfig {
    fn is_default(&self) -> bool {
        *self == TelemetryConfig::default()
    }
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
            auto_sync: true,
            agents_syncing: true,
            sync_debounce_secs: default_sync_debounce_secs(),
            hook_scope: HookScope::default(),
            auto_update: AutoUpdate::default(),
            telemetry: TelemetryConfig::default(),
            agents: Vec::new(),
            logging: LoggingConfig::default(),
            defaults: DefaultsConfig::default(),
            plugin_source: Vec::new(),
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
#[serde(deny_unknown_fields)]
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
// Merged configuration view
// ---------------------------------------------------------------------------

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
    dirs: symposium_sdk::dirs::SymposiumDirs,
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
        let dirs = symposium_sdk::dirs::SymposiumDirs::from_environment();
        let _ = fs::create_dir_all(&dirs.config_dir);
        let _ = fs::create_dir_all(&dirs.cache_dir);

        let config = load_config_from(&dirs.config_dir);

        // Note: can't use tracing here — logging isn't initialized yet.
        // init_logging() is called after construction.

        Self {
            config,
            dirs,
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

        let cache_dir = config_dir.join("cache");
        let _ = fs::create_dir_all(&cache_dir);

        // In test mode, use the root as the home directory so that
        // global hook registration writes into the tempdir.
        let home_dir = root.to_path_buf();

        let dirs = symposium_sdk::dirs::SymposiumDirs::new(config_dir, cache_dir, None);

        Self {
            config,
            dirs,
            home_dir,
        }
    }

    /// The resolved directory paths.
    pub fn dirs(&self) -> &symposium_sdk::dirs::SymposiumDirs {
        &self.dirs
    }

    /// The cargo binary override, if set via `SYMPOSIUM_CARGO`.
    pub fn cargo_override(&self) -> Option<&Path> {
        self.dirs.cargo_override.as_deref()
    }

    /// Create a `WorkspaceDeps` with disk caching enabled.
    pub fn workspace_deps(&self, cwd: &Path) -> symposium_sdk::workspace::WorkspaceDeps {
        self.dirs.workspace_deps(cwd)
    }

    /// Build a `Command` for the cargo binary.
    ///
    /// Uses the test override if set, otherwise plain `"cargo"`.
    pub fn cargo_command(&self) -> std::process::Command {
        match &self.dirs.cargo_override {
            Some(path) => std::process::Command::new(path),
            None => std::process::Command::new("cargo"),
        }
    }

    /// Create an [`InstallContext`] for use with `symposium-install` functions.
    pub fn install_context(&self) -> symposium_install::InstallContext {
        let ctx = symposium_install::InstallContext::new(self.dirs.cache_dir.clone());
        match &self.dirs.cargo_override {
            Some(path) => ctx.with_cargo_bin(path.clone()),
            None => ctx,
        }
    }

    /// Override the cargo binary path (test-only).
    #[doc(hidden)]
    pub fn set_cargo_override(&mut self, path: PathBuf) {
        self.dirs.cargo_override = Some(path);
    }

    /// Initialize logging with an optional report layer. Call once at startup.
    ///
    /// When `report_layer` is `Some`, the layer is composed into the
    /// subscriber so it receives events alongside the file logger.
    /// Per-layer filtering ensures the report layer can receive debug
    /// events even when the file log level is set higher.
    pub fn init_logging(&self, report_layer: Option<crate::report::ReportLayer>) {
        use std::fs::OpenOptions;
        use tracing_subscriber::EnvFilter;
        use tracing_subscriber::Layer as _;
        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::util::SubscriberInitExt;

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
        let file_filter = EnvFilter::new(level.as_str());

        let file_layer = tracing_subscriber::fmt::layer()
            .with_writer(file)
            .with_ansi(false)
            .with_filter(file_filter);

        // The report layer does its own level filtering internally, so
        // give it a permissive filter that lets all events through.
        let report_filter = EnvFilter::new("trace");
        let report_layer = report_layer.map(|l| l.with_filter(report_filter));

        tracing_subscriber::registry()
            .with(file_layer)
            .with(report_layer)
            .init();

        tracing::debug!(
            config_dir = %self.dirs.config_dir.display(),
            cache_dir = %self.dirs.cache_dir.display(),
            log_level = %level,
            log_file = %log_path.display(),
            "logging initialized"
        );
        tracing::trace!(config = ?self.config, "loaded config");
    }

    pub fn config_dir(&self) -> &Path {
        &self.dirs.config_dir
    }

    pub fn cache_dir(&self) -> &Path {
        &self.dirs.cache_dir
    }

    pub fn home_dir(&self) -> &Path {
        &self.home_dir
    }

    /// Returns the effective list of plugin sources, including built-in defaults.
    pub fn plugin_sources(&self) -> Vec<ResolvedPluginSource> {
        let mut sources = Vec::new();

        if self.config.defaults.symposium_recommendations {
            sources.push(ResolvedPluginSource {
                source: PluginSourceConfig {
                    name: "symposium-recommendations".to_string(),
                    git: Some(BUILTIN_RECOMMENDATIONS_URL.to_string()),
                    path: None,
                    auto_update: true,
                },
                base_dir: self.dirs.config_dir.clone(),
            });
        }

        if self.config.defaults.user_plugins {
            sources.push(ResolvedPluginSource {
                source: PluginSourceConfig {
                    name: "user-plugins".to_string(),
                    git: None,
                    path: Some("plugins".to_string()),
                    auto_update: true,
                },
                base_dir: self.dirs.config_dir.clone(),
            });
        }

        for s in &self.config.plugin_source {
            sources.push(ResolvedPluginSource {
                source: s.clone(),
                base_dir: self.dirs.config_dir.clone(),
            });
        }

        sources
    }

    /// Write the user config to disk.
    pub fn save_config(&self) -> anyhow::Result<()> {
        let path = self.dirs.config_dir.join("config.toml");
        let contents = toml::to_string_pretty(&self.config)?;
        fs::write(&path, contents)?;
        Ok(())
    }

    #[cfg(test)]
    pub fn plugins_dir(&self) -> PathBuf {
        let dir = self.dirs.config_dir.join("plugins");
        let _ = fs::create_dir_all(&dir);
        dir
    }

    fn logs_dir(&self) -> PathBuf {
        let dir = resolve_logs_dir(&self.dirs.config_dir);
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

/// Resolve logs dir from environment.
///
/// Resolution: SYMPOSIUM_HOME/logs → XDG_STATE_HOME/symposium/logs → config_dir/logs.
fn resolve_logs_dir(config_dir: &Path) -> PathBuf {
    if let Ok(home) = env::var("SYMPOSIUM_HOME") {
        return PathBuf::from(home).join("logs");
    }
    if let Ok(xdg) = env::var("XDG_STATE_HOME") {
        return PathBuf::from(xdg).join("symposium").join("logs");
    }
    config_dir.join("logs")
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

fn default_true() -> bool {
    true
}

fn default_sync_debounce_secs() -> u64 {
    5
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
    fn parse_agents() {
        let config: Config = toml::from_str(indoc! {r#"
            auto-sync = true

            [[agent]]
            name = "claude"

            [[agent]]
            name = "copilot"
        "#})
        .unwrap();
        assert_eq!(config.agents.len(), 2);
        assert_eq!(config.agents[0].name, "claude");
        assert_eq!(config.agents[1].name, "copilot");
        assert!(config.auto_sync);
    }

    #[test]
    fn parse_config_defaults() {
        let config: Config = toml::from_str("").unwrap();
        assert!(config.agents.is_empty());
        assert!(config.auto_sync); // default true
        assert!(config.agents_syncing); // default true
    }

    #[test]
    fn parse_telemetry_defaults_off() {
        let config: Config = toml::from_str("").unwrap();
        assert!(!config.telemetry.enabled);
    }

    #[test]
    fn parse_telemetry_enabled() {
        let config: Config = toml::from_str(indoc! {"
            [telemetry]
            enabled = true
        "})
        .unwrap();
        assert!(config.telemetry.enabled);
    }

    #[test]
    fn telemetry_off_is_omitted_from_serialized_config() {
        let config = Config::default();
        let serialized = toml::to_string_pretty(&config).unwrap();
        assert!(
            !serialized.contains("[telemetry]"),
            "default (off) telemetry should not be written to config.toml: {serialized}"
        );
    }

    #[test]
    fn parse_agents_syncing_disabled() {
        let config: Config = toml::from_str(indoc! {"
            agents-syncing = false
        "})
        .unwrap();
        assert!(!config.agents_syncing);
    }

    #[test]
    fn resolve_logs_dir_uses_xdg_state_home() {
        let tmp = tempfile::tempdir().unwrap();
        let xdg_state = tmp.path().join("state");
        std::fs::create_dir_all(&xdg_state).unwrap();

        // Temporarily set XDG_STATE_HOME and unset SYMPOSIUM_HOME
        let old_state = env::var("XDG_STATE_HOME").ok();
        let old_home = env::var("SYMPOSIUM_HOME").ok();
        unsafe {
            env::set_var("XDG_STATE_HOME", &xdg_state);
            env::remove_var("SYMPOSIUM_HOME");
        }

        let result = resolve_logs_dir(tmp.path());

        // Restore
        unsafe {
            match old_state {
                Some(v) => env::set_var("XDG_STATE_HOME", v),
                None => env::remove_var("XDG_STATE_HOME"),
            }
            match old_home {
                Some(v) => env::set_var("SYMPOSIUM_HOME", v),
                None => env::remove_var("SYMPOSIUM_HOME"),
            }
        }

        assert_eq!(result, xdg_state.join("symposium").join("logs"));
    }
}
