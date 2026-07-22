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

#[derive(Debug, Serialize, Clone)]
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

    /// Which discovered plugins the user has consented to.
    #[serde(default, skip_serializing_if = "PluginsConfig::is_default")]
    pub plugins: PluginsConfig,

    /// Agents configured for this user.
    #[serde(default, rename = "agent")]
    pub agents: Vec<AgentEntry>,

    #[serde(default)]
    pub logging: LoggingConfig,

    /// Default registries that are always included unless disabled.
    #[serde(default)]
    pub defaults: DefaultsConfig,

    /// User-defined registries (git repos or local paths).
    /// `plugin-source` is the retired spelling, still accepted.
    #[serde(default, rename = "registry", alias = "plugin-source")]
    pub registries: Vec<RegistryConfig>,
}

/// The `[plugins]` section: enablement, the consent axis.
///
/// Activation predicates answer *when* a plugin applies; enablement answers
/// *whether it may run at all*. The workspace and the configured registries
/// are trust roots — what they define needs no per-plugin consent. A
/// dependency is deliberately not a trust root: depending on a crate means
/// compiling its code, not letting its author inject agent context. So a
/// plugin embedded in a dependency runs only once the user consents, either
/// ahead of time (`auto-enable`) or by name (`use`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginsConfig {
    /// Dependency names whose embedded plugins load without being asked
    /// about; `"*"` pre-consents to every dependency. Matched
    /// hyphen/underscore-insensitively, like crate names.
    #[serde(default, rename = "auto-enable", skip_serializing_if = "Vec::is_empty")]
    pub auto_enable: Vec<String>,

    /// Plugins enabled deliberately, the durable record `cargo agents use`
    /// writes. Unlike `auto-enable` (consent for what a dependency already
    /// carries), a used plugin is enabled whether or not any dependency
    /// references it — it is also what wakes a dormant registry plugin.
    #[serde(default, rename = "use", skip_serializing_if = "Vec::is_empty")]
    pub used: Vec<UseEntry>,

    /// Plugin names pruned from enablement, and the record of declined
    /// discoveries (so they are not offered again).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disable: Vec<String>,
}

impl PluginsConfig {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Names enabled by `use` entries that apply while working in
    /// `workspace_root`.
    pub fn used_names_in(&self, workspace_root: &Path) -> Vec<&str> {
        self.used
            .iter()
            .filter(|entry| entry.applies_in(workspace_root))
            .map(UseEntry::name)
            .collect()
    }

    /// Does `name` appear in `auto-enable` (directly or via `"*"`)?
    pub fn is_auto_enabled(&self, name: &str) -> bool {
        self.auto_enable
            .iter()
            .any(|entry| name_matches(entry, name))
    }

    /// Does `name` appear in `disable`?
    pub fn is_disabled(&self, name: &str) -> bool {
        self.disable.iter().any(|entry| name_matches(entry, name))
    }

    /// Is `name` enabled by a `use` entry applicable in `workspace_root`?
    pub fn is_used_in(&self, name: &str, workspace_root: &Path) -> bool {
        self.used_names_in(workspace_root)
            .iter()
            .any(|entry| name_matches(entry, name))
    }
}

/// Does a configured entry name `name`? `"*"` matches everything; otherwise
/// the comparison is hyphen/underscore-insensitive, since these entries are
/// user-typed package names.
fn name_matches(entry: &str, name: &str) -> bool {
    entry == "*"
        || crate::crate_sources::normalize_crate_name(entry)
            == crate::crate_sources::normalize_crate_name(name)
}

/// One `[plugins] use` entry: a plugin name enabled deliberately, scoped
/// either to a single workspace or to every workspace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UseEntry {
    /// `use = ["name"]` — enabled in every workspace.
    Global(String),
    /// `use = [{ name = "...", workspace = "/path" }]` — enabled while
    /// working in the named workspace root.
    Workspace { name: String, workspace: PathBuf },
}

impl UseEntry {
    pub fn name(&self) -> &str {
        match self {
            UseEntry::Global(name) => name,
            UseEntry::Workspace { name, .. } => name,
        }
    }

    /// Does this entry apply while working in `workspace_root`?
    pub fn applies_in(&self, workspace_root: &Path) -> bool {
        match self {
            UseEntry::Global(_) => true,
            UseEntry::Workspace { workspace, .. } => {
                let canon = |p: &Path| fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
                canon(workspace) == canon(workspace_root)
            }
        }
    }
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
            plugins: PluginsConfig::default(),
            agents: Vec::new(),
            logging: LoggingConfig::default(),
            defaults: DefaultsConfig::default(),
            registries: Vec::new(),
        }
    }
}

/// Raw user config root as accepted in `~/.symposium/config.toml`.
#[derive(Debug, Deserialize)]
struct RawConfig {
    #[serde(default = "default_true", rename = "auto-sync")]
    auto_sync: bool,
    #[serde(default = "default_true", rename = "agents-syncing")]
    agents_syncing: bool,
    #[serde(default = "default_sync_debounce_secs", rename = "sync-debounce-secs")]
    sync_debounce_secs: u64,
    #[serde(default, rename = "hook-scope")]
    hook_scope: HookScope,
    #[serde(default, rename = "auto-update")]
    auto_update: AutoUpdate,
    #[serde(default)]
    telemetry: TelemetryConfig,
    #[serde(default)]
    plugins: PluginsConfig,
    #[serde(default, rename = "agent")]
    agents: Vec<AgentEntry>,
    #[serde(default)]
    logging: LoggingConfig,
    #[serde(default)]
    defaults: DefaultsConfig,
    #[serde(default, rename = "registry", alias = "plugin-source")]
    registries: Vec<RegistryConfig>,
}

impl Default for RawConfig {
    fn default() -> Self {
        Config::default().into()
    }
}

impl RawConfig {
    fn validate(self) -> Config {
        Config {
            auto_sync: self.auto_sync,
            agents_syncing: self.agents_syncing,
            sync_debounce_secs: self.sync_debounce_secs,
            hook_scope: self.hook_scope,
            auto_update: self.auto_update,
            telemetry: self.telemetry,
            plugins: self.plugins,
            agents: self.agents,
            logging: self.logging,
            defaults: self.defaults,
            registries: self.registries,
        }
    }
}

impl From<Config> for RawConfig {
    fn from(config: Config) -> Self {
        Self {
            auto_sync: config.auto_sync,
            agents_syncing: config.agents_syncing,
            sync_debounce_secs: config.sync_debounce_secs,
            hook_scope: config.hook_scope,
            auto_update: config.auto_update,
            telemetry: config.telemetry,
            plugins: config.plugins,
            agents: config.agents,
            logging: config.logging,
            defaults: config.defaults,
            registries: config.registries,
        }
    }
}

/// Controls which built-in registries are enabled.
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

/// A configured registry — a git repository or a local path offering plugins.
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct RegistryConfig {
    /// Display name for this registry. Plugins loaded from it are attributed
    /// to this name, which is also the `pm` component of the ids its package
    /// manager mints.
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

/// A registry together with its base directory for resolving relative paths.
#[derive(Debug, Clone)]
pub struct ResolvedRegistry {
    pub registry: RegistryConfig,
    /// Directory to resolve relative `path` values against.
    /// For user sources this is the user config dir; for project sources
    /// this is the project root.
    pub base_dir: PathBuf,
    /// The registry follows the recommendations convention: pm-named
    /// namespace directories (`cargo/<name>/` entries implicitly gated on
    /// `depends-on(<name>)`, `symposium/` entries unconditional) rather than
    /// a flat tree of plugins. Only the built-in recommendations registry
    /// sets this today; there is no config surface for it yet.
    pub recommendations: bool,
}

impl ResolvedRegistry {
    /// Where this registry's content lives on disk: the resolved `path`
    /// (relative entries against `base_dir`), or the git cache directory the
    /// repository is unpacked into. `None` when the entry names neither, or
    /// when its URL can't be turned into a cache path.
    ///
    /// Does no network I/O — just computes the path. Fetching git content is
    /// [`plugins::ensure_registries`](crate::plugins::ensure_registries)'s job.
    pub fn content_dir(&self, cache_dir: &Path) -> Option<PathBuf> {
        if let Some(path) = &self.registry.path {
            let p = PathBuf::from(path);
            return Some(if p.is_absolute() {
                p
            } else {
                self.base_dir.join(p)
            });
        }
        let git_url = self.registry.git.as_ref()?;
        let cache_mgr = symposium_install::git::GitCacheManager::from_cache_dir(
            &cache_dir.join(REGISTRY_CACHE_SUBDIR),
        );
        match cache_mgr.cache_path_for_url(git_url) {
            Some(path) => Some(path),
            None => {
                tracing::warn!(registry = %self.registry.name, url = %git_url, "bad registry URL");
                None
            }
        }
    }
}

/// Cache subdirectory holding git registries' unpacked content.
pub const REGISTRY_CACHE_SUBDIR: &str = "plugin-sources";

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

    /// The active package-manager set: the fixed ecosystem transports plus one
    /// instance per configured registry ([`registries`](Self::registries)).
    ///
    /// A recommendations registry gets a
    /// [`RecommendationsPm`](crate::pm::RecommendationsPm); everything else
    /// gets a [`PathPm`](crate::pm::PathPm) over its content directory —
    /// including git registries, whose repository is unpacked into the cache
    /// before it is read. Each instance is named for its registry, since that
    /// name is what its plugins are attributed to.
    pub fn package_managers(&self) -> crate::pm::PmRegistry {
        let instances = self
            .registries()
            .into_iter()
            .filter_map(|resolved| {
                let dir = resolved.content_dir(self.cache_dir())?;
                let name = resolved.registry.name;
                let pm: Box<dyn crate::pm::PackageManager + Send + Sync> =
                    if resolved.recommendations {
                        Box::new(crate::pm::RecommendationsPm::new(name.clone(), dir))
                    } else {
                        Box::new(crate::pm::PathPm::new(name.clone(), dir))
                    };
                Some(crate::pm::PmInstance { name, pm })
            })
            .collect();
        crate::pm::PmRegistry::new(instances)
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

    /// Returns the effective list of registries, including built-in defaults.
    pub fn registries(&self) -> Vec<ResolvedRegistry> {
        let mut registries = Vec::new();

        if self.config.defaults.symposium_recommendations {
            registries.push(ResolvedRegistry {
                registry: RegistryConfig {
                    name: "symposium-recommendations".to_string(),
                    git: Some(BUILTIN_RECOMMENDATIONS_URL.to_string()),
                    path: None,
                    auto_update: true,
                },
                base_dir: self.dirs.config_dir.clone(),
                recommendations: true,
            });
        }

        if self.config.defaults.user_plugins {
            registries.push(ResolvedRegistry {
                registry: RegistryConfig {
                    name: "user-plugins".to_string(),
                    git: None,
                    path: Some("plugins".to_string()),
                    auto_update: true,
                },
                base_dir: self.dirs.config_dir.clone(),
                recommendations: false,
            });
        }

        for registry in &self.config.registries {
            registries.push(ResolvedRegistry {
                registry: registry.clone(),
                base_dir: self.dirs.config_dir.clone(),
                recommendations: false,
            });
        }

        registries
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
        Ok(contents) => toml::from_str::<RawConfig>(&contents)
            .unwrap_or_else(|e| {
                eprintln!("warning: failed to parse {}: {e}", path.display());
                RawConfig::default()
            })
            .validate(),
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

    fn parse_config(toml: &str) -> Config {
        toml::from_str::<RawConfig>(toml).unwrap().validate()
    }

    #[test]
    fn parse_empty_config() {
        let config = parse_config("");
        assert!(config.defaults.symposium_recommendations);
        assert!(config.defaults.user_plugins);
        assert!(config.registries.is_empty());
    }

    #[test]
    fn parse_defaults_disable_recommendations() {
        let config = parse_config(indoc! {"
            [defaults]
            symposium-recommendations = false
        "});
        assert!(!config.defaults.symposium_recommendations);
        assert!(config.defaults.user_plugins);
    }

    #[test]
    fn parse_defaults_disable_user_plugins() {
        let config = parse_config(indoc! {"
            [defaults]
            user-plugins = false
        "});
        assert!(config.defaults.symposium_recommendations);
        assert!(!config.defaults.user_plugins);
    }

    #[test]
    fn parse_registry_git() {
        let config = parse_config(indoc! {r#"
            [[registry]]
            name = "my-org"
            git = "https://github.com/my-org/plugins"
            auto-update = false
        "#});
        assert_eq!(config.registries.len(), 1);
        assert_eq!(config.registries[0].name, "my-org");
        assert_eq!(
            config.registries[0].git.as_deref(),
            Some("https://github.com/my-org/plugins")
        );
        assert!(!config.registries[0].auto_update);
    }

    /// `[[plugin-source]]` is the retired spelling of `[[registry]]`.
    #[test]
    fn parse_retired_plugin_source_spelling() {
        let config = parse_config(indoc! {r#"
            [[plugin-source]]
            name = "my-org"
            git = "https://github.com/my-org/plugins"
        "#});
        assert_eq!(config.registries.len(), 1);
        assert_eq!(config.registries[0].name, "my-org");
    }

    #[test]
    fn parse_registry_path() {
        let config = parse_config(indoc! {r#"
            [[registry]]
            name = "local"
            path = "my-plugins"
        "#});
        assert_eq!(config.registries.len(), 1);
        assert_eq!(config.registries[0].path.as_deref(), Some("my-plugins"));
        assert!(config.registries[0].auto_update); // default true
    }

    #[test]
    fn parse_multiple_registries() {
        let config = parse_config(indoc! {r#"
            [defaults]
            symposium-recommendations = false

            [[registry]]
            name = "org-a"
            git = "https://github.com/a/plugins"

            [[registry]]
            name = "org-b"
            git = "https://github.com/b/plugins"
            auto-update = false

            [[registry]]
            name = "local"
            path = "extras"
        "#});
        assert!(!config.defaults.symposium_recommendations);
        assert_eq!(config.registries.len(), 3);
        assert_eq!(config.registries[0].name, "org-a");
        assert_eq!(config.registries[1].name, "org-b");
        assert_eq!(config.registries[2].name, "local");
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
        let config = parse_config(indoc! {r#"
            auto-sync = true

            [[agent]]
            name = "claude"

            [[agent]]
            name = "copilot"
        "#});
        assert_eq!(config.agents.len(), 2);
        assert_eq!(config.agents[0].name, "claude");
        assert_eq!(config.agents[1].name, "copilot");
        assert!(config.auto_sync);
    }

    #[test]
    fn parse_config_defaults() {
        let config = parse_config("");
        assert!(config.agents.is_empty());
        assert!(config.auto_sync); // default true
        assert!(config.agents_syncing); // default true
    }

    #[test]
    fn parse_telemetry_defaults_off() {
        let config = parse_config("");
        assert!(!config.telemetry.enabled);
    }

    #[test]
    fn parse_telemetry_enabled() {
        let config = parse_config(indoc! {"
            [telemetry]
            enabled = true
        "});
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
        let config = parse_config(indoc! {"
            agents-syncing = false
        "});
        assert!(!config.agents_syncing);
    }

    #[test]
    fn parse_plugins_defaults_are_empty() {
        let config = parse_config("");
        assert!(config.plugins.auto_enable.is_empty());
        assert!(config.plugins.used.is_empty());
        assert!(config.plugins.disable.is_empty());

        // An all-default section is not written back out.
        let serialized = toml::to_string_pretty(&config).unwrap();
        assert!(
            !serialized.contains("[plugins]"),
            "default enablement config should not be written: {serialized}"
        );
    }

    #[test]
    fn parse_plugins_enablement() {
        let config = parse_config(indoc! {r#"
            [plugins]
            auto-enable = ["widget-lib"]
            disable = ["noisy-crate"]
            use = ["everywhere", { name = "scoped", workspace = "/ws/a" }]
        "#});

        assert!(config.plugins.is_auto_enabled("widget_lib"));
        assert!(!config.plugins.is_auto_enabled("other"));
        assert!(config.plugins.is_disabled("noisy-crate"));

        assert_eq!(
            config.plugins.used,
            vec![
                UseEntry::Global("everywhere".into()),
                UseEntry::Workspace {
                    name: "scoped".into(),
                    workspace: PathBuf::from("/ws/a"),
                },
            ]
        );
        assert_eq!(
            config.plugins.used_names_in(Path::new("/ws/a")),
            vec!["everywhere", "scoped"]
        );
        assert_eq!(
            config.plugins.used_names_in(Path::new("/ws/other")),
            vec!["everywhere"]
        );
        assert!(config.plugins.is_used_in("scoped", Path::new("/ws/a")));
        assert!(!config.plugins.is_used_in("scoped", Path::new("/ws/other")));

        // Entries survive a round trip through the config file.
        let reparsed = parse_config(&toml::to_string_pretty(&config).unwrap());
        assert_eq!(reparsed.plugins, config.plugins);
    }

    #[test]
    fn auto_enable_wildcard_matches_every_name() {
        let config = parse_config(indoc! {r#"
            [plugins]
            auto-enable = ["*"]
        "#});
        assert!(config.plugins.is_auto_enabled("anything"));
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
