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

#[derive(Debug, Serialize, Clone)]
pub struct Config {
    /// Automatically run `sync` when hooks are invoked.
    #[serde(rename = "auto-sync")]
    pub auto_sync: bool,

    /// Propagate user-authored skills from `.agents/skills/` to each
    /// configured agent's skill directory (e.g. `.claude/skills/`,
    /// `.kiro/skills/`). Skills that symposium itself installed into
    /// `.agents/skills/` are not propagated.
    #[serde(rename = "agents-syncing")]
    pub agents_syncing: bool,

    /// How many seconds after a successful sync we skip re-checking a skill
    /// directory. Set to 0 to disable debouncing (useful in tests).
    #[serde(rename = "sync-debounce-secs")]
    pub sync_debounce_secs: u64,

    /// Where to install agent hooks.
    #[serde(rename = "hook-scope", skip_serializing_if = "HookScope::is_default")]
    pub hook_scope: HookScope,

    /// Auto-update behavior for the symposium binary.
    #[serde(rename = "auto-update", skip_serializing_if = "AutoUpdate::is_default")]
    pub auto_update: AutoUpdate,

    /// Agents configured for this user.
    #[serde(rename = "agent")]
    pub agents: Vec<AgentEntry>,

    pub logging: LoggingConfig,

    /// User-installed plugin sources. New format uses `[[plugins]]` entries,
    /// but the legacy `[used]` format is transparently upgraded on read.
    ///
    /// This field is serialized as `[[plugins]]` and the legacy `[used]` key
    /// is never written on save.
    pub plugins: Vec<PluginsEntry>,

    /// Legacy compat: never serialized, only present during deserialization.
    #[serde(skip)]
    pub used: UsedSourceConfig,

    /// User-configured discovery allow/deny policy.
    pub discovery: DiscoveryPolicy,
}

impl<'de> Deserialize<'de> for Config {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
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
            #[serde(default, rename = "agent")]
            agents: Vec<AgentEntry>,
            #[serde(default)]
            logging: LoggingConfig,
            #[serde(default)]
            used: Option<UsedSourceConfig>,
            #[serde(default)]
            plugins: Option<Vec<PluginsEntry>>,
            #[serde(default)]
            discovery: DiscoveryPolicy,
        }
        let raw = RawConfig::deserialize(deserializer)?;

        // Determine plugins entries: prefer [[plugins]] if present,
        // otherwise migrate legacy [used] to a single global entry.
        let plugins = match (raw.plugins, raw.used) {
            (Some(plugins), _) => plugins,
            (None, Some(used)) => {
                vec![PluginsEntry {
                    predicates: crate::predicate::PredicateSet::default(),
                    source: PluginsEntrySource {
                        crates: used.crates,
                        paths: used.paths,
                        git: used.git,
                    },
                }]
            }
            (None, None) => default_plugins(),
        };

        // Build the compatibility `used` view: merge all global (no-predicate) entries.
        let used = plugins_to_used(&plugins);

        Ok(Config {
            auto_sync: raw.auto_sync,
            agents_syncing: raw.agents_syncing,
            sync_debounce_secs: raw.sync_debounce_secs,
            hook_scope: raw.hook_scope,
            auto_update: raw.auto_update,
            agents: raw.agents,
            logging: raw.logging,
            plugins,
            used,
            discovery: raw.discovery,
        })
    }
}

/// Build a legacy `UsedSourceConfig` view by merging all plugin entries
/// (regardless of predicates) for backward compatibility with callers that
/// still use `sym.used_sources()` / `sym.used_crates()`.
fn plugins_to_used(plugins: &[PluginsEntry]) -> UsedSourceConfig {
    let mut used = UsedSourceConfig::default();
    for entry in plugins {
        used.crates.extend(entry.source.crates.clone());
        used.paths.extend(entry.source.paths.clone());
        used.git.extend(entry.source.git.clone());
    }
    used
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
        let plugins = default_plugins();
        let used = plugins_to_used(&plugins);
        Self {
            auto_sync: true,
            agents_syncing: true,
            sync_debounce_secs: default_sync_debounce_secs(),
            hook_scope: HookScope::default(),
            auto_update: AutoUpdate::default(),
            agents: Vec::new(),
            logging: LoggingConfig::default(),
            plugins,
            used,
            discovery: DiscoveryPolicy::default(),
        }
    }
}

/// Installed plugin-source declarations grouped by registry.
///
/// Legacy type retained for backward-compatible deserialization of `[used]`.
/// New code should use `PluginsEntry` / `PluginsEntrySource` instead.
#[derive(Debug, Deserialize, Serialize, Clone, Default, PartialEq)]
pub struct UsedSourceConfig {
    /// Cargo dependency-table entries keyed by crate name.
    #[serde(
        default = "default_used_crates",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub crates: BTreeMap<String, CargoDependencySpec>,

    /// Direct path-registry plugin sources.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,

    /// Direct git-registry plugin sources.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub git: Vec<String>,
}

/// A `[[plugins]]` entry in user config. Each entry conditionally contributes
/// plugin sources gated by its `where.*` predicates.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginsEntry {
    /// Activation predicates (from `where.predicates`). Empty means always active.
    pub predicates: crate::predicate::PredicateSet,
    /// Plugin sources contributed by this entry.
    pub source: PluginsEntrySource,
}

/// Plugin sources within a single `[[plugins]]` entry.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PluginsEntrySource {
    /// Cargo dependency-table entries keyed by crate name.
    pub crates: BTreeMap<String, CargoDependencySpec>,
    /// Direct path-registry plugin sources.
    pub paths: Vec<String>,
    /// Direct git-registry plugin sources.
    pub git: Vec<String>,
}

impl PluginsEntrySource {
    pub fn is_empty(&self) -> bool {
        self.crates.is_empty() && self.paths.is_empty() && self.git.is_empty()
    }
}

impl Serialize for PluginsEntry {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        if !self.predicates.is_empty() {
            #[derive(Serialize)]
            struct Where<'a> {
                predicates: &'a crate::predicate::PredicateSet,
            }
            map.serialize_entry(
                "where",
                &Where {
                    predicates: &self.predicates,
                },
            )?;
        }
        #[derive(Serialize)]
        struct Source<'a> {
            #[serde(skip_serializing_if = "BTreeMap::is_empty")]
            crates: &'a BTreeMap<String, CargoDependencySpec>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            paths: &'a Vec<String>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            git: &'a Vec<String>,
        }
        map.serialize_entry(
            "source",
            &Source {
                crates: &self.source.crates,
                paths: &self.source.paths,
                git: &self.source.git,
            },
        )?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for PluginsEntry {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct WhereRaw {
            #[serde(default)]
            predicates: crate::predicate::PredicateSet,
        }
        #[derive(Deserialize)]
        struct SourceRaw {
            #[serde(default)]
            crates: BTreeMap<String, CargoDependencySpec>,
            #[serde(default)]
            paths: Vec<String>,
            #[serde(default)]
            git: Vec<String>,
        }
        #[derive(Deserialize)]
        struct Raw {
            #[serde(default, rename = "where")]
            where_clause: Option<WhereRaw>,
            #[serde(default)]
            source: Option<SourceRaw>,
        }
        let raw = Raw::deserialize(deserializer)?;
        let predicates = raw.where_clause.map(|w| w.predicates).unwrap_or_default();
        let source = match raw.source {
            Some(s) => PluginsEntrySource {
                crates: s.crates,
                paths: s.paths,
                git: s.git,
            },
            None => PluginsEntrySource::default(),
        };
        Ok(PluginsEntry { predicates, source })
    }
}

fn default_plugins() -> Vec<PluginsEntry> {
    vec![PluginsEntry {
        predicates: crate::predicate::PredicateSet::default(),
        source: PluginsEntrySource {
            crates: default_used_crates(),
            paths: Vec::new(),
            git: Vec::new(),
        },
    }]
}

fn default_used_crates() -> BTreeMap<String, CargoDependencySpec> {
    let mut crates = BTreeMap::new();
    crates.insert(
        "symposium-recommendations".to_string(),
        CargoDependencySpec::Version("1".to_string()),
    );
    crates
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

/// Parsed `<CRATE>[@<VERSION>]` use operand.
#[derive(Debug, Clone, PartialEq)]
pub struct CrateUseSpec {
    pub name: String,
    pub dependency: CargoDependencySpec,
}

impl FromStr for CrateUseSpec {
    type Err = anyhow::Error;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let (name, version) = match input.split_once('@') {
            Some((name, version)) => (name, Some(version)),
            None => (input, None),
        };

        if name.is_empty() {
            anyhow::bail!("crate use spec must include a crate name");
        }
        if name.contains('/') || name.contains('\\') {
            anyhow::bail!(
                "crate use spec `{input}` looks like a path; use `cargo agents use --path`"
            );
        }

        let dependency = match version {
            None => CargoDependencySpec::Version("*".to_string()),
            Some("") => anyhow::bail!("crate use spec `{input}` has an empty version"),
            Some(version) => CargoDependencySpec::Version(version.to_string()),
        };

        Ok(CrateUseSpec {
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

    /// Returns user-installed plugin sources in the registry-ready config shape.
    pub fn used_sources(&self) -> &UsedSourceConfig {
        &self.config.used
    }

    /// Returns user-installed crate-registry plugin sources.
    pub fn used_crates(&self) -> &BTreeMap<String, CargoDependencySpec> {
        &self.config.used.crates
    }

    /// Rebuild the legacy `used` compatibility view from `plugins`.
    pub fn rebuild_used_compat(&mut self) {
        self.config.used = plugins_to_used(&self.config.plugins);
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
        Ok(contents) => toml::from_str(&contents)
            .unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display())),
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
        assert_eq!(
            config.used.crates.get("symposium-recommendations"),
            Some(&CargoDependencySpec::Version("1".to_string()))
        );
    }

    #[test]
    fn legacy_defaults_are_rejected() {
        let err = toml::from_str::<Config>(indoc! {"
            [defaults]
            symposium-recommendations = false
        "})
        .unwrap_err();
        assert!(err.to_string().contains("unknown field"), "{err}");
    }

    #[test]
    fn legacy_plugin_source_is_rejected() {
        let err = toml::from_str::<Config>(indoc! {r#"
            [[plugin-source]]
            name = "my-org"
            git = "https://github.com/my-org/plugins"
            auto-update = false
        "#})
        .unwrap_err();
        assert!(err.to_string().contains("unknown field"), "{err}");
    }

    #[test]
    fn parse_used_crates_dependency_table() {
        let config: Config = toml::from_str(indoc! {r#"
            [used.crates]
            symposium-recommendations = "1"
            pinned-plugin = "=1.2.0"
            my-org-plugins = { git = "https://github.com/my-org/my-org-plugins", branch = "main" }
            my-local-crate = { path = "/home/me/dev/my-crate", package = "actual-crate" }
        "#})
        .unwrap();

        assert_eq!(
            config.used.crates["symposium-recommendations"].version_req(),
            Some("1")
        );
        assert_eq!(
            config.used.crates["pinned-plugin"].version_req(),
            Some("=1.2.0")
        );
        assert_eq!(
            config.used.crates["my-org-plugins"].git(),
            Some("https://github.com/my-org/my-org-plugins")
        );
        assert_eq!(
            config.used.crates["my-local-crate"].path(),
            Some("/home/me/dev/my-crate")
        );
    }

    #[test]
    fn parse_used_paths_and_git() {
        let config: Config = toml::from_str(indoc! {r#"
            [used]
            paths = ["/home/me/dev/plugin-source", "../relative-plugin"]
            git = ["https://github.com/my-org/plugin-source"]
        "#})
        .unwrap();

        assert_eq!(
            config.used.paths,
            vec!["/home/me/dev/plugin-source", "../relative-plugin"]
        );
        assert_eq!(
            config.used.git,
            vec!["https://github.com/my-org/plugin-source"]
        );
    }

    #[test]
    fn used_sources_round_trip() {
        let config: Config = toml::from_str(indoc! {r#"
            [used]
            paths = ["/home/me/dev/plugin-source"]
            git = ["https://github.com/my-org/plugin-source"]

            [used.crates]
            symposium-recommendations = "1"
            my-org-plugins = { git = "https://github.com/my-org/my-org-plugins", tag = "v1.0.0" }
        "#})
        .unwrap();

        let saved = toml::to_string_pretty(&config).unwrap();
        let reparsed: Config = toml::from_str(&saved).unwrap();
        assert_eq!(reparsed.used, config.used);
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
        let latest: CrateUseSpec = "foo".parse().unwrap();
        assert_eq!(latest.name, "foo");
        assert_eq!(
            latest.dependency,
            CargoDependencySpec::Version("*".to_string())
        );

        let major: CrateUseSpec = "foo@1".parse().unwrap();
        assert_eq!(
            major.dependency,
            CargoDependencySpec::Version("1".to_string())
        );

        let patch: CrateUseSpec = "foo@1.2.3".parse().unwrap();
        assert_eq!(
            patch.dependency,
            CargoDependencySpec::Version("1.2.3".to_string())
        );

        let exact: CrateUseSpec = "foo@=1.2.3".parse().unwrap();
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
    fn symposium_used_accessors_expose_new_config() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("config.toml"),
            indoc! {r#"
                [used]
                paths = ["/home/me/dev/plugin-source"]

                [used.crates]
                symposium-recommendations = "1"
                local-plugin = { path = "/home/me/dev/local-plugin" }
            "#},
        )
        .unwrap();

        let sym = Symposium::from_dir(tmp.path());
        assert_eq!(sym.used_sources().paths, vec!["/home/me/dev/plugin-source"]);
        assert!(sym.used_crates().contains_key("local-plugin"));
    }

    #[test]
    fn from_dir_creates_default_config() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(tmp.path());
        assert!(sym.used_crates().contains_key("symposium-recommendations"));
        assert_eq!(sym.config_dir(), tmp.path());
        assert_eq!(sym.cache_dir(), tmp.path().join("cache"));
    }

    #[test]
    fn from_dir_reads_config_file() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("config.toml"),
            indoc! {r#"
                auto-sync = false

                [used]
                paths = ["/tmp/plugin-source"]
            "#},
        )
        .unwrap();
        let sym = Symposium::from_dir(tmp.path());
        assert!(!sym.config.auto_sync);
        assert_eq!(sym.used_sources().paths, vec!["/tmp/plugin-source"]);
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

    // --- [[plugins]] config format tests ---

    #[test]
    fn parse_plugins_array_basic() {
        let config: Config = toml::from_str(indoc! {r#"
            [[plugins]]
            source.crates = { foo = "1" }
        "#})
        .unwrap();
        assert_eq!(config.plugins.len(), 1);
        assert!(config.plugins[0].predicates.is_empty());
        assert_eq!(
            config.plugins[0].source.crates["foo"],
            CargoDependencySpec::Version("1".to_string())
        );
    }

    #[test]
    fn parse_plugins_array_with_predicates() {
        let config: Config = toml::from_str(indoc! {r#"
            [[plugins]]
            where.predicates = ["directory(/tmp/foo/**)"]
            source.crates = { bar = "2" }
        "#})
        .unwrap();
        assert_eq!(config.plugins.len(), 1);
        assert!(!config.plugins[0].predicates.is_empty());
        assert_eq!(config.plugins[0].predicates.predicates.len(), 1);
        assert_eq!(
            config.plugins[0].predicates.predicates[0].to_string(),
            "directory(/tmp/foo/**)"
        );
    }

    #[test]
    fn parse_plugins_multiple_entries() {
        let config: Config = toml::from_str(indoc! {r#"
            [[plugins]]
            source.crates = { symposium-recommendations = "1" }

            [[plugins]]
            where.predicates = ["directory(/tmp/project/**)"]
            source.crates = { foo = "1" }
            source.paths = ["/home/me/plugin"]
        "#})
        .unwrap();
        assert_eq!(config.plugins.len(), 2);
        assert!(config.plugins[0].predicates.is_empty());
        assert!(!config.plugins[1].predicates.is_empty());
        assert_eq!(config.plugins[1].source.paths, vec!["/home/me/plugin"]);
    }

    #[test]
    fn parse_legacy_used_format_migrates_to_plugins() {
        let config: Config = toml::from_str(indoc! {r#"
            [used]
            paths = ["/home/me/dev/plugin-source"]
            git = ["https://github.com/org/plugin"]

            [used.crates]
            foo = "1"
        "#})
        .unwrap();
        // Legacy format becomes a single global plugins entry
        assert_eq!(config.plugins.len(), 1);
        assert!(config.plugins[0].predicates.is_empty());
        assert_eq!(
            config.plugins[0].source.crates["foo"],
            CargoDependencySpec::Version("1".to_string())
        );
        assert_eq!(
            config.plugins[0].source.paths,
            vec!["/home/me/dev/plugin-source"]
        );
        assert_eq!(
            config.plugins[0].source.git,
            vec!["https://github.com/org/plugin"]
        );
        // Compat accessor still works
        assert_eq!(config.used.paths, vec!["/home/me/dev/plugin-source"]);
    }

    #[test]
    fn plugins_entries_round_trip() {
        let config: Config = toml::from_str(indoc! {r#"
            [[plugins]]
            source.crates = { symposium-recommendations = "1" }

            [[plugins]]
            where.predicates = ["directory(/tmp/foo/**)"]
            source.crates = { bar = "2" }
            source.paths = ["/home/me/plugin"]
        "#})
        .unwrap();
        let serialized = toml::to_string_pretty(&config).unwrap();
        let reparsed: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(reparsed.plugins.len(), 2);
        assert_eq!(
            reparsed.plugins[0].source.crates,
            config.plugins[0].source.crates
        );
        assert_eq!(reparsed.plugins[1].predicates.predicates.len(), 1);
        assert_eq!(reparsed.plugins[1].source.paths, vec!["/home/me/plugin"]);
    }

    #[test]
    fn plugins_compat_used_merges_all_entries() {
        let config: Config = toml::from_str(indoc! {r#"
            [[plugins]]
            source.crates = { foo = "1" }

            [[plugins]]
            where.predicates = ["directory(/tmp/foo/**)"]
            source.crates = { bar = "2" }
        "#})
        .unwrap();
        // Compat view merges crates from all entries
        assert!(config.used.crates.contains_key("foo"));
        assert!(config.used.crates.contains_key("bar"));
    }
}
