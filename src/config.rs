use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
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

    /// User-installed plugin sources in the registry-ready model.
    #[serde(default = "default_installed_sources")]
    pub installed: InstalledSourceConfig,

    /// User-configured discovery allow/deny policy.
    #[serde(default)]
    pub discovery: DiscoveryPolicy,
}

/// An `[[agent]]` entry — just identifies an agent by name.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AgentEntry {
    /// Agent name (e.g., "claude", "copilot", "gemini").
    pub name: String,
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
            agents: Vec::new(),
            logging: LoggingConfig::default(),
            defaults: DefaultsConfig::default(),
            plugin_source: Vec::new(),
            installed: default_installed_sources(),
            discovery: DiscoveryPolicy::default(),
        }
    }
}

/// Installed plugin-source declarations grouped by registry.
#[derive(Debug, Deserialize, Serialize, Clone, Default, PartialEq)]
pub struct InstalledSourceConfig {
    /// Cargo dependency-table entries keyed by crate name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub crates: BTreeMap<String, CargoDependencySpec>,

    /// Direct path-registry plugin sources.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,

    /// Direct git-registry plugin sources.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub git: Vec<String>,
}

fn default_installed_sources() -> InstalledSourceConfig {
    let mut crates = BTreeMap::new();
    crates.insert(
        "symposium-recommendations".to_string(),
        CargoDependencySpec::Version("1".to_string()),
    );
    InstalledSourceConfig {
        crates,
        paths: Vec::new(),
        git: Vec::new(),
    }
}

/// A Cargo dependency-table value.
///
/// This intentionally accepts arbitrary inline-table fields so Symposium can
/// pass Cargo-compatible dependency specs through without maintaining a partial
/// clone of Cargo's manifest schema.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(untagged)]
pub enum CargoDependencySpec {
    Version(String),
    Table(BTreeMap<String, toml::Value>),
}

impl CargoDependencySpec {
    pub fn version_req(&self) -> Option<&str> {
        match self {
            CargoDependencySpec::Version(version) => Some(version),
            CargoDependencySpec::Table(fields) => fields.get("version").and_then(toml_value_str),
        }
    }

    pub fn git(&self) -> Option<&str> {
        match self {
            CargoDependencySpec::Version(_) => None,
            CargoDependencySpec::Table(fields) => fields.get("git").and_then(toml_value_str),
        }
    }

    pub fn path(&self) -> Option<&str> {
        match self {
            CargoDependencySpec::Version(_) => None,
            CargoDependencySpec::Table(fields) => fields.get("path").and_then(toml_value_str),
        }
    }

    pub fn package(&self) -> Option<&str> {
        match self {
            CargoDependencySpec::Version(_) => None,
            CargoDependencySpec::Table(fields) => fields.get("package").and_then(toml_value_str),
        }
    }
}

fn toml_value_str(value: &toml::Value) -> Option<&str> {
    match value {
        toml::Value::String(s) => Some(s),
        _ => None,
    }
}

/// Top-level user discovery policy.
#[derive(Debug, Deserialize, Serialize, Clone, Default, PartialEq, Eq)]
pub struct DiscoveryPolicy {
    #[serde(default, skip_serializing_if = "DiscoveryRules::is_empty")]
    pub allow: DiscoveryRules,
    #[serde(default, skip_serializing_if = "DiscoveryRules::is_empty")]
    pub deny: DiscoveryRules,
}

/// Discovery rules for all registries.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DiscoveryRules {
    #[default]
    Empty,
    Any,
    Registries(DiscoveryRegistryRules),
}

impl DiscoveryRules {
    fn is_empty(&self) -> bool {
        matches!(self, DiscoveryRules::Empty)
    }
}

impl<'de> Deserialize<'de> for DiscoveryRules {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Scalar(String),
            Table(DiscoveryRegistryRules),
        }

        match Raw::deserialize(deserializer)? {
            Raw::Scalar(s) if s == "*" => Ok(DiscoveryRules::Any),
            Raw::Scalar(s) => Err(serde::de::Error::custom(format!(
                "unsupported discovery rule `{s}`; use `*` or a registry table"
            ))),
            Raw::Table(rules) if rules.is_empty() => Ok(DiscoveryRules::Empty),
            Raw::Table(rules) => Ok(DiscoveryRules::Registries(rules)),
        }
    }
}

impl Serialize for DiscoveryRules {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            DiscoveryRules::Empty => DiscoveryRegistryRules::default().serialize(serializer),
            DiscoveryRules::Any => serializer.serialize_str("*"),
            DiscoveryRules::Registries(rules) => rules.serialize(serializer),
        }
    }
}

/// Registry-specific discovery rules.
#[derive(Debug, Deserialize, Serialize, Clone, Default, PartialEq, Eq)]
pub struct DiscoveryRegistryRules {
    #[serde(default, skip_serializing_if = "RegistryDiscoveryRule::is_empty")]
    pub crates: RegistryDiscoveryRule,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub git: Vec<String>,
}

impl DiscoveryRegistryRules {
    fn is_empty(&self) -> bool {
        self.crates.is_empty() && self.paths.is_empty() && self.git.is_empty()
    }
}

/// A registry rule, either wildcard or keyed specs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RegistryDiscoveryRule {
    #[default]
    Empty,
    Any,
    Specs(BTreeMap<String, String>),
}

impl RegistryDiscoveryRule {
    fn is_empty(&self) -> bool {
        matches!(self, RegistryDiscoveryRule::Empty)
    }
}

impl<'de> Deserialize<'de> for RegistryDiscoveryRule {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Scalar(String),
            Specs(BTreeMap<String, String>),
        }

        match Raw::deserialize(deserializer)? {
            Raw::Scalar(s) if s == "*" => Ok(RegistryDiscoveryRule::Any),
            Raw::Scalar(s) => Err(serde::de::Error::custom(format!(
                "unsupported registry discovery rule `{s}`; use `*` or a map"
            ))),
            Raw::Specs(specs) if specs.is_empty() => Ok(RegistryDiscoveryRule::Empty),
            Raw::Specs(specs) => Ok(RegistryDiscoveryRule::Specs(specs)),
        }
    }
}

impl Serialize for RegistryDiscoveryRule {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            RegistryDiscoveryRule::Empty => BTreeMap::<String, String>::new().serialize(serializer),
            RegistryDiscoveryRule::Any => serializer.serialize_str("*"),
            RegistryDiscoveryRule::Specs(specs) => specs.serialize(serializer),
        }
    }
}

/// Parsed `<CRATE>[@<VERSION>]` install operand.
#[derive(Debug, Clone, PartialEq)]
pub struct CrateInstallSpec {
    pub name: String,
    pub dependency: CargoDependencySpec,
}

impl FromStr for CrateInstallSpec {
    type Err = anyhow::Error;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let (name, version) = match input.split_once('@') {
            Some((name, version)) => (name, Some(version)),
            None => (input, None),
        };

        if name.is_empty() {
            anyhow::bail!("crate install spec must include a crate name");
        }
        if name.contains('/') || name.contains('\\') {
            anyhow::bail!(
                "crate install spec `{input}` looks like a path; use `cargo agents install --path`"
            );
        }

        let dependency = match version {
            None => CargoDependencySpec::Version("*".to_string()),
            Some("") => anyhow::bail!("crate install spec `{input}` has an empty version"),
            Some(version) => CargoDependencySpec::Version(version.to_string()),
        };

        Ok(CrateInstallSpec {
            name: name.to_string(),
            dependency,
        })
    }
}

/// A crate-registry source declaration parsed from `source.crate`.
#[derive(Debug, Clone, PartialEq)]
pub struct CrateSourceSpec {
    /// The dependency-table key, when the source names a crate explicitly.
    ///
    /// `None` represents unkeyed Cargo dependency specs such as
    /// `source.crate = { path = "../my-crate" }`, where Cargo resolves the
    /// package name from the source itself.
    pub key: Option<String>,
    pub dependency: CargoDependencySpec,
}

/// Parse the value of a `source.crate` declaration.
///
/// Accepted forms:
/// - `"foo"`: shorthand for `foo = "*"`
/// - `{ foo = "1" }`: Cargo dependency table keyed by crate name
/// - `{ foo = { git = "..." } }`: keyed inline dependency table
/// - `{ path = "../foo" }`: unkeyed dependency table, package inferred by Cargo
pub fn parse_crate_source_value(value: toml::Value) -> anyhow::Result<Vec<CrateSourceSpec>> {
    match value {
        toml::Value::String(name) if !name.is_empty() => Ok(vec![CrateSourceSpec {
            key: Some(name),
            dependency: CargoDependencySpec::Version("*".to_string()),
        }]),
        toml::Value::String(_) => anyhow::bail!("source.crate shorthand must name a crate"),
        toml::Value::Table(table) if table.is_empty() => {
            anyhow::bail!("source.crate table must not be empty")
        }
        toml::Value::Table(table) if looks_like_dependency_spec(&table) => {
            Ok(vec![CrateSourceSpec {
                key: None,
                dependency: CargoDependencySpec::Table(table.into_iter().collect()),
            }])
        }
        toml::Value::Table(table) => table
            .into_iter()
            .map(|(key, value)| {
                let dependency = match value {
                    toml::Value::String(version) => CargoDependencySpec::Version(version),
                    toml::Value::Table(table) => {
                        CargoDependencySpec::Table(table.into_iter().collect())
                    }
                    other => anyhow::bail!(
                        "source.crate.{key} must be a version string or dependency table, got {}",
                        other.type_str()
                    ),
                };
                Ok(CrateSourceSpec {
                    key: Some(key),
                    dependency,
                })
            })
            .collect(),
        other => anyhow::bail!(
            "source.crate must be a crate name string or dependency table, got {}",
            other.type_str()
        ),
    }
}

fn looks_like_dependency_spec(table: &toml::map::Map<String, toml::Value>) -> bool {
    const DEPENDENCY_FIELDS: &[&str] = &[
        "version",
        "git",
        "path",
        "branch",
        "tag",
        "rev",
        "package",
        "registry",
        "default-features",
        "features",
        "optional",
    ];
    table
        .keys()
        .any(|key| DEPENDENCY_FIELDS.contains(&key.as_str()))
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

    /// Returns user-installed plugin sources in the registry-ready config shape.
    pub fn installed_sources(&self) -> &InstalledSourceConfig {
        &self.config.installed
    }

    /// Returns user-installed crate-registry plugin sources.
    pub fn installed_crates(&self) -> &BTreeMap<String, CargoDependencySpec> {
        &self.config.installed.crates
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
        assert_eq!(
            config.installed.crates.get("symposium-recommendations"),
            Some(&CargoDependencySpec::Version("1".to_string()))
        );
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
    fn parse_installed_crates_dependency_table() {
        let config: Config = toml::from_str(indoc! {r#"
            [installed.crates]
            symposium-recommendations = "1"
            pinned-plugin = "=1.2.0"
            my-org-plugins = { git = "https://github.com/my-org/my-org-plugins", branch = "main" }
            my-local-crate = { path = "/home/me/dev/my-crate", package = "actual-crate" }
        "#})
        .unwrap();

        assert_eq!(
            config.installed.crates["symposium-recommendations"].version_req(),
            Some("1")
        );
        assert_eq!(
            config.installed.crates["pinned-plugin"].version_req(),
            Some("=1.2.0")
        );
        assert_eq!(
            config.installed.crates["my-org-plugins"].git(),
            Some("https://github.com/my-org/my-org-plugins")
        );
        assert_eq!(
            config.installed.crates["my-local-crate"].path(),
            Some("/home/me/dev/my-crate")
        );
    }

    #[test]
    fn parse_installed_paths_and_git() {
        let config: Config = toml::from_str(indoc! {r#"
            [installed]
            paths = ["/home/me/dev/plugin-source", "../relative-plugin"]
            git = ["https://github.com/my-org/plugin-source"]
        "#})
        .unwrap();

        assert_eq!(
            config.installed.paths,
            vec!["/home/me/dev/plugin-source", "../relative-plugin"]
        );
        assert_eq!(
            config.installed.git,
            vec!["https://github.com/my-org/plugin-source"]
        );
    }

    #[test]
    fn installed_sources_round_trip() {
        let config: Config = toml::from_str(indoc! {r#"
            [installed]
            paths = ["/home/me/dev/plugin-source"]
            git = ["https://github.com/my-org/plugin-source"]

            [installed.crates]
            symposium-recommendations = "1"
            my-org-plugins = { git = "https://github.com/my-org/my-org-plugins", tag = "v1.0.0" }
        "#})
        .unwrap();

        let saved = toml::to_string_pretty(&config).unwrap();
        let reparsed: Config = toml::from_str(&saved).unwrap();
        assert_eq!(reparsed.installed, config.installed);
    }

    #[test]
    fn parse_discovery_policy_shorthands_and_tables() {
        let config: Config = toml::from_str(indoc! {r#"
            [discovery]
            allow = "*"

            [discovery.deny]
            crates = { unsafe-plugin = "*" }
            paths = ["/tmp/untrusted"]
            git = ["https://github.com/bad/*"]
        "#})
        .unwrap();

        assert_eq!(config.discovery.allow, DiscoveryRules::Any);
        let DiscoveryRules::Registries(deny) = config.discovery.deny else {
            panic!("expected registry-specific deny rules");
        };
        assert_eq!(
            deny.crates,
            RegistryDiscoveryRule::Specs(BTreeMap::from([(
                "unsafe-plugin".to_string(),
                "*".to_string()
            )]))
        );
        assert_eq!(deny.paths, vec!["/tmp/untrusted"]);
        assert_eq!(deny.git, vec!["https://github.com/bad/*"]);
    }

    #[test]
    fn parse_crate_install_specs() {
        let latest: CrateInstallSpec = "foo".parse().unwrap();
        assert_eq!(latest.name, "foo");
        assert_eq!(
            latest.dependency,
            CargoDependencySpec::Version("*".to_string())
        );

        let major: CrateInstallSpec = "foo@1".parse().unwrap();
        assert_eq!(
            major.dependency,
            CargoDependencySpec::Version("1".to_string())
        );

        let patch: CrateInstallSpec = "foo@1.2.3".parse().unwrap();
        assert_eq!(
            patch.dependency,
            CargoDependencySpec::Version("1.2.3".to_string())
        );

        let exact: CrateInstallSpec = "foo@=1.2.3".parse().unwrap();
        assert_eq!(
            exact.dependency,
            CargoDependencySpec::Version("=1.2.3".to_string())
        );
    }

    fn source_crate_value(toml: &str) -> toml::Value {
        let value: toml::Value = toml::from_str(toml).unwrap();
        value
            .get("source")
            .and_then(|source| source.get("crate"))
            .cloned()
            .unwrap()
    }

    #[test]
    fn parse_source_crate_string_shorthand() {
        let specs =
            parse_crate_source_value(source_crate_value(r#"source.crate = "foo""#)).unwrap();

        assert_eq!(
            specs,
            vec![CrateSourceSpec {
                key: Some("foo".to_string()),
                dependency: CargoDependencySpec::Version("*".to_string())
            }]
        );
    }

    #[test]
    fn parse_source_crate_dotted_dependency_table() {
        let specs = parse_crate_source_value(source_crate_value(indoc! {r#"
            [source.crate]
            foo = "1"
            bar = { git = "https://github.com/me/bar", branch = "main" }
        "#}))
        .unwrap();

        assert_eq!(specs[0].key.as_deref(), Some("bar"));
        assert_eq!(specs[0].dependency.git(), Some("https://github.com/me/bar"));
        assert_eq!(specs[1].key.as_deref(), Some("foo"));
        assert_eq!(specs[1].dependency.version_req(), Some("1"));
    }

    #[test]
    fn parse_source_crate_unkeyed_path_spec() {
        let specs = parse_crate_source_value(source_crate_value(indoc! {r#"
            source.crate = { path = "../my-crate", package = "actual-crate" }
        "#}))
        .unwrap();

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].key, None);
        assert_eq!(specs[0].dependency.path(), Some("../my-crate"));
    }

    #[test]
    fn parse_source_crate_rejects_unsupported_values() {
        let err =
            parse_crate_source_value(source_crate_value("source.crate = [\"foo\"]")).unwrap_err();
        assert!(
            err.to_string().contains("source.crate must be"),
            "unexpected error: {err}"
        );

        let err = parse_crate_source_value(source_crate_value(indoc! {r#"
            [source.crate]
            foo = ["1"]
        "#}))
        .unwrap_err();
        assert!(
            err.to_string().contains("source.crate.foo"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn symposium_installed_accessors_expose_new_config_without_changing_sources() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("config.toml"),
            indoc! {r#"
                [installed]
                paths = ["/home/me/dev/plugin-source"]

                [installed.crates]
                symposium-recommendations = "1"
                local-plugin = { path = "/home/me/dev/local-plugin" }
            "#},
        )
        .unwrap();

        let sym = Symposium::from_dir(tmp.path());
        assert_eq!(
            sym.installed_sources().paths,
            vec!["/home/me/dev/plugin-source"]
        );
        assert!(sym.installed_crates().contains_key("local-plugin"));

        let legacy_sources = sym.plugin_sources();
        assert!(
            legacy_sources
                .iter()
                .any(|s| s.source.name == "user-plugins")
        );
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
