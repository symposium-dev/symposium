use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::config::Symposium;
use crate::hook::HookEvent;
use crate::hook_schema::HookAgent;
use crate::installation::Source;

use sacp::schema::McpServer;

/// An MCP server entry in a plugin manifest.
pub type McpServerEntry = McpServer;

/// An MCP server entry with optional crate filtering.
///
/// When `crates` is present, the server is only registered if the workspace
/// matches those predicates (ANDed with plugin-level `crates`).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginMcpServer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crates: Option<crate::predicate::PredicateSet>,
    /// Shell predicates that must all pass for this server to be registered.
    /// ANDed with plugin-level `shell_predicates`.
    #[serde(
        default,
        skip_serializing_if = "crate::shell_predicate::ShellPredicateSet::is_empty"
    )]
    pub shell_predicates: crate::shell_predicate::ShellPredicateSet,
    #[serde(flatten)]
    pub server: McpServerEntry,
}

/// Controls how aggressively plugin sources are updated.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum UpdateLevel {
    /// Debounced: skip the API check if fetched recently.
    None,
    /// Always check freshness via API, but only download if stale.
    Check,
    /// Always re-download regardless of staleness.
    Fetch,
}

/// Source declaration for a skill group.
///
/// Accepts a table with at most one of `path`, `git`, or `crate_path` set,
/// or the shorthand string `source = "crate"` (equivalent to
/// `source.crate_path = ".symposium/skills"`).
///
/// The shorthand and explicit forms are *preserved* through parse/serialize
/// round-trips: `source = "crate"` deserializes to a `CratePath` with
/// `explicit_path: None` and serializes back as `"crate"`;
/// `source.crate_path = ".symposium/skills"` deserializes to
/// `explicit_path: Some(...)` and serializes back as the table form, even
/// though both resolve to the same on-disk path.
#[derive(Debug, Clone, Default)]
pub enum PluginSource {
    /// No source specified (skills discovered in the plugin directory itself).
    #[default]
    None,
    /// Local filesystem path, relative to the plugin manifest.
    Path(PathBuf),
    /// GitHub URL pointing to a directory in a repository.
    Git(String),
    /// Relative subpath inside a fetched crate's source tree.
    CratePath(CratePathSource),
}

/// Payload for [`PluginSource::CratePath`].
///
/// Captures whether the user wrote the shorthand (`source = "crate"`) or the
/// explicit form (`source.crate_path = "<p>"`) so that serialization can
/// reproduce the original input faithfully. Use [`CratePathSource::as_str`]
/// to get the effective subdirectory (with the default substituted for
/// shorthand).
#[derive(Debug, Clone, Default)]
pub struct CratePathSource {
    /// `None` captures the shorthand form (use the default subdir).
    /// `Some(p)` captures explicit `source.crate_path = "<p>"`.
    pub explicit_path: Option<String>,
}

impl CratePathSource {
    /// Default subdirectory used when the user writes `source = "crate"`.
    pub const DEFAULT_PATH: &'static str = ".symposium/skills";

    /// The shorthand form (`source = "crate"`) — no explicit path recorded.
    pub fn shorthand() -> Self {
        Self {
            explicit_path: None,
        }
    }

    /// The explicit form (`source.crate_path = "<p>"`).
    pub fn explicit(path: impl Into<String>) -> Self {
        Self {
            explicit_path: Some(path.into()),
        }
    }

    /// Resolved subpath: the explicit path if present, otherwise the default.
    pub fn as_str(&self) -> &str {
        self.explicit_path.as_deref().unwrap_or(Self::DEFAULT_PATH)
    }

    /// True if the user wrote the `source = "crate"` shorthand.
    pub fn is_shorthand(&self) -> bool {
        self.explicit_path.is_none()
    }
}

impl serde::Serialize for PluginSource {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self {
            PluginSource::None => serializer.serialize_map(Some(0))?.end(),
            PluginSource::Path(p) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("path", p)?;
                map.end()
            }
            PluginSource::Git(url) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("git", url)?;
                map.end()
            }
            // Shorthand form — user wrote `source = "crate"`.
            PluginSource::CratePath(CratePathSource {
                explicit_path: None,
            }) => serializer.serialize_str("crate"),
            // Explicit form — user wrote `source.crate_path = "<p>"`.
            PluginSource::CratePath(CratePathSource {
                explicit_path: Some(p),
            }) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("crate_path", p)?;
                map.end()
            }
        }
    }
}

impl<'de> serde::Deserialize<'de> for PluginSource {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de;

        /// Helper for table-form deserialization.
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct PluginSourceFields {
            #[serde(default)]
            path: Option<PathBuf>,
            #[serde(default)]
            git: Option<String>,
            #[serde(default)]
            crate_path: Option<String>,
        }

        struct PluginSourceVisitor;

        impl<'de> de::Visitor<'de> for PluginSourceVisitor {
            type Value = PluginSource;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str(r#""crate" or a table with path/git/crate_path"#)
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                match v {
                    "crate" => Ok(PluginSource::CratePath(CratePathSource::shorthand())),
                    other => Err(de::Error::custom(format!(
                        "unknown source shorthand \"{other}\"; only \"crate\" is supported"
                    ))),
                }
            }

            fn visit_map<A: de::MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
                let fields =
                    PluginSourceFields::deserialize(de::value::MapAccessDeserializer::new(map))?;
                let count = fields.path.is_some() as u8
                    + fields.git.is_some() as u8
                    + fields.crate_path.is_some() as u8;
                if count > 1 {
                    return Err(de::Error::custom(
                        "source.path, source.git, and source.crate_path are mutually exclusive",
                    ));
                }
                Ok(match (fields.path, fields.git, fields.crate_path) {
                    (Some(p), None, None) => PluginSource::Path(p),
                    (None, Some(url), None) => PluginSource::Git(url),
                    (None, None, Some(cp)) => {
                        PluginSource::CratePath(CratePathSource::explicit(cp))
                    }
                    (None, None, None) => PluginSource::None,
                    // Unreachable given the `count > 1` guard above.
                    _ => unreachable!("count > 1 guard"),
                })
            }
        }

        deserializer.deserialize_any(PluginSourceVisitor)
    }
}

/// A `[[skills]]` entry from a plugin manifest.
///
/// Each group declares which crates it advises on (`crates`) and
/// optionally a remote source for the skill files.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillGroup {
    /// Crate predicates this group advises on (e.g., `["serde", "serde_json>=1.0"]`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crates: Option<crate::predicate::PredicateSet>,
    /// Shell predicates that must all pass for this group's skills to install.
    /// ANDed with plugin-level `shell_predicates` and skill-level frontmatter.
    #[serde(
        default,
        skip_serializing_if = "crate::shell_predicate::ShellPredicateSet::is_empty"
    )]
    pub shell_predicates: crate::shell_predicate::ShellPredicateSet,
    /// Remote source for skills.
    #[serde(default)]
    pub source: PluginSource,
}

/// Raw command reference as it appears in TOML: a string (named installation
/// reference) or an inline installation table.
///
/// Inline forms are promoted at validation time into synthetic
/// `[[installations]]` entries, so the validated `Plugin` only ever stores
/// installation references as plain names.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawInstallationRef {
    Named(String),
    Inline(RawInlineInstallation),
}

/// Inline installation table. Carries the same fields as a
/// `[[installations]]` entry minus `name`.
#[derive(Debug, Deserialize)]
struct RawInlineInstallation {
    #[serde(default)]
    install_commands: Vec<String>,
    #[serde(default)]
    requirements: Vec<RawInstallationRef>,
    #[serde(flatten, default)]
    source: Option<Source>,
    #[serde(default)]
    executable: Option<String>,
    #[serde(default)]
    script: Option<String>,
    #[serde(default)]
    args: Vec<String>,
}

/// A `[[installations]]` entry in the validated `Plugin`.
///
/// Inline references on hooks and on other installations are promoted to
/// synthetic entries here, so this is the single source of truth: every
/// `Hook.command` and `Hook.requirements` / `Installation.requirements`
/// names a member of `Plugin.installations`.
///
/// Installations may be runnable (have `executable` or `script`), pure setup
/// (only `install_commands`), pure aggregators (only `requirements`), or any
/// combination. Whether an installation is *expected* to resolve to a runnable
/// is decided at the hook layer.
#[derive(Debug, Clone, Serialize)]
pub struct Installation {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requirements: Vec<String>,
    /// Shell commands run after the kind-specific install step completes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub install_commands: Vec<String>,
    /// How to acquire bits onto disk. `None` means no acquisition step —
    /// `executable` / `script` are taken as paths on disk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    /// Path to a binary to run. For `cargo`, the binary name in the install's
    /// `bin/` dir. For `github` / `binary`, a path inside the acquired tree.
    /// For `None` source, a path on disk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
    /// Path to a shell script. Same resolution rules as `executable`, but
    /// invoked as `sh <path> <args>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    /// Default invocation arguments. The hook may set its own; not both.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
}

/// A parsed plugin with its path and manifest.
#[derive(Debug, Clone)]
pub struct ParsedPlugin {
    /// The path from which the plugin was parsed.
    pub path: PathBuf,

    /// The parsed plugin manifest.
    pub plugin: Plugin,
}

/// A loaded, *validated* plugin manifest.
///
/// This is a table of contents — it describes what skills and hooks are
/// available, but does not load skill content. The skills layer handles
/// discovery and loading.
#[derive(Debug, Clone, Serialize)]
pub struct Plugin {
    pub name: String,
    /// Crate predicates this plugin applies to. `["*"]` for all crates.
    pub crates: crate::predicate::PredicateSet,
    /// Shell predicates that must all pass for this plugin to apply at all.
    /// Evaluated at sync time (for skills/MCP) and at hook dispatch time.
    #[serde(
        default,
        skip_serializing_if = "crate::shell_predicate::ShellPredicateSet::is_empty"
    )]
    pub shell_predicates: crate::shell_predicate::ShellPredicateSet,
    /// Named installation entries available to hooks in this plugin.
    /// Order matches declaration order in the manifest.
    pub installations: Vec<Installation>,
    pub hooks: Vec<Hook>,
    pub skills: Vec<SkillGroup>,
    /// MCP servers to register for this plugin.
    pub mcp_servers: Vec<PluginMcpServer>,
}

impl Plugin {
    /// Check if this plugin applies to the given workspace crates.
    /// Returns true if any predicate matches.
    pub fn applies_to_crates(&self, workspace_crates: &[(String, semver::Version)]) -> bool {
        self.crates.matches(workspace_crates)
    }

    /// Check if this plugin's `shell_predicates` all hold.
    /// Vacuously true when the list is empty.
    pub fn shell_predicates_hold(&self) -> bool {
        self.shell_predicates.evaluate()
    }

    /// Return MCP servers applicable to the given workspace crates.
    ///
    /// A server matches if its own `crates` predicates match (or are absent,
    /// meaning it inherits from the plugin level which is already checked)
    /// AND its own `shell_predicates` all hold.
    pub fn applicable_mcp_servers(
        &self,
        workspace_crates: &[(String, semver::Version)],
    ) -> Vec<McpServerEntry> {
        self.mcp_servers
            .iter()
            .filter(|s| {
                let Some(ref pred_set) = s.crates else {
                    return true;
                };
                pred_set.matches(workspace_crates) && s.shell_predicates.evaluate()
            })
            .map(|s| s.server.clone())
            .collect()
    }
}

/// A validated hook definition.
///
/// `command` is the name of an `Installation` in the plugin (possibly a
/// synthetic one promoted from an inline declaration). `executable` / `script`
/// / `args` may further specify the invocation when not pinned by the
/// installation.
#[derive(Debug, Clone, Serialize)]
pub struct Hook {
    pub name: String,
    pub event: HookEvent,
    pub agent: Option<HookAgent>,
    pub matcher: Option<String>,
    /// Installation names to acquire before the hook runs. Includes the
    /// command installation's own requirements (one level of expansion).
    pub requirements: Vec<String>,
    /// Name of the installation whose acquisition this hook drives.
    pub command: String,
    /// What to run from the installation. Validation guarantees that across
    /// (`executable`, `script`) on the hook AND on the installation, at most
    /// one is set.
    pub executable: Option<String>,
    pub script: Option<String>,
    /// Invocation arguments. Validation guarantees at most one of
    /// (hook `args`, installation `args`) is non-empty.
    pub args: Vec<String>,
    pub format: HookFormat,
    /// Shell predicates that must all pass for this hook to dispatch.
    /// Evaluated at dispatch time, ANDed with the plugin's predicates.
    #[serde(
        default,
        skip_serializing_if = "crate::shell_predicate::ShellPredicateSet::is_empty"
    )]
    pub shell_predicates: crate::shell_predicate::ShellPredicateSet,
}

/// Resolve a `RawInstallationRef`. If named, validate against the existing
/// installations and return the name. If inline, promote the inline body to
/// a new synthetic `Installation` (named via `synth_name`) appended to
/// `installations`, and return the synthetic name.
fn resolve_or_promote(
    raw: RawInstallationRef,
    installations: &mut Vec<Installation>,
    names: &mut std::collections::BTreeSet<String>,
    synth_name: &mut dyn FnMut() -> String,
    ctx: &str,
) -> Result<String> {
    match raw {
        RawInstallationRef::Named(name) => {
            if !names.contains(&name) {
                bail!("{ctx} references unknown installation `{name}`");
            }
            Ok(name)
        }
        RawInstallationRef::Inline(inline) => {
            let name = synth_name();
            if !names.insert(name.clone()) {
                bail!("{ctx}: synthetic installation name `{name}` conflicts with an existing one");
            }
            let RawInlineInstallation {
                install_commands,
                requirements: raw_reqs,
                source,
                executable,
                script,
                args,
            } = inline;
            // Promoted inline requirements get the same treatment as named-installation
            // requirements: synthesized via `<name>__req_<i>`.
            let mut reqs = Vec::with_capacity(raw_reqs.len());
            for (i, r) in raw_reqs.into_iter().enumerate() {
                let req = resolve_or_promote(
                    r,
                    installations,
                    names,
                    &mut || format!("{name}__req_{i}"),
                    &format!("{ctx} requirement[{i}]"),
                )?;
                reqs.push(req);
            }
            let install = Installation {
                name: name.clone(),
                requirements: reqs,
                install_commands,
                source,
                executable,
                script,
                args,
            };
            validate_installation(&install)?;
            installations.push(install);
            Ok(name)
        }
    }
}

/// Validate a raw hook into a `Hook`, promoting any inline `command` /
/// `requirements` into synthetic entries on `installations`.
fn validate_hook(
    raw: RawHook,
    installations: &mut Vec<Installation>,
    names: &mut std::collections::BTreeSet<String>,
) -> Result<Hook> {
    let RawHook {
        name: hook_name,
        event,
        agent,
        matcher,
        requirements: raw_requirements,
        command: raw_command,
        executable: hook_executable,
        script: hook_script,
        args: hook_args,
        format,
        shell_predicates,
    } = raw;

    let command = resolve_or_promote(
        raw_command,
        installations,
        names,
        &mut || hook_name.clone(),
        &format!("hook `{hook_name}`"),
    )?;

    let install = installations
        .iter()
        .find(|i| i.name == command)
        .cloned()
        .expect("just resolved");

    // Across (hook.exec, hook.script, install.exec, install.script): at most
    // one is set. The user's rule is "exactly one of executable/script" globally.
    let mut runnables: Vec<&str> = Vec::new();
    if install.executable.is_some() {
        runnables.push("installation `executable`");
    }
    if install.script.is_some() {
        runnables.push("installation `script`");
    }
    if hook_executable.is_some() {
        runnables.push("hook `executable`");
    }
    if hook_script.is_some() {
        runnables.push("hook `script`");
    }
    if runnables.len() > 1 {
        bail!(
            "hook `{hook_name}`: at most one of `executable` / `script` may be set across \
             hook and installation, but {} are set",
            runnables.join(", ")
        );
    }

    // The hook must end up runnable. Cargo can infer a single binary at
    // acquisition time, so it's allowed to omit the runnable; every other
    // case requires `executable` or `script` somewhere.
    if runnables.is_empty() {
        let cargo_inferable = matches!(install.source, Some(Source::Cargo(_)));
        if !cargo_inferable {
            bail!(
                "hook `{hook_name}`: command `{}` has no `executable` or `script` and the hook \
                 supplies none either — nothing to run",
                install.name
            );
        }
    }

    // Args: at most one of hook.args / install.args is non-empty.
    let final_args = match (install.args.is_empty(), hook_args.is_empty()) {
        (false, false) => bail!(
            "hook `{hook_name}`: `args` is set on both the installation and the hook; \
             remove it from one"
        ),
        (true, _) => hook_args,
        (false, true) => install.args.clone(),
    };

    let mut final_requirements: Vec<String> = install.requirements.clone();
    for (i, raw_req) in raw_requirements.into_iter().enumerate() {
        let req = resolve_or_promote(
            raw_req,
            installations,
            names,
            &mut || format!("{hook_name}__req_{i}"),
            &format!("hook `{hook_name}` requirement[{i}]"),
        )?;
        if let Some(entry) = installations.iter().find(|i| i.name == req) {
            final_requirements.extend(entry.requirements.iter().cloned());
        }
        final_requirements.push(req);
    }

    Ok(Hook {
        name: hook_name,
        event,
        agent,
        matcher,
        requirements: final_requirements,
        command,
        executable: hook_executable,
        script: hook_script,
        args: final_args,
        format,
        shell_predicates,
    })
}

/// Validate semantic constraints on an installation that serde alone cannot
/// express:
/// - `executable` and `script` are mutually exclusive on a single layer.
/// - cargo + `git` requires an explicit `executable`, since we can't query
///   crates.io to infer one.
fn validate_installation(install: &Installation) -> Result<()> {
    if install.executable.is_some() && install.script.is_some() {
        bail!(
            "installation `{}`: `executable` and `script` are mutually exclusive — \
             pick one",
            install.name
        );
    }
    if let Some(Source::Cargo(c)) = &install.source {
        if c.git.is_some() && install.executable.is_none() {
            bail!(
                "installation `{}`: cargo source with `git` requires `executable` to be set \
                 (crates.io is not consulted, so the binary name is unknown)",
                install.name
            );
        }
        if c.global && install.executable.is_none() {
            bail!(
                "installation `{}`: cargo source with `global = true` requires `executable` to \
                 be set (the binary is spawned by name via `$PATH` lookup, so we don't infer \
                 it from crates.io)",
                install.name
            );
        }
    }
    Ok(())
}

/// The wire format a plugin hook expects for input/output.
///
/// This is distinct from `HookAgent` because:
/// - `Symposium` is a wire format but not an agent (no CLI invokes hooks
///   in symposium format natively).
/// - Not all agents have hook wire formats (e.g., Goose uses MCP extensions,
///   OpenCode uses JS plugins), so only agents with shell-hook JSON formats
///   appear here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HookFormat {
    /// Symposium canonical format (default).
    #[default]
    Symposium,
    /// A specific agent's wire format.
    Claude,
    Codex,
    Copilot,
    Gemini,
    Kiro,
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
    /// Non-fatal load warnings for plugins or standalone skills that were skipped.
    pub warnings: Vec<LoadWarning>,
}

/// A non-fatal plugin source load failure.
#[derive(Debug, Clone)]
pub struct LoadWarning {
    /// Path to the plugin or skill that was skipped.
    pub path: PathBuf,
    /// Human-readable error message.
    pub message: String,
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
#[serde(deny_unknown_fields)]
struct RawPluginManifest {
    name: String,
    crates: crate::predicate::PredicateSet,
    #[serde(default)]
    shell_predicates: crate::shell_predicate::ShellPredicateSet,
    #[serde(default)]
    installations: Vec<RawNamedInstallation>,
    #[serde(default)]
    hooks: Vec<RawHook>,
    #[serde(default)]
    skills: Vec<SkillGroup>,
    #[serde(default)]
    mcp_servers: Vec<PluginMcpServer>,
}

/// `[[installations]]` entry: a name plus the same fields as a `RawInlineInstallation`.
#[derive(Debug, Deserialize)]
struct RawNamedInstallation {
    name: String,
    #[serde(default)]
    requirements: Vec<RawInstallationRef>,
    #[serde(default)]
    install_commands: Vec<String>,
    #[serde(flatten, default)]
    source: Option<Source>,
    #[serde(default)]
    executable: Option<String>,
    #[serde(default)]
    script: Option<String>,
    #[serde(default)]
    args: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawHook {
    name: String,
    event: HookEvent,
    #[serde(default)]
    agent: Option<HookAgent>,
    #[serde(default)]
    matcher: Option<String>,
    #[serde(default)]
    requirements: Vec<RawInstallationRef>,
    /// Named installation (`"my-install"`) or inline installation table.
    command: RawInstallationRef,
    /// What to run from the installation. Across hook + installation, at most
    /// one of `executable` / `script` may be set.
    #[serde(default)]
    executable: Option<String>,
    #[serde(default)]
    script: Option<String>,
    /// Invocation arguments. Forbidden when the installation also declares `args`.
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    format: HookFormat,
    #[serde(default)]
    shell_predicates: crate::shell_predicate::ShellPredicateSet,
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
        if let Some(ref path) = source_path
            && let Ok(contents) = scan_source_dir(path)
        {
            for parsed_plugin in contents.plugins.into_iter().flatten() {
                if parsed_plugin.plugin.name == name {
                    return Some(parsed_plugin);
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
        match crate::installation::git::parse_github_url(git_url) {
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
    use crate::installation::git;

    let source = git::parse_github_url(git_url)?;
    let cache_mgr = git::GitCacheManager::new(sym, "plugin-sources");
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
    let mut warnings = Vec::new();

    for dir in resolve_plugin_source_dirs(sym, &sources) {
        match scan_source_dir(&dir) {
            Ok(contents) => {
                for result in contents.plugins {
                    match result {
                        Ok(p) => plugins.push(p),
                        Err(e) => {
                            tracing::warn!(error = %e, "failed to load plugin");
                            warnings.push(LoadWarning {
                                path: dir.join("<unknown>.toml"),
                                message: format!("failed to load plugin: {e}"),
                            });
                        }
                    }
                }
                for skill_md in contents.skill_files {
                    match crate::skills::load_standalone_skill(&skill_md) {
                        Ok(skill) => standalone_skills.push(skill),
                        Err(e) => {
                            tracing::warn!(
                                path = %skill_md.display(),
                                error = %e,
                                "failed to load standalone skill"
                            );
                            warnings.push(LoadWarning {
                                path: skill_md,
                                message: format!("failed to load standalone skill: {e}"),
                            });
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!(dir = %dir.display(), error = %e, "failed to scan plugin source dir");
                warnings.push(LoadWarning {
                    path: dir,
                    message: format!("failed to scan plugin source dir: {e}"),
                });
            }
        }
    }

    tracing::debug!(
        plugins = plugins.len(),
        standalone_skills = standalone_skills.len(),
        "plugin registry loaded"
    );

    PluginRegistry {
        plugins,
        standalone_skills,
        warnings,
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
    if let Some(dir_type) = discover_directory_type(dir)? {
        match dir_type {
            DirectoryType::Plugin(_) => anyhow::bail!(
                "plugin source root contains SYMPOSIUM.toml — it should contain subdirectories with plugins, not be a plugin itself: {}",
                dir.display()
            ),
            DirectoryType::Skill(_) => anyhow::bail!(
                "plugin source root contains SKILL.md — it should contain subdirectories with skills, not be a skill itself: {}",
                dir.display()
            ),
        }
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
    /// Optional warning (non-fatal).
    pub warning: Option<String>,
    /// Child results (e.g., skills belonging to a plugin).
    pub children: Vec<ValidationResult>,
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
    let mut plugin_skill_dirs: Vec<PathBuf> = Vec::new();

    for plugin_result in contents.plugins {
        let (path, plugin, result) = match plugin_result {
            Ok(parsed) => (parsed.path.clone(), Some(parsed), Ok(())),
            Err(e) => {
                let path = dir.join("<unknown>.toml");
                (path, None, Err(e))
            }
        };

        let mut children = Vec::new();

        // Validate that local skill groups contain discoverable skills.
        if let Some(parsed) = &plugin {
            let plugin_dir = parsed.path.parent().unwrap_or(dir);
            for group in &parsed.plugin.skills {
                if let PluginSource::Path(ref rel_path) = group.source {
                    let joined = plugin_dir.join(rel_path);
                    let skills_dir: PathBuf = joined.components().collect();
                    plugin_skill_dirs.push(skills_dir.clone());
                    let found = crate::skills::discover_skills(&skills_dir, group);
                    if found.is_empty() {
                        children.push(ValidationResult {
                            path: skills_dir,
                            kind: ValidationKind::Skill,
                            result: Ok(()),
                            warning: Some(
                                "skill group source.path contains no SKILL.md files".into(),
                            ),
                            children: Vec::new(),
                        });
                    } else {
                        for skill_result in found {
                            let (skill_path, result) = match skill_result {
                                Ok(skill) => (skill.path.clone(), Ok(())),
                                Err(e) => (skills_dir.join("SKILL.md"), Err(e)),
                            };
                            children.push(ValidationResult {
                                path: skill_path,
                                kind: ValidationKind::Skill,
                                result,
                                warning: None,
                                children: Vec::new(),
                            });
                        }
                    }
                }
            }
        }

        results.push(ValidationResult {
            path: path.clone(),
            kind: ValidationKind::Plugin,
            result,
            warning: None,
            children,
        });
    }

    for skill_md in contents.skill_files {
        // Skip skills already validated as part of a plugin group.
        if plugin_skill_dirs.iter().any(|d| skill_md.starts_with(d)) {
            continue;
        }
        let result = crate::skills::load_standalone_skill(&skill_md).map(|_| ());
        results.push(ValidationResult {
            path: skill_md,
            kind: ValidationKind::Skill,
            result,
            warning: None,
            children: Vec::new(),
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
        for pred in &plugin_result.plugin.crates.predicates {
            pred.collect_crate_names(&mut names);
        }
        for group in &plugin_result.plugin.skills {
            if let Some(pred_set) = &group.crates {
                for pred in &pred_set.predicates {
                    pred.collect_crate_names(&mut names);
                }
            }
        }
        for mcp in &plugin_result.plugin.mcp_servers {
            if let Some(pred_set) = &mcp.crates {
                for pred in &pred_set.predicates {
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

/// Load and validate a single plugin from a TOML manifest.
pub fn load_plugin(manifest_path: &Path) -> Result<ParsedPlugin> {
    let content = fs::read_to_string(manifest_path)?;
    let manifest: RawPluginManifest = toml::from_str(&content)?;
    let plugin = validate_manifest(manifest)
        .with_context(|| format!("validating `{}`", manifest_path.display()))?;
    Ok(ParsedPlugin {
        path: manifest_path.to_path_buf(),
        plugin,
    })
}

/// Convert a raw manifest into a validated `Plugin`.
///
/// User-declared `[[installations]]` come first in the resulting list, in
/// declaration order. Inline references on installations and hooks are
/// promoted into synthetic entries appended to the same list so that every
/// validated reference is a plain name.
fn validate_manifest(manifest: RawPluginManifest) -> Result<Plugin> {
    let mut names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for entry in &manifest.installations {
        if !names.insert(entry.name.clone()) {
            bail!("duplicate installation name `{}`", entry.name);
        }
    }

    let mut installations: Vec<Installation> = Vec::with_capacity(manifest.installations.len());
    for raw in manifest.installations {
        let RawNamedInstallation {
            name,
            requirements: raw_reqs,
            install_commands,
            source,
            executable,
            script,
            args,
        } = raw;
        // Pre-register the entry so synthesized requirement names can use
        // `<name>__req_<i>` without colliding with the entry itself.
        installations.push(Installation {
            name: name.clone(),
            requirements: Vec::new(),
            install_commands,
            source,
            executable,
            script,
            args,
        });
        let idx = installations
            .iter()
            .position(|i| i.name == name)
            .expect("just pushed");
        validate_installation(&installations[idx])?;
        let mut reqs = Vec::with_capacity(raw_reqs.len());
        for (i, r) in raw_reqs.into_iter().enumerate() {
            let req = resolve_or_promote(
                r,
                &mut installations,
                &mut names,
                &mut || format!("{name}__req_{i}"),
                &format!("installation `{name}` requirement[{i}]"),
            )?;
            reqs.push(req);
        }
        let idx = installations
            .iter()
            .position(|i| i.name == name)
            .expect("just pushed");
        installations[idx].requirements = reqs;
    }

    let mut hooks = Vec::with_capacity(manifest.hooks.len());
    for raw in manifest.hooks {
        hooks.push(validate_hook(raw, &mut installations, &mut names)?);
    }

    validate_skill_groups(&manifest.crates, &manifest.skills)?;

    Ok(Plugin {
        name: manifest.name,
        crates: manifest.crates,
        shell_predicates: manifest.shell_predicates,
        installations,
        hooks,
        skills: manifest.skills,
        mcp_servers: manifest.mcp_servers,
    })
}

/// Validate skill-group source constraints that serde alone cannot express.
///
/// - At parse time we enforce: if a group uses `source.crate_path` (including
///   the `source = "crate"` shorthand), at least one non-wildcard predicate
///   must be reachable from the group — either on the plugin itself or on the
///   group's own `crates` — so that Symposium can resolve concrete crates to
///   fetch.
///
/// Valid:
///   crates = ["serde"]             + source = "crate"         → fetch serde
///   crates = ["*"], group ["serde"] + source.crate_path = …   → fetch serde
///   crates = ["*", "serde"]        + source.crate_path = …    → fetch serde
///
/// Invalid:
///   crates = ["*"]                 + source = "crate"         → no concrete crate
///   crates = ["*"], group ["*"]    + source.crate_path = …    → no concrete crate
fn validate_skill_groups(
    plugin_crates: &crate::predicate::PredicateSet,
    skills: &[SkillGroup],
) -> Result<()> {
    for (i, group) in skills.iter().enumerate() {
        if matches!(group.source, PluginSource::CratePath(_)) {
            let has_non_wildcard = plugin_crates
                .predicates
                .iter()
                .chain(
                    group
                        .crates
                        .as_ref()
                        .map(|ps| ps.predicates.as_slice())
                        .unwrap_or_default(),
                )
                .any(|p| !matches!(p, crate::predicate::Predicate::Wildcard));
            if !has_non_wildcard {
                bail!(
                    "skills group {i} uses source.crate_path but all predicates \
                     (plugin-level and group-level) are wildcards or absent — \
                     at least one non-wildcard predicate is required to resolve concrete crates"
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;

    use crate::predicate::PredicateSet;

    fn pred_set(s: &str) -> PredicateSet {
        PredicateSet::parse(s).unwrap()
    }

    fn from_str(s: &str) -> Result<Plugin> {
        let manifest: RawPluginManifest = toml::from_str(s)?;
        validate_manifest(manifest)
    }

    const SAMPLE: &str = indoc! {r#"
        name = "example-plugin"
        crates = ["*"]

        [[installations]]
        name = "tool"
        source = "cargo"
        crate = "example-tool"

        [[hooks]]
        name = "test"
        event = "PreToolUse"
        command = "tool"
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
            crates = ["serde"]

            [[skills]]
            crates = ["serde"]
            source.git = "https://github.com/org/repo/tree/main/serde"
        "#};
        let plugin = from_str(toml).expect("parse");
        assert_eq!(plugin.name, "remote-plugin");
        assert_eq!(plugin.skills.len(), 1);
        let group = &plugin.skills[0];
        let cr = group.crates.as_ref().unwrap();
        assert_eq!(cr.predicates.len(), 1);
        assert!(cr.predicates[0].references_crate("serde"));
        assert!(
            matches!(
                &group.source,
                PluginSource::Git(url) if url == "https://github.com/org/repo/tree/main/serde"
            ),
            "expected Git source, got {:?}",
            group.source
        );
    }

    #[test]
    fn parse_shell_predicates_top_level() {
        let toml = indoc! {r#"
            name = "shell-pred-plugin"
            crates = ["*"]
            shell_predicates = ["command -v rg", "test -f Cargo.toml"]

            [[skills]]
            crates = ["serde"]
        "#};
        let plugin = from_str(toml).expect("parse");
        assert_eq!(plugin.shell_predicates.commands.len(), 2);
        assert_eq!(plugin.shell_predicates.commands[0], "command -v rg");
    }

    #[test]
    fn parse_shell_predicates_on_skill_group() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[skills]]
            crates = ["serde"]
            shell_predicates = ["command -v jq"]
        "#};
        let plugin = from_str(toml).expect("parse");
        assert_eq!(plugin.skills[0].shell_predicates.commands.len(), 1);
    }

    #[test]
    fn parse_shell_predicates_on_hook() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[hooks]]
            name = "h"
            event = "PreToolUse"
            command = { script = "scripts/x.sh" }
            shell_predicates = ["test -d .git"]
        "#};
        let plugin = from_str(toml).expect("parse");
        assert_eq!(plugin.hooks[0].shell_predicates.commands.len(), 1);
    }

    #[test]
    fn shell_predicates_default_empty() {
        let plugin = from_str(SAMPLE).expect("parse");
        assert!(plugin.shell_predicates.is_empty());
        assert!(plugin.hooks[0].shell_predicates.is_empty());
    }

    #[test]
    fn parse_manifest_crates_as_array() {
        let toml = indoc! {r#"
            name = "array-crates"
            crates = ["*"]

            [[skills]]
            crates = ["serde"]
        "#};
        let plugin = from_str(toml).expect("parse");
        let group = &plugin.skills[0];
        let cr = group.crates.as_ref().unwrap();
        assert_eq!(cr.predicates.len(), 1);
        assert!(cr.predicates[0].references_crate("serde"));
    }

    #[test]
    fn scan_source_dir_finds_plugins_and_standalone_skills() {
        use crate::test_utils::{File, instantiate_fixture};
        let tmp = instantiate_fixture(&[
            File(
                "my-plugin/SYMPOSIUM.toml",
                indoc! {r#"
                name = "my-plugin"
                crates = ["*"]

                [[hooks]]
                name = "test"
                event = "PreToolUse"
                command = { executable = "/bin/echo" }
            "#},
            ),
            File(
                "assert-struct/SKILL.md",
                indoc! {"
                ---
                name: assert-struct
                description: Check struct layout
                crates: serde
                ---

                Use this skill.
            "},
            ),
        ]);
        // Also create a random directory (should be ignored)
        std::fs::create_dir_all(tmp.path().join("not-a-plugin-or-skill")).unwrap();

        let contents = scan_source_dir(tmp.path()).unwrap();
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
        use crate::test_utils::{File, instantiate_fixture};
        let tmp = instantiate_fixture(&[File(
            "SKILL.md",
            indoc! {"
                ---
                name: root-skill
                crates: serde
                ---

                Root level skill.
            "},
        )]);

        let err = scan_source_dir(tmp.path()).unwrap_err();
        assert!(
            err.to_string()
                .contains("plugin source root contains SKILL.md"),
            "expected root SKILL.md error, got: {err}"
        );
    }

    #[test]
    fn scan_source_dir_rejects_root_level_plugin() {
        use crate::test_utils::{File, instantiate_fixture};
        let tmp = instantiate_fixture(&[File(
            "SYMPOSIUM.toml",
            indoc! {r#"
                name = "root-plugin"
                crates = ["*"]
            "#},
        )]);

        let err = scan_source_dir(tmp.path()).unwrap_err();
        assert!(
            err.to_string()
                .contains("plugin source root contains SYMPOSIUM.toml"),
            "expected root SYMPOSIUM.toml error, got: {err}"
        );
    }

    #[test]
    fn scan_source_dir_plugin_takes_precedence_over_skill() {
        use crate::test_utils::{File, instantiate_fixture};
        let tmp = instantiate_fixture(&[
            File(
                "mixed/SYMPOSIUM.toml",
                indoc! {r#"
                name = "mixed-plugin"
                crates = ["*"]
            "#},
            ),
            File(
                "mixed/SKILL.md",
                indoc! {"
                ---
                name: ignored-skill
                crates: serde
                ---

                This should be ignored.
            "},
            ),
        ]);

        let contents = scan_source_dir(tmp.path()).unwrap();
        assert_eq!(contents.plugins.len(), 1);
        assert_eq!(contents.skill_files.len(), 0);
        expect_test::expect![[r#"mixed-plugin"#]]
            .assert_eq(&contents.plugins[0].as_ref().unwrap().plugin.name);
    }

    #[test]
    fn scan_source_dir_symposium_toml_precedence() {
        use crate::test_utils::{File, instantiate_fixture};
        let tmp = instantiate_fixture(&[
            File(
                "precedence-test/SYMPOSIUM.toml",
                indoc! {r#"
                name = "preferred-plugin"
                crates = ["*"]
            "#},
            ),
            File(
                "precedence-test/other.toml",
                indoc! {r#"
                name = "ignored-plugin"
            "#},
            ),
        ]);

        let contents = scan_source_dir(tmp.path()).unwrap();
        assert_eq!(contents.plugins.len(), 1);
        assert_eq!(contents.skill_files.len(), 0);
        expect_test::expect![[r#"preferred-plugin"#]]
            .assert_eq(&contents.plugins[0].as_ref().unwrap().plugin.name);
    }

    #[test]
    fn scan_source_dir_pruning_behavior() {
        use crate::test_utils::{File, instantiate_fixture};
        let tmp = instantiate_fixture(&[
            File(
                "foo/SYMPOSIUM.toml",
                indoc! {r#"
                name = "foo-plugin"
                crates = ["*"]
            "#},
            ),
            File(
                "foo/bar/SKILL.md",
                indoc! {"
                ---
                name: foo-bar-skill
                crates: serde
                ---

                Should be pruned.
            "},
            ),
            File(
                "baz/SKILL.md",
                indoc! {"
                ---
                name: baz-skill
                crates: tokio
                ---

                Should be found.
            "},
            ),
            File(
                "baz/qux/SYMPOSIUM.toml",
                indoc! {r#"
                name = "qux-plugin"
                crates = ["*"]
            "#},
            ),
            File(
                "baz/qux/SKILL.md",
                indoc! {"
                ---
                name: qux-skill
                crates: anyhow
                ---

                Should be pruned.
            "},
            ),
        ]);

        let contents = scan_source_dir(tmp.path()).unwrap();
        assert_eq!(contents.plugins.len(), 1);
        assert_eq!(contents.skill_files.len(), 1);
        expect_test::expect![[r#"foo-plugin"#]]
            .assert_eq(&contents.plugins[0].as_ref().unwrap().plugin.name);
        assert!(contents.skill_files[0].ends_with("baz/SKILL.md"));
    }

    #[test]
    fn validate_source_dir_mixed() {
        use crate::test_utils::{File, instantiate_fixture};
        let tmp = instantiate_fixture(&[
            File(
                "good-plugin/SYMPOSIUM.toml",
                indoc! {r#"
                name = "good-plugin"
                crates = ["serde"]
            "#},
            ),
            File("bad-plugin/SYMPOSIUM.toml", "not valid toml {{{"),
            File(
                "my-skill/SKILL.md",
                indoc! {"
                ---
                name: my-skill
                description: A skill
                crates: serde
                ---

                Body.
            "},
            ),
            File(
                "bad-skill/SKILL.md",
                indoc! {"
                ---
                description: No name
                crates: serde
                ---

                Body.
            "},
            ),
        ]);

        let results = validate_source_dir(tmp.path()).unwrap();
        let ok_count = results.iter().filter(|r| r.result.is_ok()).count();
        let err_count = results.iter().filter(|r| r.result.is_err()).count();
        assert_eq!(results.len(), 4);
        assert_eq!(ok_count, 2);
        assert_eq!(err_count, 2);
    }

    #[test]
    fn validate_source_dir_rejects_illformed_standalone_skill() {
        use crate::test_utils::{File, instantiate_fixture};
        let tmp = instantiate_fixture(&[File(
            "bad-skill/SKILL.md",
            indoc! {"
                ---
                name: rust-best-practice
                description: [Critical] Best practice for Rust coding.
                crates: serde
                ---

                Body.
            "},
        )]);

        let results = validate_source_dir(tmp.path()).unwrap();
        assert_eq!(results.len(), 1);
        assert!(
            results[0].result.is_err(),
            "standalone skill with non-string YAML value should fail validation"
        );
    }

    #[test]
    fn collect_crate_names_from_source_dir() {
        use crate::test_utils::{File, instantiate_fixture};
        let tmp = instantiate_fixture(&[
            File(
                "my-plugin/SYMPOSIUM.toml",
                indoc! {r#"
                name = "my-plugin"
                crates = ["*"]

                [[skills]]
                crates = ["serde", "serde_json>=1.0"]
            "#},
            ),
            File(
                "my-skill/SKILL.md",
                indoc! {"
                ---
                name: my-skill
                description: A skill
                crates: anyhow
                ---

                Body.
            "},
            ),
        ]);

        let names = collect_crate_names_in_source_dir(tmp.path()).unwrap();
        // BTreeSet means sorted output
        assert_eq!(names, vec!["anyhow", "serde", "serde_json"]);
    }

    #[test]
    fn collect_crate_names_skips_invalid_items() {
        use crate::test_utils::{File, instantiate_fixture};
        let tmp = instantiate_fixture(&[
            File("bad-plugin/SYMPOSIUM.toml", "not valid {{{"),
            File(
                "good-skill/SKILL.md",
                indoc! {"
                ---
                name: good
                description: Good skill
                crates: serde
                ---

                Body.
            "},
            ),
            File(
                "bad-skill/SKILL.md",
                indoc! {"
                ---
                name: bad
                ---

                Body.
            "},
            ),
        ]);

        let names = collect_crate_names_in_source_dir(tmp.path()).unwrap();
        // Only the valid skill's crate name
        assert_eq!(names, vec!["serde"]);
    }

    #[tokio::test]
    async fn check_crate_exists_on_crates_io() {
        assert!(check_crate_exists("serde").await);
        assert!(!check_crate_exists("this-crate-definitely-does-not-exist-zzz").await);
    }

    #[test]
    fn path_at_wrong_level_is_rejected() {
        let toml = indoc! {r#"
            name = "Symposium"
            crates = ["*"]

            [[skills]]
            path = "."
        "#};
        let err = from_str(toml).unwrap_err();
        assert!(
            err.to_string().contains("unknown field"),
            "expected unknown field error, got: {err}"
        );
    }

    #[test]
    fn parse_manifest_with_multiple_skill_groups() {
        let toml = indoc! {r#"
            name = "multi-group"
            crates = ["*"]

            [[skills]]
            crates = ["serde"]

            [[skills]]
            crates = ["tokio"]
        "#};
        let plugin = from_str(toml).expect("parse");
        assert_eq!(plugin.name, "multi-group");
        assert_eq!(plugin.skills.len(), 2);
        assert!(plugin.skills[0].crates.as_ref().unwrap().predicates[0].references_crate("serde"));
        assert!(plugin.skills[1].crates.as_ref().unwrap().predicates[0].references_crate("tokio"));
    }

    #[test]
    fn plugin_crate_filtering() {
        let workspace_crates = vec![
            ("serde".to_string(), semver::Version::new(1, 0, 0)),
            ("tokio".to_string(), semver::Version::new(1, 0, 0)),
        ];

        // Plugin with wildcard - should apply to all
        let plugin_wildcard = Plugin {
            name: "wildcard".to_string(),
            crates: pred_set("*"),
            shell_predicates: Default::default(),
            hooks: vec![],
            skills: vec![],
            mcp_servers: vec![],
            installations: Vec::new(),
        };
        assert!(plugin_wildcard.applies_to_crates(&workspace_crates));

        // Plugin targeting serde - should apply
        let plugin_serde = Plugin {
            name: "serde-plugin".to_string(),
            crates: pred_set("serde"),
            shell_predicates: Default::default(),
            hooks: vec![],
            skills: vec![],
            mcp_servers: vec![],
            installations: Vec::new(),
        };
        assert!(plugin_serde.applies_to_crates(&workspace_crates));

        // Plugin targeting non-existent crate - should not apply
        let plugin_other = Plugin {
            name: "other-plugin".to_string(),
            crates: pred_set("other-crate"),
            shell_predicates: Default::default(),
            hooks: vec![],
            skills: vec![],
            mcp_servers: vec![],
            installations: Vec::new(),
        };
        assert!(!plugin_other.applies_to_crates(&workspace_crates));

        // Plugin with version predicate - should reject wrong version
        let plugin_version = Plugin {
            name: "version-plugin".to_string(),
            crates: pred_set("tokio>=2.0"),
            shell_predicates: Default::default(),
            hooks: vec![],
            skills: vec![],
            mcp_servers: vec![],
            installations: Vec::new(),
        };
        assert!(!plugin_version.applies_to_crates(&workspace_crates));
    }

    #[test]
    fn validate_source_dir_enforces_crates_requirement() {
        use crate::test_utils::{File, instantiate_fixture};
        let tmp = instantiate_fixture(&[
            File(
                "no-crates-plugin/SYMPOSIUM.toml",
                indoc! {r#"
                name = "no-crates-plugin"

                [[hooks]]
                name = "some-hook"
                event = "PreToolUse"
                command = { executable = "/bin/echo" }
            "#},
            ),
            File(
                "good-plugin/SYMPOSIUM.toml",
                indoc! {r#"
                name = "good-plugin"
                crates = ["serde"]

                [[hooks]]
                name = "some-hook"
                event = "PreToolUse"
                command = { executable = "/bin/echo" }
            "#},
            ),
        ]);

        let results = validate_source_dir(tmp.path()).unwrap();
        assert_eq!(results.len(), 2);

        let ok_count = results.iter().filter(|r| r.result.is_ok()).count();
        let err_count = results.iter().filter(|r| r.result.is_err()).count();
        assert_eq!(ok_count, 1, "Plugin with crates should pass");
        assert_eq!(
            err_count, 1,
            "Plugin without crates should fail TOML parsing"
        );
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

    /// Cargo-installed binary referenced by name as the hook's command.
    /// Demonstrates the "install a binary, run it as a hook" pattern.
    #[test]
    fn cargo_install_used_as_hook() {
        let toml = indoc! {r#"
            name = "cargo-as-hook"
            crates = ["*"]

            [[installations]]
            name = "rg"
            source = "cargo"
            crate = "ripgrep"
            executable = "rg"

            [[hooks]]
            name = "rg-version"
            event = "PreToolUse"
            command = "rg"
            args = ["--version"]
        "#};
        let plugin = from_str(toml).expect("parse");
        let hook = &plugin.hooks[0];
        assert_eq!(hook.command, "rg");
        assert!(hook.executable.is_none());
        assert!(hook.script.is_none());
        assert_eq!(hook.args, vec!["--version".to_string()]);
    }

    /// rtk: install the rtk binary as a requirement, run a hook script
    /// pulled from a separate github source. The hook picks the script file
    /// inside the repo at the use site.
    #[test]
    fn rtk_requirement_plus_github_command() {
        let toml = indoc! {r#"
            name = "rtk-plugin"
            crates = ["*"]

            [[installations]]
            name = "rtk"
            source = "cargo"
            crate = "rtk"

            [[installations]]
            name = "rtk-hooks"
            source = "github"
            url = "https://github.com/example/rtk-hooks"

            [[hooks]]
            name = "rewrite"
            event = "PreToolUse"
            requirements = ["rtk"]
            command = "rtk-hooks"
            script = "hooks/claude/rtk-rewrite.sh"
            args = ["--format"]
        "#};
        let plugin = from_str(toml).expect("parse");
        let hook = &plugin.hooks[0];
        assert_eq!(hook.requirements, vec!["rtk".to_string()]);
        assert_eq!(hook.command, "rtk-hooks");
        assert_eq!(hook.script.as_deref(), Some("hooks/claude/rtk-rewrite.sh"));
        assert_eq!(hook.args, vec!["--format".to_string()]);
    }

    /// `script` on a github installation pins the file; hooks need not repeat it.
    #[test]
    fn github_script_on_installation_is_used() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[installations]]
            name = "g"
            source = "github"
            url = "https://github.com/o/r"
            script = "scripts/x.sh"

            [[hooks]]
            name = "h"
            event = "PreToolUse"
            command = "g"
        "#};
        let plugin = from_str(toml).expect("parse");
        let install = plugin.installations.iter().find(|i| i.name == "g").unwrap();
        assert_eq!(install.script.as_deref(), Some("scripts/x.sh"));
        assert!(plugin.hooks[0].script.is_none());
    }

    #[test]
    fn missing_named_installation_errors() {
        let toml = indoc! {r#"
            name = "bad-plugin"
            crates = ["*"]

            [[hooks]]
            name = "rewrite"
            event = "PreToolUse"
            command = "nope"
        "#};
        let err = from_str(toml).unwrap_err();
        assert!(
            err.to_string().contains("unknown installation"),
            "expected unknown-installation error, got: {err}"
        );
    }

    /// Setting `executable` and `script` on the same installation is rejected.
    #[test]
    fn executable_and_script_together_errors() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[installations]]
            name = "x"
            executable = "bin/x"
            script = "scripts/x.sh"
        "#};
        let err = from_str(toml).unwrap_err();
        assert!(
            err.to_string()
                .contains("`executable` and `script` are mutually exclusive"),
            "got: {err}"
        );
    }

    /// Same kind set on installation and hook is rejected.
    #[test]
    fn executable_set_on_both_layers_errors() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[installations]]
            name = "g"
            source = "github"
            url = "https://github.com/o/r"
            executable = "bin/x"

            [[hooks]]
            name = "h"
            event = "PreToolUse"
            command = "g"
            executable = "bin/y"
        "#};
        let err = from_str(toml).unwrap_err();
        assert!(
            err.to_string()
                .contains("at most one of `executable` / `script`"),
            "got: {err}"
        );
    }

    /// Mixing kinds across layers is rejected too: install has executable,
    /// hook tries to add script.
    #[test]
    fn executable_install_with_hook_script_errors() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[installations]]
            name = "g"
            source = "github"
            url = "https://github.com/o/r"
            executable = "bin/x"

            [[hooks]]
            name = "h"
            event = "PreToolUse"
            command = "g"
            script = "scripts/x.sh"
        "#};
        let err = from_str(toml).unwrap_err();
        assert!(
            err.to_string()
                .contains("at most one of `executable` / `script`"),
            "got: {err}"
        );
    }

    /// Install.script + hook.script — same kind on both layers.
    #[test]
    fn script_set_on_both_layers_errors() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[installations]]
            name = "g"
            source = "github"
            url = "https://github.com/o/r"
            script = "a.sh"

            [[hooks]]
            name = "h"
            event = "PreToolUse"
            command = "g"
            script = "b.sh"
        "#};
        let err = from_str(toml).unwrap_err();
        assert!(
            err.to_string()
                .contains("at most one of `executable` / `script`"),
            "got: {err}"
        );
    }

    /// Install.script + hook.exec — different kinds across layers.
    #[test]
    fn script_install_with_hook_executable_errors() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[installations]]
            name = "g"
            source = "github"
            url = "https://github.com/o/r"
            script = "a.sh"

            [[hooks]]
            name = "h"
            event = "PreToolUse"
            command = "g"
            executable = "bin/x"
        "#};
        let err = from_str(toml).unwrap_err();
        assert!(
            err.to_string()
                .contains("at most one of `executable` / `script`"),
            "got: {err}"
        );
    }

    /// Hook.exec + hook.script — both set on the hook itself.
    #[test]
    fn hook_executable_and_script_together_errors() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[installations]]
            name = "setup"
            install_commands = ["true"]

            [[hooks]]
            name = "h"
            event = "PreToolUse"
            command = "setup"
            executable = "/bin/echo"
            script = "x.sh"
        "#};
        let err = from_str(toml).unwrap_err();
        assert!(
            err.to_string()
                .contains("at most one of `executable` / `script`"),
            "got: {err}"
        );
    }

    /// A bare-installation + hook-level `script` is valid: the installation
    /// only contributes `install_commands`, the hook supplies the runnable.
    #[test]
    fn hook_script_against_bare_installation_is_ok() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[installations]]
            name = "setup"
            install_commands = ["true"]

            [[hooks]]
            name = "h"
            event = "PreToolUse"
            command = "setup"
            script = "x.sh"
        "#};
        let plugin = from_str(toml).expect("parse");
        let hook = &plugin.hooks[0];
        assert_eq!(hook.script.as_deref(), Some("x.sh"));
        assert!(hook.executable.is_none());
    }

    /// Cargo source with `git` and no `executable` is rejected at parse time —
    /// we can't infer the binary name without consulting crates.io.
    #[test]
    fn cargo_git_without_executable_errors() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[installations]]
            name = "tool"
            source = "cargo"
            crate = "tool"
            git = "https://github.com/example/tool"

            [[hooks]]
            name = "h"
            event = "PreToolUse"
            command = "tool"
        "#};
        let err = from_str(toml).unwrap_err();
        assert!(
            err.to_string().contains("`git` requires `executable`"),
            "got: {err}"
        );
    }

    /// `args` may be set on the installation OR the hook, but not both.
    #[test]
    fn args_set_on_both_layers_is_error() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[installations]]
            name = "rg"
            source = "cargo"
            crate = "ripgrep"
            executable = "rg"
            args = ["--default"]

            [[hooks]]
            name = "h"
            event = "PreToolUse"
            command = "rg"
            args = ["--override"]
        "#};
        let err = from_str(toml).unwrap_err();
        assert!(
            err.to_string().contains("`args` is set on both"),
            "got: {err}"
        );
    }

    /// Hook with no args inherits installation defaults.
    #[test]
    fn hook_inherits_installation_args() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[installations]]
            name = "rg"
            source = "cargo"
            crate = "ripgrep"
            executable = "rg"
            args = ["--default"]

            [[hooks]]
            name = "h"
            event = "PreToolUse"
            command = "rg"
        "#};
        let plugin = from_str(toml).expect("parse");
        assert_eq!(plugin.hooks[0].args, vec!["--default".to_string()]);
    }

    /// Inline command is promoted to a synthetic installation named after the hook.
    #[test]
    fn inline_installation_in_command() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[hooks]]
            name = "inline"
            event = "PreToolUse"
            command = { source = "cargo", crate = "rtk", executable = "rtk" }
        "#};
        let plugin = from_str(toml).expect("parse");
        let hook = &plugin.hooks[0];
        assert_eq!(hook.command, "inline");
        let installation = plugin
            .installations
            .iter()
            .find(|i| i.name == "inline")
            .expect("synthetic");
        assert!(matches!(
            &installation.source,
            Some(Source::Cargo(c)) if c.crate_name == "rtk"
        ));
        assert_eq!(installation.executable.as_deref(), Some("rtk"));
        assert!(hook.executable.is_none());
        assert!(hook.script.is_none());
        assert!(hook.args.is_empty());
    }

    /// A no-source inline `command` (just an executable on disk) works.
    #[test]
    fn inline_no_source_executable() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[hooks]]
            name = "h"
            event = "PreToolUse"
            command = { executable = "/usr/local/bin/tool" }
        "#};
        let plugin = from_str(toml).expect("parse");
        let install = plugin
            .installations
            .iter()
            .find(|i| i.name == "h")
            .expect("synthetic");
        assert!(install.source.is_none());
        assert_eq!(install.executable.as_deref(), Some("/usr/local/bin/tool"));
    }

    /// Inline command's synthesized name (= hook name) clashing with an
    /// existing user-declared installation is rejected.
    #[test]
    fn inline_command_name_clash_errors() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[installations]]
            name = "h"
            source = "cargo"
            crate = "x"

            [[hooks]]
            name = "h"
            event = "PreToolUse"
            command = { executable = "/bin/echo" }
        "#};
        let err = from_str(toml).unwrap_err();
        assert!(err.to_string().contains("conflicts"), "got: {err}");
    }

    /// Hook + inline command setting args on both is rejected.
    #[test]
    fn inline_command_with_hook_args_errors() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[hooks]]
            name = "h"
            event = "PreToolUse"
            command = { executable = "/bin/echo", args = ["a"] }
            args = ["b"]
        "#};
        let err = from_str(toml).unwrap_err();
        assert!(
            err.to_string().contains("`args` is set on both"),
            "got: {err}"
        );
    }

    /// `install_commands` round-trips through validation.
    #[test]
    fn install_commands_field_is_carried_through() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[installations]]
            name = "rg"
            source = "cargo"
            crate = "ripgrep"
            executable = "rg"
            install_commands = ["echo post-install ran", "true"]
        "#};
        let plugin = from_str(toml).expect("parse");
        let install = plugin
            .installations
            .iter()
            .find(|i| i.name == "rg")
            .unwrap();
        assert_eq!(
            install.install_commands,
            vec!["echo post-install ran".to_string(), "true".to_string()]
        );
    }

    /// `install_commands` set on an inline `command` ends up on the synthetic
    /// installation that the hook gets promoted to.
    #[test]
    fn install_commands_on_inline_command() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[hooks]]
            name = "h"
            event = "PreToolUse"
            command = { executable = "/bin/echo", install_commands = ["echo prep"] }
        "#};
        let plugin = from_str(toml).expect("parse");
        let synth = plugin
            .installations
            .iter()
            .find(|i| i.name == "h")
            .expect("synthetic");
        assert_eq!(synth.install_commands, vec!["echo prep".to_string()]);
    }

    /// `install_commands` set on an inline `requirement` ends up on the
    /// synthetic installation promoted from that requirement.
    #[test]
    fn install_commands_on_inline_requirement() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[hooks]]
            name = "h"
            event = "PreToolUse"
            requirements = [
                { install_commands = ["echo req-prep"] },
            ]
            command = { executable = "/bin/echo" }
        "#};
        let plugin = from_str(toml).expect("parse");
        let synth = plugin
            .installations
            .iter()
            .find(|i| i.name == "h__req_0")
            .expect("synthetic requirement");
        assert_eq!(synth.install_commands, vec!["echo req-prep".to_string()]);
    }

    /// A hook whose command resolves to an installation with no
    /// `executable`/`script` (and the hook supplies none) is rejected at
    /// parse time, except for cargo (where the binary can be inferred).
    #[test]
    fn hook_command_must_resolve_to_runnable() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[installations]]
            name = "setup"
            install_commands = ["echo prep"]

            [[hooks]]
            name = "h"
            event = "PreToolUse"
            command = "setup"
        "#};
        let err = from_str(toml).unwrap_err();
        assert!(err.to_string().contains("nothing to run"), "got: {err}");
    }

    /// Cargo source without explicit `executable` is allowed — the binary is
    /// inferred from crates.io at acquisition time.
    #[test]
    fn cargo_without_executable_is_ok() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[installations]]
            name = "rg"
            source = "cargo"
            crate = "ripgrep"

            [[hooks]]
            name = "h"
            event = "PreToolUse"
            command = "rg"
        "#};
        from_str(toml).expect("parse");
    }

    /// `global = true` on cargo source round-trips through validation.
    #[test]
    fn cargo_global_field_round_trips() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[installations]]
            name = "rg"
            source = "cargo"
            crate = "ripgrep"
            executable = "rg"
            global = true

            [[hooks]]
            name = "h"
            event = "PreToolUse"
            command = "rg"
        "#};
        let plugin = from_str(toml).expect("parse");
        let install = plugin
            .installations
            .iter()
            .find(|i| i.name == "rg")
            .unwrap();
        match &install.source {
            Some(Source::Cargo(c)) => assert!(c.global),
            _ => panic!("expected cargo source"),
        }
    }

    /// Cargo source with `global = true` and no `executable` is rejected at
    /// parse time — we don't infer the binary from crates.io for global
    /// installs, so the user must say what to spawn.
    #[test]
    fn cargo_global_without_executable_errors() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[installations]]
            name = "rg"
            source = "cargo"
            crate = "ripgrep"
            global = true

            [[hooks]]
            name = "h"
            event = "PreToolUse"
            command = "rg"
        "#};
        let err = from_str(toml).unwrap_err();
        assert!(
            err.to_string()
                .contains("`global = true` requires `executable`"),
            "got: {err}"
        );
    }

    /// `git` field on cargo source round-trips through validation.
    #[test]
    fn cargo_git_field_round_trips() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[installations]]
            name = "tool"
            source = "cargo"
            crate = "tool"
            git = "https://github.com/example/tool"
            executable = "tool"

            [[hooks]]
            name = "h"
            event = "PreToolUse"
            command = "tool"
        "#};
        let plugin = from_str(toml).expect("parse");
        let install = plugin
            .installations
            .iter()
            .find(|i| i.name == "tool")
            .unwrap();
        match &install.source {
            Some(Source::Cargo(c)) => {
                assert_eq!(c.git.as_deref(), Some("https://github.com/example/tool"));
            }
            _ => panic!("expected cargo source"),
        }
    }

    /// Inline installations may carry their own `requirements`. The validated
    /// shape ends up with the requirement promoted under `<owner>__req_<i>`.
    #[test]
    fn inline_installation_can_have_requirements() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[installations]]
            name = "rtk"
            source = "cargo"
            crate = "rtk"

            [[hooks]]
            name = "h"
            event = "PreToolUse"
            command = { source = "github", url = "https://github.com/o/r", script = "x.sh", requirements = ["rtk"] }
        "#};
        let plugin = from_str(toml).expect("parse");
        let synth = plugin.installations.iter().find(|i| i.name == "h").unwrap();
        assert_eq!(synth.requirements, vec!["rtk".to_string()]);
        // Hook's requirements include the synthesized command's own reqs (one level).
        assert_eq!(plugin.hooks[0].requirements, vec!["rtk".to_string()]);
    }

    /// An installation may carry only `install_commands` (pure setup, no
    /// runnable). Useful as a side-effect requirement.
    #[test]
    fn pure_install_commands_installation_is_ok() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[installations]]
            name = "setup"
            install_commands = ["echo prep"]

            [[hooks]]
            name = "h"
            event = "PreToolUse"
            requirements = ["setup"]
            command = { executable = "/bin/echo" }
        "#};
        let plugin = from_str(toml).expect("parse");
        let setup = plugin
            .installations
            .iter()
            .find(|i| i.name == "setup")
            .unwrap();
        assert!(setup.source.is_none());
        assert!(setup.executable.is_none());
        assert!(setup.script.is_none());
        assert_eq!(setup.install_commands, vec!["echo prep".to_string()]);
    }

    #[test]
    fn duplicate_installation_name_errors() {
        let toml = indoc! {r#"
            name = "dup"
            crates = ["*"]

            [[installations]]
            name = "x"
            source = "cargo"
            crate = "a"

            [[installations]]
            name = "x"
            source = "cargo"
            crate = "b"
        "#};
        let err = from_str(toml).unwrap_err();
        assert!(
            err.to_string().contains("duplicate installation"),
            "got: {err}"
        );
    }

    #[test]
    fn requirements_named_and_inline() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[installations]]
            name = "rtk"
            source = "cargo"
            crate = "rtk"

            [[hooks]]
            name = "uses-req"
            event = "PreToolUse"
            requirements = [
                "rtk",
                { source = "cargo", crate = "ripgrep" },
            ]
            command = { executable = "/bin/echo" }
        "#};
        let plugin = from_str(toml).expect("parse");
        let reqs = &plugin.hooks[0].requirements;
        assert_eq!(reqs.len(), 2);
        assert_eq!(reqs[0], "rtk");
        assert_eq!(reqs[1], "uses-req__req_1");
        let synth = plugin
            .installations
            .iter()
            .find(|i| i.name == "uses-req__req_1")
            .expect("synthetic");
        assert!(matches!(&synth.source, Some(Source::Cargo(_))));
    }

    /// An installation's own `requirements` are appended (one level) to any
    /// hook that references that installation as its command.
    #[test]
    fn installation_requirements_propagate_to_hook() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[installations]]
            name = "rtk"
            source = "cargo"
            crate = "rtk"

            [[installations]]
            name = "rtk-hooks"
            source = "github"
            url = "https://github.com/example/rtk-hooks"
            requirements = ["rtk"]

            [[hooks]]
            name = "rewrite"
            event = "PreToolUse"
            command = "rtk-hooks"
            script = "hooks/x.sh"
        "#};
        let plugin = from_str(toml).expect("parse");
        let reqs = &plugin.hooks[0].requirements;
        assert_eq!(reqs, &vec!["rtk".to_string()]);
    }

    /// Installation-level requirements pull in via a named hook requirement
    /// too, not just the command.
    #[test]
    fn installation_requirements_propagate_via_named_hook_requirement() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[installations]]
            name = "a"
            source = "cargo"
            crate = "a"

            [[installations]]
            name = "b"
            source = "cargo"
            crate = "b"
            requirements = ["a"]

            [[hooks]]
            name = "h"
            event = "PreToolUse"
            requirements = ["b"]
            command = { executable = "/bin/echo" }
        "#};
        let plugin = from_str(toml).expect("parse");
        let reqs = &plugin.hooks[0].requirements;
        assert_eq!(reqs, &vec!["a".to_string(), "b".to_string()]);
    }

    /// Installation requirements can also be inline.
    #[test]
    fn installation_requirements_can_be_inline() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[installations]]
            name = "rtk-hooks"
            source = "github"
            url = "https://github.com/example/rtk-hooks"
            requirements = [{ source = "cargo", crate = "rtk" }]

            [[hooks]]
            name = "rewrite"
            event = "PreToolUse"
            command = "rtk-hooks"
            script = "hooks/x.sh"
        "#};
        let plugin = from_str(toml).expect("parse");
        let reqs = &plugin.hooks[0].requirements;
        assert_eq!(reqs.len(), 1);
        let synth_name = &reqs[0];
        assert_eq!(synth_name, "rtk-hooks__req_0");
        let synth = plugin
            .installations
            .iter()
            .find(|i| &i.name == synth_name)
            .expect("synthetic");
        assert!(matches!(&synth.source, Some(Source::Cargo(_))));
    }

    /// An unknown name in an installation's `requirements` is rejected at
    /// parse time, just like in hook requirements.
    #[test]
    fn installation_requirement_unknown_name_errors() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[installations]]
            name = "x"
            source = "cargo"
            crate = "x"
            requirements = ["nope"]
        "#};
        let err = from_str(toml).unwrap_err();
        assert!(
            err.to_string().contains("unknown installation"),
            "got: {err}"
        );
    }

    #[test]
    fn requirements_unknown_named_errors() {
        let toml = indoc! {r#"
            name = "p"
            crates = ["*"]

            [[hooks]]
            name = "h"
            event = "PreToolUse"
            requirements = ["nope"]
            command = { executable = "/bin/echo" }
        "#};
        let err = from_str(toml).unwrap_err();
        assert!(
            err.to_string().contains("unknown installation"),
            "got: {err}"
        );
    }

    // --- source mutual exclusivity and crate_path tests ---

    #[test]
    fn parse_crate_path_source() {
        let toml = indoc! {r#"
            name = "crate-path-plugin"
            crates = ["serde"]

            [[skills]]
            source.crate_path = "skills"
        "#};
        let plugin = from_str(toml).expect("parse");
        assert!(
            matches!(
                &plugin.skills[0].source,
                PluginSource::CratePath(s) if s.explicit_path.as_deref() == Some("skills")
            ),
            r#"explicit source.crate_path = "skills" should be CratePath with explicit_path=Some("skills"), got {:?}"#,
            plugin.skills[0].source,
        );
    }

    #[test]
    fn parse_source_crate_shorthand() {
        let toml = indoc! {r#"
            name = "crate-shorthand"
            crates = ["serde"]

            [[skills]]
            source = "crate"
        "#};
        let plugin = from_str(toml).expect("parse");
        assert!(
            matches!(
                &plugin.skills[0].source,
                PluginSource::CratePath(s) if s.is_shorthand()
            ),
            r#"source = "crate" shorthand should be a shorthand CratePath, got {:?}"#,
            plugin.skills[0].source,
        );
    }

    /// The shorthand and explicit forms are intentionally distinguishable in
    /// the data model so that the serializer can preserve whichever form the
    /// user originally wrote, even when the explicit form happens to resolve
    /// to the default subdirectory.
    #[test]
    fn explicit_default_path_preserves_table_form() {
        let plugin = from_str(indoc! {r#"
            name = "rt"
            crates = ["serde"]

            [[skills]]
            source.crate_path = ".symposium/skills"
        "#})
        .unwrap();
        // Parsed value is the explicit variant.
        assert!(matches!(
            &plugin.skills[0].source,
            PluginSource::CratePath(s) if !s.is_shorthand() && s.as_str() == ".symposium/skills"
        ));
        // Serialized form keeps the table, does NOT collapse to "crate".
        let toml_str = toml::to_string_pretty(&plugin).expect("serialize");
        assert!(
            !toml_str.contains(r#"source = "crate""#),
            "explicit `source.crate_path = \".symposium/skills\"` should NOT collapse to shorthand, got:\n{toml_str}"
        );
        assert!(
            toml_str.contains("crate_path"),
            "explicit form should remain a table, got:\n{toml_str}"
        );
    }

    /// The `source = "crate"` shorthand resolves to the default subpath.
    #[test]
    fn shorthand_resolves_to_default_path() {
        assert_eq!(CratePathSource::shorthand().as_str(), ".symposium/skills");
        assert_eq!(CratePathSource::DEFAULT_PATH, ".symposium/skills");
    }

    #[test]
    fn parse_source_unknown_string_is_error() {
        let toml = indoc! {r#"
            name = "bad"
            crates = ["serde"]

            [[skills]]
            source = "magic"
        "#};
        let err = from_str(toml).unwrap_err();
        assert!(
            err.to_string().contains("unknown source shorthand"),
            "expected unknown shorthand error, got: {err}"
        );
    }

    #[test]
    fn reject_path_and_git() {
        let toml = indoc! {r#"
            name = "bad"
            crates = ["serde"]

            [[skills]]
            source.path = "."
            source.git = "https://github.com/org/repo/tree/main/x"
        "#};
        let err = from_str(toml).unwrap_err();
        assert!(err.to_string().contains("mutually exclusive"), "{err}");
    }

    #[test]
    fn reject_path_and_crate_path() {
        let toml = indoc! {r#"
            name = "bad"
            crates = ["serde"]

            [[skills]]
            source.path = "."
            source.crate_path = "skills"
        "#};
        let err = from_str(toml).unwrap_err();
        assert!(err.to_string().contains("mutually exclusive"), "{err}");
    }

    #[test]
    fn reject_git_and_crate_path() {
        let toml = indoc! {r#"
            name = "bad"
            crates = ["serde"]

            [[skills]]
            source.git = "https://github.com/org/repo/tree/main/x"
            source.crate_path = "skills"
        "#};
        let err = from_str(toml).unwrap_err();
        assert!(err.to_string().contains("mutually exclusive"), "{err}");
    }

    // --- wildcard + crate_path validation tests ---

    #[test]
    fn crate_path_valid_with_plugin_non_wildcard() {
        let toml = indoc! {r#"
            name = "ok"
            crates = ["serde"]

            [[skills]]
            source.crate_path = "skills"
        "#};
        from_str(toml).expect("should be valid");
    }

    #[test]
    fn crate_path_valid_with_group_non_wildcard() {
        let toml = indoc! {r#"
            name = "ok"
            crates = ["*"]

            [[skills]]
            crates = ["serde"]
            source.crate_path = "skills"
        "#};
        from_str(toml).expect("should be valid");
    }

    #[test]
    fn crate_path_valid_with_mixed_wildcard_and_concrete() {
        let toml = indoc! {r#"
            name = "ok"
            crates = ["*", "serde"]

            [[skills]]
            source.crate_path = "skills"
        "#};
        from_str(toml).expect("should be valid");
    }

    #[test]
    fn crate_path_reject_all_wildcards() {
        let toml = indoc! {r#"
            name = "bad"
            crates = ["*"]

            [[skills]]
            crates = ["*"]
            source.crate_path = "skills"
        "#};
        let err = from_str(toml).unwrap_err();
        assert!(err.to_string().contains("non-wildcard"), "{err}");
    }

    #[test]
    fn crate_path_reject_wildcard_plugin_no_group_crates() {
        let toml = indoc! {r#"
            name = "bad"
            crates = ["*"]

            [[skills]]
            source.crate_path = "skills"
        "#};
        let err = from_str(toml).unwrap_err();
        assert!(err.to_string().contains("non-wildcard"), "{err}");
    }

    /// Shorthand (`source = "crate"`) is also subject to the wildcard check,
    /// since it resolves to CratePath too.
    #[test]
    fn crate_shorthand_reject_all_wildcards() {
        let toml = indoc! {r#"
            name = "bad"
            crates = ["*"]

            [[skills]]
            source = "crate"
        "#};
        let err = from_str(toml).unwrap_err();
        assert!(err.to_string().contains("non-wildcard"), "{err}");
    }

    // --- TOML serialization round-trip tests ---

    /// Serialize a plugin to TOML and parse it back.
    fn roundtrip(plugin: &Plugin) -> Plugin {
        let toml_str = toml::to_string_pretty(plugin).expect("serialize");
        from_str(&toml_str).unwrap_or_else(|e| panic!("round-trip parse failed:\n{toml_str}\n{e}"))
    }

    #[test]
    fn roundtrip_source_crate_shorthand() {
        let plugin = from_str(indoc! {r#"
            name = "rt"
            crates = ["serde"]

            [[skills]]
            source = "crate"
        "#})
        .unwrap();
        let rt = roundtrip(&plugin);
        assert!(
            matches!(
                &rt.skills[0].source,
                PluginSource::CratePath(s) if s.is_shorthand()
            ),
            "shorthand should round-trip as a shorthand CratePath, got {:?}",
            rt.skills[0].source,
        );
    }

    #[test]
    fn roundtrip_source_crate_path_custom() {
        let plugin = from_str(indoc! {r#"
            name = "rt"
            crates = ["serde"]

            [[skills]]
            source.crate_path = ".symposium/skills"
        "#})
        .unwrap();
        let rt = roundtrip(&plugin);
        assert!(
            matches!(
                &rt.skills[0].source,
                PluginSource::CratePath(s) if s.explicit_path.as_deref() == Some(".symposium/skills")
            ),
            r#"explicit crate_path should round-trip preserving the explicit path, got {:?}"#,
            rt.skills[0].source,
        );
    }

    #[test]
    fn roundtrip_source_path() {
        let plugin = from_str(indoc! {r#"
            name = "rt"
            crates = ["serde"]

            [[skills]]
            source.path = "skills/v1"
        "#})
        .unwrap();
        let rt = roundtrip(&plugin);
        assert!(
            matches!(
                &rt.skills[0].source,
                PluginSource::Path(p) if p.as_path() == std::path::Path::new("skills/v1")
            ),
            "expected Path source, got {:?}",
            rt.skills[0].source,
        );
    }

    #[test]
    fn roundtrip_source_git() {
        let plugin = from_str(indoc! {r#"
            name = "rt"
            crates = ["serde"]

            [[skills]]
            source.git = "https://github.com/org/repo/tree/main/skills"
        "#})
        .unwrap();
        let rt = roundtrip(&plugin);
        assert!(
            matches!(
                &rt.skills[0].source,
                PluginSource::Git(url) if url == "https://github.com/org/repo/tree/main/skills"
            ),
            "expected Git source, got {:?}",
            rt.skills[0].source,
        );
    }

    #[test]
    fn roundtrip_source_none() {
        let plugin = from_str(indoc! {r#"
            name = "rt"
            crates = ["serde"]

            [[skills]]
            crates = ["serde"]
        "#})
        .unwrap();
        let rt = roundtrip(&plugin);
        assert!(
            matches!(&rt.skills[0].source, PluginSource::None),
            "expected None source, got {:?}",
            rt.skills[0].source,
        );
    }

    #[test]
    fn serialize_crate_shorthand_uses_string_form() {
        let plugin = from_str(indoc! {r#"
            name = "rt"
            crates = ["serde"]

            [[skills]]
            source = "crate"
        "#})
        .unwrap();
        let toml_str = toml::to_string_pretty(&plugin).expect("serialize");
        assert!(
            toml_str.contains(r#"source = "crate""#),
            "shorthand CratePath should serialize as source = \"crate\", got:\n{toml_str}"
        );
    }

    #[test]
    fn serialize_custom_crate_path_uses_table_form() {
        let plugin = from_str(indoc! {r#"
            name = "rt"
            crates = ["serde"]

            [[skills]]
            source.crate_path = ".symposium/skills"
        "#})
        .unwrap();
        let toml_str = toml::to_string_pretty(&plugin).expect("serialize");
        assert!(
            toml_str.contains("crate_path"),
            "explicit crate_path should serialize as a table, got:\n{toml_str}"
        );
        assert!(
            !toml_str.contains(r#"source = "crate""#),
            "explicit crate_path should NOT use shorthand, got:\n{toml_str}"
        );
    }
}
