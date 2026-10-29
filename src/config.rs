use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::Level;

use crate::pm::PackageId;

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
    #[serde(default, rename = "agent", skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<AgentEntry>,

    #[serde(default)]
    pub logging: LoggingConfig,

    /// Default registries that are always included unless disabled.
    #[serde(default)]
    pub defaults: DefaultsConfig,

    /// User-defined registries (git repos or local paths).
    #[serde(default, rename = "registry", skip_serializing_if = "Vec::is_empty")]
    pub registries: Vec<RegistryConfig>,

    /// Package managers for ecosystems beyond cargo, each a binary speaking
    /// the PM protocol.
    #[serde(
        default,
        rename = "package-manager",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub package_managers: Vec<PackageManagerConfig>,
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
    /// about; `"*"` pre-consents to every dependency.
    #[serde(default, rename = "auto-enable", skip_serializing_if = "Vec::is_empty")]
    pub auto_enable: Vec<PluginRef>,

    /// Plugins enabled deliberately, the durable record `cargo agents use`
    /// writes. Unlike `auto-enable` (consent for what a dependency already
    /// carries), a used plugin is enabled whether or not any dependency
    /// references it — it is also what wakes a dormant registry plugin.
    #[serde(default, rename = "use", skip_serializing_if = "Vec::is_empty")]
    pub used: Vec<UseEntry>,

    /// Plugin names pruned from enablement, and the record of declined
    /// discoveries (so they are not offered again).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disable: Vec<PluginRef>,
}

impl PluginsConfig {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The plugins `use` entries enable while working in `workspace_root`.
    pub fn used_in(&self, workspace_root: &Path) -> Vec<&PluginRef> {
        self.used
            .iter()
            .filter(|entry| entry.applies_in(workspace_root))
            .map(UseEntry::plugin)
            .collect()
    }

    /// Is this plugin pre-consented to by `auto-enable`? `deps` is what a
    /// bare name is resolved against: consent is for what a dependency
    /// carries, so a bare name covers the dependency of that name, and covers
    /// nothing while dependencies from two package managers share it.
    pub fn is_auto_enabled(
        &self,
        id: &PackageId,
        deps: &[PackageId],
        spellings: &Spellings,
    ) -> bool {
        self.auto_enable.iter().any(|entry| match &entry.pm {
            Some(_) => spellings.names(entry, id),
            None if entry.name == "*" => true,
            None => {
                spellings.names(entry, id)
                    && deps
                        .iter()
                        .all(|dep| dep.pm == id.pm || !spellings.names(entry, dep))
            }
        })
    }

    /// Does `disable` name the package `id`? A bare name is not resolved: it
    /// turns off every plugin of that name, since turning off too much is the
    /// safe way to be wrong.
    pub fn is_disabled(&self, id: &PackageId, spellings: &Spellings) -> bool {
        self.disable.iter().any(|entry| spellings.names(entry, id))
    }

    /// Whether any enablement entry could pull in a crate plugin. With neither
    /// `auto-enable` nor `use` naming anything,
    /// [`enabled_dependencies`](crate::discovery::enabled_dependencies) is empty
    /// regardless of the dependency graph — so a caller can skip resolving the
    /// workspace crates on that basis.
    pub fn has_enablement_entries(&self) -> bool {
        !self.auto_enable.is_empty() || !self.used.is_empty()
    }
}

/// The packages `[plugins]` entries name in one workspace under another
/// spelling than their own: cargo's `widget-lib` for an entry `widget_lib`.
/// Whether two names are one package is its package manager's to say
/// ([`canonical_name`](crate::pm::PackageManager::canonical_name)), which can
/// mean asking it, so this is resolved once for each package, by
/// [`Names::spellings`](crate::discovery::Names::spellings) and
/// [`Names::spell`](crate::discovery::Names::spell), and compared against from
/// then on.
#[derive(Debug, Default, Clone)]
pub struct Spellings(Vec<(PluginRef, PackageId)>);

impl Spellings {
    /// Record that `entry` names `id`.
    pub fn add(&mut self, entry: PluginRef, id: PackageId) {
        self.0.push((entry, id));
    }

    /// Does `entry` name `id`, as spelled or under another spelling?
    pub fn names(&self, entry: &PluginRef, id: &PackageId) -> bool {
        entry.matches(id)
            || self
                .0
                .iter()
                .any(|(e, spelled)| e == entry && spelled.pm == id.pm && spelled.name == id.name)
    }
}

/// A plugin named in configuration: the canonical name a package manager knows
/// it by, and optionally which package manager that is.
///
/// A name alone is not an identity: two ecosystems may use the same word, and
/// what a name *means* is the offering PM's business (a crate name for cargo,
/// the plugin's own `name` for a registry). But it usually is unambiguous, so
/// `pm` may be left out, and a bare name names whichever plugin of that name
/// there is. Where it could mean more than one, it is resolved by the code
/// reading it: see [`crate::discovery::Names`] for `use`, and
/// [`PluginsConfig::is_auto_enabled`] and [`PluginsConfig::is_disabled`].
/// Whether it names a package spelled another way is resolved per workspace,
/// in [`Spellings`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "RawPluginRef", into = "RawPluginRef")]
pub struct PluginRef {
    /// The package manager, or `None` for a bare name.
    pub pm: Option<String>,
    pub name: String,
}

impl PluginRef {
    pub fn new(pm: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            pm: Some(pm.into()),
            name: name.into(),
        }
    }

    /// A name that leaves its package manager unstated.
    pub fn bare(name: impl Into<String>) -> Self {
        Self {
            pm: None,
            name: name.into(),
        }
    }

    /// Does this entry name `id`, spelled as `id` is? A bare entry names a
    /// plugin of its name in any package manager. `"*"` names every package
    /// of the entry's package manager, or of every package manager when the
    /// entry names none.
    ///
    /// A package manager may spell a package more than one way (cargo's
    /// `widget-lib` is also `widget_lib`), which only it can say, so an entry
    /// spelled another way names a package through the [`Spellings`] resolved
    /// for the workspace.
    pub fn matches(&self, id: &PackageId) -> bool {
        self.pm.as_ref().is_none_or(|pm| *pm == id.pm) && (self.name == "*" || self.name == id.name)
    }
}

impl std::fmt::Display for PluginRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.pm {
            Some(pm) => write!(f, "{pm}:{}", self.name),
            None => f.write_str(&self.name),
        }
    }
}

/// The accepted spellings of a plugin reference: a bare string, or a table
/// whose `pm` may be left out. A bare reference is written as a bare string,
/// the spelling earlier releases read.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum RawPluginRef {
    Bare(String),
    Qualified(RawPluginTable),
}

/// The table spelling of a plugin reference. An unknown key is an error, so a
/// mistyped one is not silently dropped.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPluginTable {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pm: Option<String>,
    name: String,
}

impl From<RawPluginRef> for PluginRef {
    fn from(raw: RawPluginRef) -> Self {
        match raw {
            RawPluginRef::Bare(name) => PluginRef::bare(name),
            RawPluginRef::Qualified(RawPluginTable { pm, name }) => PluginRef { pm, name },
        }
    }
}

impl From<&str> for PluginRef {
    fn from(name: &str) -> Self {
        PluginRef::bare(name)
    }
}

impl From<PluginRef> for RawPluginRef {
    fn from(r: PluginRef) -> Self {
        match r.pm {
            None => RawPluginRef::Bare(r.name),
            pm => RawPluginRef::Qualified(RawPluginTable { pm, name: r.name }),
        }
    }
}

/// One `[plugins] use` entry: a plugin enabled deliberately, scoped either to
/// a single workspace or to every workspace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "RawUseEntry", into = "RawUseEntry")]
pub struct UseEntry {
    pub plugin: PluginRef,
    /// The workspace this applies in, or `None` for every workspace.
    pub workspace: Option<PathBuf>,
}

/// The accepted spellings of a `use` entry. A bare string is a bare name
/// enabled everywhere.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum RawUseEntry {
    Bare(String),
    Table(RawUseTable),
}

/// The table spelling of a `use` entry. An unknown key is an error: a
/// mistyped `workspace` would otherwise make a scoped entry global.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawUseTable {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pm: Option<String>,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    workspace: Option<PathBuf>,
}

impl From<RawUseEntry> for UseEntry {
    fn from(raw: RawUseEntry) -> Self {
        match raw {
            RawUseEntry::Bare(name) => UseEntry {
                plugin: PluginRef::from(name.as_str()),
                workspace: None,
            },
            RawUseEntry::Table(RawUseTable {
                pm,
                name,
                workspace,
            }) => UseEntry {
                plugin: PluginRef { pm, name },
                workspace,
            },
        }
    }
}

/// A bare entry is written as earlier releases wrote it: a bare name, or a
/// table of `name` and `workspace` when scoped. See [`RawPluginRef`].
impl From<UseEntry> for RawUseEntry {
    fn from(e: UseEntry) -> Self {
        match (e.plugin.pm, e.workspace) {
            (None, None) => RawUseEntry::Bare(e.plugin.name),
            (pm, workspace) => RawUseEntry::Table(RawUseTable {
                pm,
                name: e.plugin.name,
                workspace,
            }),
        }
    }
}

impl UseEntry {
    /// Enabled in every workspace.
    pub fn global(plugin: PluginRef) -> Self {
        Self {
            plugin,
            workspace: None,
        }
    }

    /// Enabled while working in `workspace`.
    pub fn scoped(plugin: PluginRef, workspace: PathBuf) -> Self {
        Self {
            plugin,
            workspace: Some(workspace),
        }
    }

    pub fn plugin(&self) -> &PluginRef {
        &self.plugin
    }

    pub fn name(&self) -> &str {
        &self.plugin.name
    }

    pub fn is_global(&self) -> bool {
        self.workspace.is_none()
    }

    /// Does this entry apply while working in `workspace_root`?
    pub fn applies_in(&self, workspace_root: &Path) -> bool {
        match &self.workspace {
            None => true,
            Some(workspace) => {
                let canon = |p: &Path| fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
                canon(workspace) == canon(workspace_root)
            }
        }
    }
}

/// An `[[agent]]` entry — just identifies an agent by name.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AgentEntry {
    /// Agent name (e.g., "antigravity", "claude", "copilot").
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
            package_managers: Vec::new(),
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
    #[serde(default, rename = "registry")]
    registries: Vec<RegistryConfig>,
    #[serde(default, rename = "package-manager")]
    package_managers: Vec<PackageManagerConfig>,
}

impl RawConfig {
    fn validate(self) -> Result<Config, String> {
        self.check_package_manager_names()?;
        Ok(Config {
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
            package_managers: self.package_managers,
        })
    }

    /// A `[[package-manager]]` name is what ids route by, what `[plugins]`
    /// entries and `depends-on` atoms name, and a directory under the cache.
    /// So it must be a plain identifier, and no other package manager or
    /// registry may have it: the first would shadow the second silently.
    /// `cargo` is allowed, since it replaces the built-in cargo instance.
    fn check_package_manager_names(&self) -> Result<(), String> {
        let mut builtin_registries = Vec::new();
        if self.defaults.symposium_recommendations {
            builtin_registries.push("symposium-recommendations");
        }
        if self.defaults.user_plugins {
            builtin_registries.push("user-plugins");
        }
        let mut seen: Vec<&str> = Vec::new();
        for pm in &self.package_managers {
            let name = pm.name.as_str();
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
            {
                return Err(format!(
                    "`[[package-manager]]` name {name:?} must be letters, digits, `_` and `-`"
                ));
            }
            let registry = builtin_registries
                .iter()
                .copied()
                .chain(self.registries.iter().map(|r| r.name.as_str()))
                .any(|r| r == name);
            if registry || seen.contains(&name) {
                return Err(format!(
                    "`[[package-manager]]` name `{name}` is already taken by another package manager or registry"
                ));
            }
            seen.push(name);
        }
        Ok(())
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

/// A `[[package-manager]]` entry: an ecosystem Symposium does not know natively,
/// reached by running a binary that speaks the PM protocol.
///
/// This is the bootstrap channel for the PM layer. The cargo PM is built in;
/// anything else (npm, pypi, a private ecosystem) arrives here. An entry
/// named `cargo` *replaces* the built-in in-process instance, which is how you
/// run even the cargo PM out of process.
///
/// A configured PM is an ecosystem transport, so its `active_plugins` are the
/// plugins embedded in the workspace's dependencies. Those are **not** trusted,
/// for the same reason cargo's aren't: depending on a package should not let
/// its author inject agent context. Consent still applies.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PackageManagerConfig {
    /// The ecosystem this PM owns: the `pm` component of every id it mints.
    /// The PM must report the same name at `initialize`, or it is refused:
    /// ids route by this name, so a mismatch would misroute silently.
    pub name: String,

    /// The binary to run. A bare name is resolved on `PATH`.
    pub command: String,

    /// Extra arguments, before the protocol takes over stdio.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
}

// ---------------------------------------------------------------------------
// Merged configuration view
// ---------------------------------------------------------------------------

/// Cache subdirectory holding git registries' unpacked content.
pub const REGISTRY_CACHE_SUBDIR: &str = "plugin-sources";

const BUILTIN_RECOMMENDATIONS_URL: &str = "https://github.com/symposium-dev/recommendations";

/// Full application context: parsed config + resolved directory paths.
///
/// Thread `&Symposium` through all call sites instead of using global state.
#[derive(Clone)]
pub struct Symposium {
    pub config: Config,
    config_error: Option<String>,
    dirs: crate::dirs::SymposiumDirs,
    home_dir: PathBuf,
    env: EnvLookup,
}

type EnvLookup = Arc<dyn Fn(&str) -> Option<std::ffi::OsString> + Send + Sync>;

impl Symposium {
    /// Production constructor: resolves paths from environment.
    ///
    /// Resolution order for config dir:
    /// 1. `SYMPOSIUM_HOME` env var
    /// 2. `XDG_CONFIG_HOME/symposium`
    /// 3. `~/.symposium`
    pub fn from_environment() -> Self {
        let home_dir = dirs::home_dir().expect("could not determine home directory");
        let dirs = crate::dirs::SymposiumDirs::from_environment();
        let _ = fs::create_dir_all(&dirs.config_dir);
        let _ = fs::create_dir_all(&dirs.cache_dir);

        let (config, config_error) = load_config_from(&dirs.config_dir);

        // Note: can't use tracing here — logging isn't initialized yet.
        // init_logging() is called after construction.

        Self {
            config,
            config_error,
            dirs,
            home_dir,
            env: Arc::new(|name| env::var_os(name)),
        }
    }

    /// Test constructor: everything rooted under a single directory.
    ///
    /// Creates `config.toml` from the provided config if not already present.
    pub fn from_dir(root: &Path) -> Self {
        let config_dir = root.to_path_buf();
        let _ = fs::create_dir_all(&config_dir);

        let (config, config_error) = load_config_from(&config_dir);

        let cache_dir = config_dir.join("cache");
        let _ = fs::create_dir_all(&cache_dir);

        // In test mode, use the root as the home directory so that
        // global hook registration writes into the tempdir.
        let home_dir = root.to_path_buf();

        let dirs = crate::dirs::SymposiumDirs::new(config_dir, cache_dir, None);

        Self {
            config,
            config_error,
            dirs,
            home_dir,
            env: Arc::new(|_| None),
        }
    }

    /// The resolved directory paths.
    pub fn dirs(&self) -> &crate::dirs::SymposiumDirs {
        &self.dirs
    }

    /// The cargo binary override, if set via `SYMPOSIUM_CARGO`.
    pub fn cargo_override(&self) -> Option<&Path> {
        self.dirs.cargo_override.as_deref()
    }

    /// Create a `WorkspaceDeps` with disk caching enabled, shareable as an
    /// [`Arc`] (a [`CargoPm`](crate::pm::CargoPm) holds one).
    pub fn workspace_deps(&self, cwd: &Path) -> Arc<crate::pm::WorkspaceDeps> {
        Arc::new(self.dirs.workspace_deps(cwd))
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

    /// The active package-manager set: the ecosystem transports (cargo, and
    /// each `[[package-manager]]`), then one instance per configured registry
    /// ([`registry_instances`](Self::registry_instances)), named for its
    /// registry, since that name is what its plugins are attributed to.
    ///
    /// `cwd` is the directory a spawned PM is told it answers for: `None` for
    /// a workspace-independent operation, so it does not go looking for one.
    pub fn package_managers(
        &self,
        workspace: &Arc<crate::pm::WorkspaceDeps>,
        cwd: Option<&Path>,
    ) -> crate::pm::PmRegistry {
        // The ecosystem transports, over dependencies, so not trust roots; then
        // one registry instance per configured registry, trust roots. A
        // `[[package-manager]]` named `cargo` replaces the built-in in-process
        // instance, which is how the cargo PM is run out of process.
        let configured = &self.config.package_managers;
        let builtin_cargo =
            (!configured.iter().any(|pm| pm.name == crate::pm::CARGO_PM)).then(|| {
                crate::pm::PmInstance::new(
                    crate::pm::CARGO_PM,
                    false,
                    crate::pm::PluginKind::Package,
                    Box::new(crate::pm::CargoPm::new(Arc::clone(workspace))),
                )
            });
        let instances = builtin_cargo
            .into_iter()
            .chain(
                configured
                    .iter()
                    .map(|pm| self.configured_pm_instance(pm, cwd)),
            )
            .chain(self.registry_instances());
        crate::pm::PmRegistry::new(instances.collect())
    }

    /// A `[[package-manager]]` entry as a spawned instance.
    ///
    /// The process is not started here: [`RemotePm`](crate::pm::RemotePm)
    /// spawns lazily, so a configured PM that nothing asks about costs nothing.
    fn configured_pm_instance(
        &self,
        pm: &PackageManagerConfig,
        cwd: Option<&Path>,
    ) -> crate::pm::PmInstance {
        let mut command = crate::pm::RemotePmCommand::new(&pm.command)
            .workspace(cwd.map(Path::to_path_buf))
            .cache_dir(self.dirs.cache_dir.join("pm").join(&pm.name));
        for arg in &pm.args {
            command = command.arg(arg);
        }
        // The cargo binary override is the test harness's channel to a fake
        // cargo. It comes from Symposium's directories rather than its process
        // environment, so inheriting the environment would not carry it.
        if let Some(cargo) = &self.dirs.cargo_override {
            command = command.env("SYMPOSIUM_CARGO", cargo.display().to_string());
        }

        crate::pm::PmInstance::new(
            &pm.name,
            false,
            crate::pm::PluginKind::Package,
            Box::new(crate::pm::RemotePm::new(&pm.name, command)),
        )
    }

    /// The package managers for a workspace-independent operation (registry
    /// listing, crates.io search): the cargo transport is built over a detached
    /// resolver that never runs `cargo metadata`.
    pub fn detached_managers(&self) -> crate::pm::PmRegistry {
        self.package_managers(&Arc::new(crate::pm::WorkspaceDeps::detached()), None)
    }

    /// The workspace context for `cwd`: the package managers plus where the
    /// workspace is and what it depends on, resolved lazily and cached.
    ///
    /// This is the single construction point, so one invocation shares one PM
    /// set, which is what keeps an out-of-process PM to one process per
    /// invocation rather than one per call site.
    pub fn workspace(&self, dir: &Path) -> Arc<crate::pm::Workspace> {
        Arc::new(crate::pm::Workspace::new(
            Some(dir.to_path_buf()),
            self.package_managers(&self.workspace_deps(dir), Some(dir)),
        ))
    }

    /// Override the cargo binary path (test-only).
    #[doc(hidden)]
    pub fn set_cargo_override(&mut self, path: PathBuf) {
        self.dirs.cargo_override = Some(path);
    }

    #[doc(hidden)]
    pub fn set_env(
        &mut self,
        env: impl Fn(&str) -> Option<std::ffi::OsString> + Send + Sync + 'static,
    ) {
        self.env = Arc::new(env);
    }

    pub(crate) fn env_dir(&self, var: &str) -> Option<PathBuf> {
        (self.env)(var)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
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

        let level = self.log_level();
        let file_filter = EnvFilter::new(level.as_str());

        let file_layer = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .ok()
            .map(|file| {
                tracing_subscriber::fmt::layer()
                    .with_writer(file)
                    .with_ansi(false)
                    .with_filter(file_filter)
            });

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

    /// The registry package-manager instances, in effect order: the builtin
    /// recommendations repo, the builtin `user-plugins` directory, then the
    /// configured `[[registry]]` entries. Each is a trust root — a git entry is
    /// a [`GitPm`](crate::pm::GitPm), a path entry a [`PathPm`](crate::pm::PathPm)
    /// — so refreshing (pulling git content) is the PM's own concern rather than
    /// a separate step. A registry whose source can't be resolved (a malformed
    /// git URL, or an entry naming neither `git` nor `path`) is skipped with a
    /// warning.
    pub fn registry_instances(&self) -> Vec<crate::pm::PmInstance> {
        let mut configs: Vec<RegistryConfig> = Vec::new();

        if self.config.defaults.symposium_recommendations {
            configs.push(RegistryConfig {
                name: "symposium-recommendations".to_string(),
                git: Some(BUILTIN_RECOMMENDATIONS_URL.to_string()),
                path: None,
                auto_update: true,
            });
        }

        if self.config.defaults.user_plugins
            && fs::symlink_metadata(self.dirs.config_dir.join("plugins")).is_ok()
        {
            configs.push(RegistryConfig {
                name: "user-plugins".to_string(),
                git: None,
                path: Some("plugins".to_string()),
                auto_update: true,
            });
        }

        configs.extend(self.config.registries.iter().cloned());
        configs
            .into_iter()
            .filter_map(|cfg| self.registry_instance(cfg))
            .collect()
    }

    /// Build the registry instance for one config entry. Relative `path` values
    /// resolve against the config dir.
    fn registry_instance(&self, cfg: RegistryConfig) -> Option<crate::pm::PmInstance> {
        let name = cfg.name.clone();
        let (pm, source): (Box<dyn crate::pm::PackageManager>, _) = if let Some(url) = &cfg.git {
            match crate::pm::GitPm::new(name.clone(), url.clone(), self.install_context()) {
                Some(git) => {
                    let source = git.source();
                    (Box::new(git), source)
                }
                None => {
                    tracing::warn!(registry = %name, url = %url, "bad registry URL");
                    return None;
                }
            }
        } else if let Some(path) = &cfg.path {
            let p = PathBuf::from(path);
            let dir = if p.is_absolute() {
                p
            } else {
                self.dirs.config_dir.join(p)
            };
            let path = crate::pm::PathPm::new(name.clone(), dir);
            let source = path.source();
            (Box::new(path), source)
        } else {
            tracing::warn!(registry = %name, "registry names neither git nor path");
            return None;
        };
        let mut instance =
            crate::pm::PmInstance::new(name, true, crate::pm::PluginKind::Registry, pm)
                .with_source(source);
        instance.auto_update = cfg.auto_update;
        Some(instance)
    }

    pub fn config_error(&self) -> Option<&str> {
        self.config_error.as_deref()
    }

    /// Write the user config to disk.
    pub fn save_config(&self) -> anyhow::Result<()> {
        let path = self.dirs.config_dir.join("config.toml");
        if let Some(error) = &self.config_error {
            anyhow::bail!(
                "not overwriting {}: it could not be read; fix or remove it, then run the command again\n{error}",
                path.display()
            );
        }
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
    if let Some(home) = crate::dirs::env_dir("SYMPOSIUM_HOME") {
        return home.join("logs");
    }
    if let Some(xdg) = crate::dirs::xdg_dir("XDG_STATE_HOME") {
        return xdg.join("symposium").join("logs");
    }
    config_dir.join("logs")
}

/// Load config from a config directory.
fn load_config_from(config_dir: &Path) -> (Config, Option<String>) {
    let path = config_dir.join("config.toml");
    let error = match fs::read_to_string(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Config::default(), None),
        Err(e) => e.to_string(),
        Ok(contents) => match toml::from_str::<RawConfig>(&contents) {
            Ok(raw) => match raw.validate() {
                Ok(config) => return (config, None),
                Err(e) => e,
            },
            Err(e) => e.to_string(),
        },
    };
    eprintln!(
        "warning: failed to read {}, so this run uses the defaults with auto-update off and will not change the file\n{error}",
        path.display()
    );
    let config = Config {
        auto_update: AutoUpdate::Off,
        ..Config::default()
    };
    (config, Some(error))
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
        toml::from_str::<RawConfig>(toml)
            .unwrap()
            .validate()
            .unwrap()
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

    #[test]
    fn plugin_source_table_does_not_break_registry() {
        let config = parse_config(indoc! {r#"
            [[plugin-source]]
            name = "old"
            path = "old"

            [[registry]]
            name = "new"
            path = "new"
        "#});
        let names: Vec<_> = config.registries.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["new"]);
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
    fn unparseable_config_is_never_saved() {
        for contents in [
            indoc! {r#"
                auto-update = "off"

                [[agent]]
                name = "claude"

                [plugins]
                auto_enable = ["x"]
            "#},
            indoc! {r#"
                auto-update = false

                [[agent]]
                name = "claude"
            "#},
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("config.toml");
            std::fs::write(&path, contents).unwrap();
            let sym = Symposium::from_dir(tmp.path());
            assert_eq!(sym.config.auto_update, AutoUpdate::Off);
            assert!(sym.save_config().is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), contents);
        }
    }

    #[test]
    fn unreadable_config_is_never_saved() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, b"\xff\xfe").unwrap();
        let sym = Symposium::from_dir(tmp.path());
        assert_eq!(sym.config.auto_update, AutoUpdate::Off);
        assert!(sym.save_config().is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"\xff\xfe");
    }

    #[test]
    fn missing_config_is_saved() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(tmp.path());
        sym.save_config().unwrap();
        assert!(tmp.path().join("config.toml").is_file());
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
    fn empty_agent_and_registry_lists_are_not_serialized() {
        let serialized = toml::to_string_pretty(&Config::default()).unwrap();
        assert!(!serialized.contains("agent ="), "{serialized}");
        assert!(!serialized.contains("registry ="), "{serialized}");
    }

    #[test]
    fn saved_config_accepts_appended_tables() {
        let tmp = tempfile::tempdir().unwrap();
        Symposium::from_dir(tmp.path()).save_config().unwrap();
        let path = tmp.path().join("config.toml");
        let mut contents = std::fs::read_to_string(&path).unwrap();
        contents.push_str(indoc! {r#"

            [[agent]]
            name = "claude"

            [[registry]]
            name = "local"
            path = "p"
        "#});
        std::fs::write(&path, contents).unwrap();

        let sym = Symposium::from_dir(tmp.path());
        let agents: Vec<_> = sym.config.agents.iter().map(|a| a.name.as_str()).collect();
        let registries: Vec<_> = sym
            .config
            .registries
            .iter()
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(agents, ["claude"]);
        assert_eq!(registries, ["local"]);
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

        let cargo = |n: &str| crate::pm::PackageId::any_version(crate::pm::CARGO_PM, n);
        let names =
            |refs: Vec<&PluginRef>| refs.into_iter().map(|r| r.name.clone()).collect::<Vec<_>>();
        assert!(
            config
                .plugins
                .is_auto_enabled(&cargo("widget-lib"), &[], &Spellings::default())
        );
        assert!(
            !config
                .plugins
                .is_auto_enabled(&cargo("other"), &[], &Spellings::default())
        );
        assert!(
            config
                .plugins
                .is_disabled(&cargo("noisy-crate"), &Spellings::default())
        );

        assert_eq!(
            config.plugins.used,
            vec![
                UseEntry::global(PluginRef::bare("everywhere")),
                UseEntry::scoped(PluginRef::bare("scoped"), PathBuf::from("/ws/a")),
            ]
        );
        assert_eq!(
            names(config.plugins.used_in(Path::new("/ws/a"))),
            vec!["everywhere", "scoped"]
        );
        assert_eq!(
            names(config.plugins.used_in(Path::new("/ws/other"))),
            vec!["everywhere"]
        );

        // Entries survive a round trip through the config file.
        let reparsed = parse_config(&toml::to_string_pretty(&config).unwrap());
        assert_eq!(reparsed.plugins, config.plugins);
    }

    /// A bare `disable` entry turns off every plugin of its name, whichever
    /// package manager offers it: turning off too much is the safe way to be
    /// wrong. A qualified one turns off only its own.
    #[test]
    fn a_bare_disable_names_every_plugin_of_its_name() {
        let config = parse_config(indoc! {r#"
            [plugins]
            disable = ["noisy", { pm = "npm", name = "quiet" }]
        "#});
        let id = crate::pm::PackageId::any_version;
        assert!(
            config
                .plugins
                .is_disabled(&id(crate::pm::CARGO_PM, "noisy"), &Spellings::default())
        );
        assert!(config.plugins.is_disabled(
            &id("symposium-recommendations", "noisy"),
            &Spellings::default()
        ));
        assert!(
            config
                .plugins
                .is_disabled(&id("npm", "quiet"), &Spellings::default())
        );
        assert!(
            !config
                .plugins
                .is_disabled(&id(crate::pm::CARGO_PM, "quiet"), &Spellings::default())
        );
    }

    /// A bare `auto-enable` entry consents to the dependency of its name, and
    /// to nothing while dependencies from two package managers share it, so
    /// consent to one ecosystem's package never silently covers another's.
    #[test]
    fn a_bare_auto_enable_is_resolved_against_the_dependencies() {
        let config = parse_config(indoc! {r#"
            [plugins]
            auto-enable = ["widget", { pm = "npm", name = "left-pad" }]
        "#});
        let id = crate::pm::PackageId::any_version;
        let cargo_widget = id(crate::pm::CARGO_PM, "widget");
        let npm_widget = id("npm", "widget");

        let deps = [cargo_widget.clone()];
        assert!(
            config
                .plugins
                .is_auto_enabled(&cargo_widget, &deps, &Spellings::default())
        );

        let deps = [cargo_widget.clone(), npm_widget.clone()];
        assert!(
            !config
                .plugins
                .is_auto_enabled(&cargo_widget, &deps, &Spellings::default())
        );
        assert!(
            !config
                .plugins
                .is_auto_enabled(&npm_widget, &deps, &Spellings::default())
        );

        let deps = [id("npm", "left-pad"), id(crate::pm::CARGO_PM, "left-pad")];
        assert!(
            config
                .plugins
                .is_auto_enabled(&deps[0], &deps, &Spellings::default())
        );
        assert!(
            !config
                .plugins
                .is_auto_enabled(&deps[1], &deps, &Spellings::default())
        );
    }

    /// Each entry is written as it was read: a bare name stays bare, in the
    /// shapes earlier releases read (a string, or `{ name, workspace }` when
    /// scoped), and a qualified one keeps its `pm`.
    #[test]
    fn entries_are_written_as_they_were_read() {
        let config = parse_config(indoc! {r#"
            [plugins]
            auto-enable = ["widget-lib", { pm = "npm", name = "left-pad" }]
            disable = ["noisy-crate", { pm = "cargo", name = "quiet-crate" }]
            use = [
                "everywhere",
                { name = "scoped", workspace = "/ws/a" },
                { pm = "npm", name = "is-even" },
            ]
        "#});
        let written: toml::Table =
            toml::from_str(&toml::to_string_pretty(&config).unwrap()).unwrap();
        let plugins = &written["plugins"];
        let table =
            |s: &str| toml::from_str::<toml::Value>(&format!("x = {s}")).unwrap()["x"].clone();
        assert_eq!(
            plugins["auto-enable"],
            toml::Value::Array(vec![
                "widget-lib".into(),
                table(r#"{ pm = "npm", name = "left-pad" }"#),
            ])
        );
        assert_eq!(
            plugins["disable"],
            toml::Value::Array(vec![
                "noisy-crate".into(),
                table(r#"{ pm = "cargo", name = "quiet-crate" }"#),
            ])
        );
        let used = plugins["use"].as_array().unwrap();
        assert_eq!(used[0], "everywhere".into());
        assert_eq!(
            used[1],
            table(r#"{ name = "scoped", workspace = "/ws/a" }"#)
        );
        assert_eq!(used[2]["pm"], "npm".into());

        assert_eq!(
            parse_config(&toml::to_string_pretty(&config).unwrap()).plugins,
            config.plugins
        );
    }

    /// A mistyped key in a `[plugins]` table is an error, not dropped: a
    /// misspelled `workspace` would otherwise make a scoped entry global.
    #[test]
    fn an_unknown_key_in_a_plugins_table_is_an_error() {
        for toml in [
            "[plugins]\nuse = [{ name = \"x\", workspce = \"/a\" }]",
            "[plugins]\ndisable = [{ name = \"x\", workspace = \"/a\" }]",
        ] {
            assert!(toml::from_str::<RawConfig>(toml).is_err(), "{toml}");
        }
    }

    /// Leaving out `pm` in a table is the same as a bare string.
    #[test]
    fn a_table_without_pm_is_a_bare_name() {
        let config = parse_config(indoc! {r#"
            [plugins]
            disable = [{ name = "noisy-crate" }]
            use = [{ name = "everywhere" }]
        "#});
        assert_eq!(config.plugins.disable[0], PluginRef::bare("noisy-crate"));
        assert_eq!(
            config.plugins.used[0],
            UseEntry::global(PluginRef::bare("everywhere"))
        );
    }

    #[test]
    fn auto_enable_wildcard_matches_every_name() {
        let id = crate::pm::PackageId::any_version;
        let deps = [id(crate::pm::CARGO_PM, "widget"), id("npm", "widget")];
        // A wildcard is the same entry in either spelling, and survives a
        // round trip as itself.
        for spelling in [r#""*""#, r#"{ name = "*" }"#] {
            let config = parse_config(&format!("[plugins]\nauto-enable = [{spelling}]\n"));
            assert!(
                config
                    .plugins
                    .is_auto_enabled(&deps[0], &deps, &Spellings::default())
            );
            assert!(
                config
                    .plugins
                    .is_auto_enabled(&deps[1], &deps, &Spellings::default())
            );
            let reparsed = parse_config(&toml::to_string_pretty(&config).unwrap());
            assert_eq!(reparsed.plugins, config.plugins);
        }
        let config = parse_config("[plugins]\nauto-enable = [{ pm = \"npm\", name = \"*\" }]\n");
        assert!(
            config
                .plugins
                .is_auto_enabled(&deps[1], &deps, &Spellings::default())
        );
        assert!(
            !config
                .plugins
                .is_auto_enabled(&deps[0], &deps, &Spellings::default())
        );
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

    /// Ids route by name, so a configured package manager cannot take a name
    /// another package manager or a registry has, and the name must be a plain
    /// identifier. Such a config is an error, as an unparseable one is, rather
    /// than one entry silently shadowing another.
    #[test]
    fn a_package_manager_name_must_be_free_and_plain() {
        let check = |toml: &str| {
            toml::from_str::<RawConfig>(toml)
                .unwrap()
                .validate()
                .map(|_| ())
        };
        let pm = |name: &str| format!("[[package-manager]]\nname = {name:?}\ncommand = \"x\"\n");

        assert!(check(&format!("{}{}", pm("npm"), pm("pypi"))).is_ok());
        assert!(check(&pm("cargo")).is_ok(), "replaces the built-in cargo");
        assert!(check(&format!("{}{}", pm("npm"), pm("npm"))).is_err());
        assert!(check(&pm("user-plugins")).is_err());
        assert!(
            check(&format!(
                "[[registry]]\nname = \"mine\"\npath = \"mine\"\n{}",
                pm("mine")
            ))
            .is_err()
        );
        assert!(
            check(&format!(
                "[defaults]\nuser-plugins = false\n{}",
                pm("user-plugins")
            ))
            .is_ok()
        );
        for bad in ["", "../x", "/abs", "a:b"] {
            assert!(check(&pm(bad)).is_err(), "{bad:?}");
        }
        assert!(
            toml::from_str::<RawConfig>("[[package-manager]]\nname = \"npm\"\ncomand = \"x\"\n")
                .is_err(),
            "a misspelled key is an error, not ignored"
        );
    }
}
