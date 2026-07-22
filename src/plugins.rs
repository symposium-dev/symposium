use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::config::Symposium;
use crate::hook::HookEvent;
use crate::hook_schema::HookAgent;
use crate::pm::{ANY_VERSION, PackageId};
use crate::skills::skill_origin_hash;
use symposium_install::Source;

use sacp::schema::McpServer;

/// An MCP server entry in a plugin manifest.
pub type McpServerEntry = McpServer;

/// An MCP server entry with optional activation predicates.
///
/// The server's `depends-on` and `predicates` fields are merged into one
/// [`PredicateSet`](crate::predicate::PredicateSet); the server is only
/// registered when that set holds (ANDed with the plugin-level set).
#[derive(Debug, Clone, Serialize)]
pub struct PluginMcpServer {
    #[serde(
        default,
        skip_serializing_if = "crate::predicate::PredicateSet::is_empty"
    )]
    pub predicates: crate::predicate::PredicateSet,
    #[serde(flatten)]
    pub server: McpServerEntry,
}

#[derive(Debug, Deserialize)]
struct RawPluginMcpServer {
    #[serde(default, rename = "depends-on")]
    depends_on: Option<crate::predicate::DependsOnList>,
    /// Rejected: renamed to `depends-on`.
    #[serde(default)]
    crates: Option<toml::Value>,
    #[serde(default)]
    predicates: crate::predicate::PredicateSet,
    #[serde(flatten)]
    server: McpServerEntry,
}

impl RawPluginMcpServer {
    fn validate(self) -> Result<PluginMcpServer> {
        reject_crates_field(&self.crates)?;
        Ok(PluginMcpServer {
            predicates: crate::predicate::PredicateSet::merged(self.depends_on, self.predicates),
            server: self.server,
        })
    }
}

/// Shared rejection for the retired `crates` field, with a migration hint.
fn reject_crates_field(crates: &Option<toml::Value>) -> Result<()> {
    if crates.is_some() {
        bail!("the `crates` field has been renamed; use `depends-on` instead");
    }
    Ok(())
}

use symposium_install::UpdateLevel;

/// Source declaration for a skill group.
///
/// Accepts one of:
/// - `source.path = "..."` — local path
/// - `source.git = "..."` — GitHub URL
///
/// A crate is no longer referenced from a skill group. A crate provides a
/// plugin (and its skills) via a `[[plugins]] source.cargo = "..."` chained
/// reference — see [`ChainedPlugin`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginSource {
    /// Local filesystem path, relative to the plugin manifest.
    Path(PathBuf),
    /// GitHub URL pointing to a directory in a repository.
    Git(String),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawPluginSource {
    Shorthand(String),
    Table(RawPluginSourceTable),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPluginSourceTable {
    #[serde(default)]
    path: Option<PathBuf>,
    #[serde(default)]
    git: Option<String>,
    /// Rejected: `source.crate = { ... }` is no longer valid.
    #[serde(default, rename = "crate")]
    crate_field: Option<toml::Value>,
    /// Rejected: `source.crate_path = "..."` is no longer valid.
    #[serde(default)]
    crate_path: Option<toml::Value>,
}

impl RawPluginSource {
    fn validate(self) -> Result<PluginSource> {
        match self {
            RawPluginSource::Shorthand(value) => bail!(
                "`source = \"{value}\"` is no longer supported; a crate now provides a plugin \
                 via a `[[plugins]] source.cargo = \"...\"` reference"
            ),
            RawPluginSource::Table(fields) => {
                if fields.crate_path.is_some() || fields.crate_field.is_some() {
                    bail!(
                        "crate skill sources are no longer referenced from a skill group; \
                         reference the crate's plugin with a `[[plugins]] source.cargo = \"...\"` \
                         entry instead"
                    );
                }

                let exclusive_count = fields.path.is_some() as u8 + fields.git.is_some() as u8;
                if exclusive_count > 1 {
                    bail!("source.path and source.git are mutually exclusive");
                }

                Ok(match (fields.path, fields.git) {
                    (Some(p), None) => PluginSource::Path(p),
                    (None, Some(url)) => PluginSource::Git(url),
                    (None, None) => bail!("a skill group `source` must set `path` or `git`"),
                    _ => unreachable!("exclusive_count > 1 guard"),
                })
            }
        }
    }
}

/// Default subdirectory used when no `[package.metadata.symposium]` is present.
pub const CRATE_DEFAULT_SKILLS_PATH: &str = "skills";

/// Default location for skills that apply while *maintaining* a workspace
/// (as opposed to using its published crates): the `workspace-member()`-gated
/// second default skill group.
pub const AGENTS_SKILLS_PATH: &str = ".agents/skills";

impl serde::Serialize for PluginSource {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self {
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
        }
    }
}

/// A `[[skills]]` entry from a plugin manifest.
///
/// The group's `depends-on` and `predicates` fields are merged into one
/// [`PredicateSet`](crate::predicate::PredicateSet) that gates the group.
#[derive(Debug, Clone, Serialize)]
pub struct SkillGroup {
    #[serde(
        default,
        skip_serializing_if = "crate::predicate::PredicateSet::is_empty"
    )]
    pub predicates: crate::predicate::PredicateSet,
    /// Remote source for skills.
    #[serde(default)]
    pub source: PluginSource,
    /// The group is defined by a workspace-member plugin. Provenance, stamped
    /// during manifest validation, not manifest content: workspace skills are
    /// informal, so their SKILL.md `name` defaults to the skill directory's
    /// name and `description` (with the frontmatter itself) is optional.
    #[serde(skip)]
    pub workspace_member: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSkillGroup {
    #[serde(default, rename = "depends-on")]
    depends_on: Option<crate::predicate::DependsOnList>,
    /// Rejected: renamed to `depends-on`.
    #[serde(default)]
    crates: Option<toml::Value>,
    #[serde(default)]
    predicates: crate::predicate::PredicateSet,
    #[serde(default)]
    source: Option<RawPluginSource>,
}

impl RawSkillGroup {
    fn validate(self) -> Result<SkillGroup> {
        reject_crates_field(&self.crates)?;
        let source = self
            .source
            .context("a `[[skills]]` group must set `source.path` or `source.git`")?
            .validate()?;
        Ok(SkillGroup {
            predicates: crate::predicate::PredicateSet::merged(self.depends_on, self.predicates),
            source,
            workspace_member: false,
        })
    }
}

/// A raw `[[plugins]]` entry.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawChainedPlugin {
    #[serde(default, rename = "depends-on")]
    depends_on: Option<crate::predicate::DependsOnList>,
    #[serde(default)]
    predicates: crate::predicate::PredicateSet,
    source: RawChainedSource,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawChainedSource {
    /// Dependency-atom string (`source.cargo = "widget>=1"`) or explicit
    /// table (`source.cargo = { name = "widget", version = ">=1" }`).
    #[serde(default)]
    cargo: Option<RawChainedCargo>,
    /// Not yet implemented — reserved so the error is a clear message rather
    /// than an unknown-field parse failure.
    #[serde(default)]
    git: Option<toml::Value>,
    /// Not yet implemented — reserved like `git`.
    #[serde(default)]
    path: Option<toml::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawChainedCargo {
    Atom(String),
    Table(RawChainedCargoTable),
    /// Anything else — rejected with a migration hint.
    Other(toml::Value),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawChainedCargoTable {
    name: String,
    #[serde(default)]
    version: Option<String>,
}

impl RawChainedPlugin {
    fn validate(self) -> Result<ChainedPlugin> {
        if self.source.git.is_some() || self.source.path.is_some() {
            bail!(
                "[[plugins]] currently supports only `source.cargo`; \
                 git and path chained plugins are not yet implemented"
            );
        }
        let Some(cargo) = self.source.cargo else {
            bail!(
                "[[plugins]] entry needs `source.cargo = \"<crate><version req>\"` \
                 or `source.cargo = {{ name = \"...\", version = \"...\" }}`"
            );
        };
        let (name, version) = match cargo {
            RawChainedCargo::Atom(atom) => match crate::predicate::parse_dep_atom(&atom)? {
                crate::predicate::Predicate::DependsOn(name, req) => {
                    (name, req.map(|r| r.to_string()))
                }
                _ => bail!("[[plugins]] source.cargo needs a crate name, not `*`"),
            },
            RawChainedCargo::Table(t) => {
                if t.name.is_empty() {
                    bail!("[[plugins]] source.cargo `name` is empty");
                }
                let version = t.version.filter(|v| !v.is_empty() && v.as_str() != "*");
                if let Some(req) = &version {
                    semver::VersionReq::parse(req).with_context(|| {
                        format!(
                            "[[plugins]] source.cargo version {req:?} is not a valid version requirement"
                        )
                    })?;
                }
                (t.name, version)
            }
            RawChainedCargo::Other(v) => {
                if let toml::Value::Table(t) = &v
                    && let Some((name, toml::Value::String(req))) = t.iter().next()
                {
                    bail!(
                        "[[plugins]] source.cargo no longer takes the dependency-table form \
                         `{{ {name} = \"{req}\" }}`; write it as \
                         `source.cargo = {{ name = \"{name}\", version = \"{req}\" }}`"
                    );
                }
                bail!(
                    "[[plugins]] source.cargo takes a dependency atom (`\"widget>=1\"`) \
                     or a `{{ name = \"...\", version = \"...\" }}` table"
                )
            }
        };
        Ok(ChainedPlugin {
            predicates: crate::predicate::PredicateSet::merged(self.depends_on, self.predicates),
            name,
            version,
        })
    }
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

/// A validated custom predicate definition from a `[[predicate]]` entry.
#[derive(Debug, Clone, Serialize)]
pub struct CustomPredicate {
    /// The predicate name (valid identifier, not a builtin).
    pub name: String,
    /// Name of the installation whose binary/script implements this predicate.
    pub command: String,
    /// Static arguments passed before the dynamic raw-arg.
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

    /// The plugin source's root directory on disk. Used to compute a
    /// `source.path` group's base directory and its `path:<rel>` report label.
    pub source_dir: PathBuf,

    /// Whether this plugin is defined by a member of the active workspace.
    /// Provenance, stamped by the loader: registry sources stamp `false`;
    /// the workspace-plugin loader (workspace-local extensions) will stamp
    /// `true`. Backs the `workspace-member()` predicate.
    pub workspace_member: bool,

    /// The plugin's canonical package identity. Set to the resolved crate id for
    /// a plugin loaded through a `[[plugins]] source.cargo` chained reference (a
    /// crate carrying its own manifest). Registry and workspace plugins have no
    /// real package identity, so this is a placeholder id tagged with the source
    /// name (registry) or `"local"` (workspace).
    ///
    /// Used only to key chained-plugin cycle/diamond detection on the normalized
    /// crate name (see `skills::expand_chained_plugins`). It does *not* affect
    /// skill identity — that is the `SKILL.md` path hash.
    ///
    /// FIXME: the registry/workspace placeholder `pm` tags (`"user-plugins"`,
    /// `"local"`, …) should become real `path` PM ids once that PM lands.
    pub canonical: crate::pm::PackageId,
}

impl ParsedPlugin {
    /// Evaluate the plugin-level predicate set, stamping this plugin's
    /// provenance into the context first. Use this — not
    /// `plugin.applies()` directly — when iterating loaded plugins, so
    /// `workspace-member()` sees the right plugin's provenance. The stamp
    /// carries over to the plugin's nested component evaluations (groups,
    /// skills, hooks, MCP servers, subcommands) on the same context.
    pub fn applies(&self, ctx: &mut crate::predicate::PredicateContext) -> bool {
        ctx.set_workspace_member(self.workspace_member);
        self.plugin.applies(ctx)
    }
}

/// A loaded, *validated* plugin manifest.
///
/// This is a table of contents — it describes what skills and hooks are
/// available, but does not load skill content. The skills layer handles
/// discovery and loading.
#[derive(Debug, Clone, Serialize)]
pub struct Plugin {
    pub name: String,
    /// Activation predicates for this plugin — the plugin's `depends-on`
    /// (lowered to `any(depends-on(...))`) merged with its `predicates`. Holds
    /// when every entry holds. Evaluated at sync time (for skills/MCP), at
    /// subcommand lookup, and at hook dispatch.
    pub predicates: crate::predicate::PredicateSet,
    /// Named installation entries available to hooks in this plugin.
    /// Order matches declaration order in the manifest.
    pub installations: Vec<Installation>,
    pub hooks: Vec<Hook>,
    pub skills: Vec<SkillGroup>,
    /// MCP servers to register for this plugin.
    pub mcp_servers: Vec<PluginMcpServer>,
    /// Subcommands vended by this plugin, keyed by the name the user types
    /// after `cargo agents`. Empty for plugins that vend no subcommands.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub subcommands: std::collections::BTreeMap<String, Subcommand>,
    /// Custom predicate definitions vended by this plugin.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_predicates: Vec<CustomPredicate>,
    /// Chained plugin references (`[[plugins]]`): whenever this plugin is
    /// active and any per-edge predicates hold, the referenced plugin loads
    /// too. Expanded during skill resolution by `skills::expand_chained_plugins`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chained: Vec<ChainedPlugin>,
}

/// A validated `[[plugins]]` chained reference: whenever the owning plugin is
/// active and `predicates` hold, the referenced plugin is loaded too. Only
/// `source.cargo` (a crate carrying plugin content) is representable today.
#[derive(Debug, Clone, Serialize)]
pub struct ChainedPlugin {
    /// Predicates gating this edge, on top of the owning plugin's own gate.
    #[serde(skip_serializing_if = "crate::predicate::PredicateSet::is_empty")]
    pub predicates: crate::predicate::PredicateSet,
    /// Crate carrying the chained plugin content (`source.cargo`).
    pub name: String,
    /// Version requirement, if given. Recorded but not yet enforced — the crate
    /// always resolves against the workspace (pin / path override), whether or
    /// not this is set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl Plugin {
    /// Check if this plugin's activation predicates hold in `ctx`.
    pub fn applies(&self, ctx: &mut crate::predicate::PredicateContext) -> bool {
        self.predicates.evaluate(ctx)
    }

    /// Look up a named installation on this plugin.
    pub fn get_installation(&self, name: &str) -> Option<&Installation> {
        self.installations.iter().find(|i| i.name == name)
    }

    /// True if gating this plugin's hooks (plugin-level plus hook-level
    /// predicates) needs the workspace crate graph — i.e. some predicate names a
    /// concrete crate, not just `depends-on(*)`. Lets hook dispatch skip the cargo
    /// query when no crate is actually referenced.
    pub fn hooks_need_dep_resolution(&self) -> bool {
        self.predicates.has_concrete_dep()
            || self.hooks.iter().any(|h| h.predicates.has_concrete_dep())
    }

    /// Return MCP servers whose own predicates hold in `ctx`.
    ///
    /// ANDed with the plugin-level predicates, which the caller checks
    /// separately.
    pub fn applicable_mcp_servers(
        &self,
        ctx: &mut crate::predicate::PredicateContext,
    ) -> Vec<McpServerEntry> {
        self.mcp_servers
            .iter()
            .filter(|s| s.predicates.evaluate(ctx))
            .map(|s| s.server.clone())
            .collect()
    }
}

/// Whether a subcommand is intended for human or agent use.
///
/// Controls grouping in `cargo agents --help`; does not gate dispatch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Audience {
    Humans,
    #[default]
    Agents,
}

/// A validated `[subcommand.<name>]` entry.
///
/// `command` is the name of an `Installation` in the plugin (possibly a
/// synthetic one promoted from an inline declaration), matching the same
/// resolution pattern as `Hook.command`.
#[derive(Debug, Clone, Serialize)]
pub struct Subcommand {
    pub description: String,
    pub audience: Audience,
    pub command: String,
    /// Activation predicates for this subcommand (its `depends-on` lowered and
    /// merged with its `predicates`). ANDed with the plugin-level set.
    #[serde(
        default,
        skip_serializing_if = "crate::predicate::PredicateSet::is_empty"
    )]
    pub predicates: crate::predicate::PredicateSet,
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
    /// Activation predicates that must all hold for this hook to dispatch.
    /// Evaluated at dispatch time, ANDed with the plugin's predicates.
    #[serde(
        default,
        skip_serializing_if = "crate::predicate::PredicateSet::is_empty"
    )]
    pub predicates: crate::predicate::PredicateSet,
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
        predicates,
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
        predicates,
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

/// A standalone skill in the registry, paired with the origin it should be
/// attributed to (derived from the source name + the skill's path within
/// that source, so two registries can each contribute a same-named
/// standalone skill without colliding).
#[derive(Debug, Clone)]
pub struct StandaloneSkill {
    pub skill: crate::skills::Skill,
    pub origin_hash: String,
}

/// A resolved custom predicate definition in the registry.
///
/// Stores the plugin index and predicate index within that plugin so that
/// acquisition can look up the `Installation` later.
#[derive(Debug, Clone)]
pub struct ResolvedCustomPredicate {
    /// Index into `PluginRegistry.plugins` for the owning plugin.
    pub plugin_index: usize,
    /// The command installation name on the owning plugin.
    pub command: String,
    /// Static args passed before the dynamic raw-arg.
    pub args: Vec<String>,
}

/// Global registry of custom predicates collected from all plugins.
///
/// Built unconditionally from every plugin's `[[predicate]]` entries (regardless
/// of whether the plugin is "active" in the current workspace). Collisions
/// (same name from two plugins) are excluded and warned at load time.
#[derive(Debug, Default)]
pub struct CustomPredicateRegistry {
    entries: std::collections::HashMap<String, ResolvedCustomPredicate>,
}

impl CustomPredicateRegistry {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn get(&self, name: &str) -> Option<&ResolvedCustomPredicate> {
        self.entries.get(name)
    }

    pub fn contains_key(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &ResolvedCustomPredicate)> {
        self.entries.iter()
    }
}

/// Loaded plugin registry: plugins from TOML manifests and standalone skills
/// discovered directly in plugin source directories.
#[derive(Debug)]
pub struct PluginRegistry {
    /// Plugins loaded from `.toml` manifest files.
    pub plugins: Vec<ParsedPlugin>,
    /// Skills discovered as standalone directories containing a `SKILL.md`
    /// file directly in a plugin source directory (no TOML manifest needed).
    pub standalone_skills: Vec<StandaloneSkill>,
    /// Non-fatal load warnings for plugins or standalone skills that were skipped.
    pub warnings: Vec<LoadWarning>,
    /// Global custom predicate registry. Built from all plugins' `custom_predicates`.
    pub custom_predicates: CustomPredicateRegistry,
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

/// A `[[predicate]]` entry in the raw TOML manifest.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCustomPredicate {
    name: String,
    /// Named installation or inline installation table.
    command: RawInstallationRef,
    #[serde(default)]
    args: Vec<String>,
}

/// `[defaults]` section: opt-outs for the default content added to
/// workspace plugin manifests (and, later, crate-embedded plugins).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDefaults {
    /// Add the default `[[skills]] source.path = "skills"` group.
    #[serde(default = "default_skills_flag")]
    skills: bool,
}

fn default_skills_flag() -> bool {
    true
}

impl Default for RawDefaults {
    fn default() -> Self {
        Self {
            skills: default_skills_flag(),
        }
    }
}

/// Where a plugin manifest came from, for validation rules that differ by
/// origin: a registry manifest must carry its own `name` and must reference
/// at least one dependency; a workspace-member manifest is already gated by
/// workspace membership, and a crate-embedded manifest is already gated by
/// the chained reference that reached it, so both are relaxed (the name
/// defaults to a fallback) and default content applies.
enum ManifestOrigin<'a> {
    Registry,
    WorkspaceMember {
        dir_name: &'a str,
        /// Append the `workspace-member()`-gated `.agents/skills` default
        /// group (the `agents-syncing` config knob).
        agents_skills: bool,
    },
    /// A `SYMPOSIUM.toml` shipped inside a crate, reached through a
    /// `[[plugins]] source.cargo` chained reference. The name defaults to the
    /// crate name; the every-plugin-must-mention-a-dependency rule is waived
    /// (the reference is the gate); the default `skills/` group is appended
    /// (but not the workspace-only `.agents/skills` group).
    Crate {
        crate_name: &'a str,
    },
    /// An entry in a recommendations registry's `cargo/<name>/` directory:
    /// the name defaults to the dependency's, and an implied
    /// `depends-on(<name>)` gate is appended to the plugin predicates (which
    /// is also what satisfies the must-reference-a-dependency rule).
    Recommendations {
        dep_name: &'a str,
    },
}

/// Raw TOML manifest deserialized from a plugin `.toml` file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPluginManifest {
    /// Required for registry plugins; defaults to the directory name for
    /// workspace plugins.
    name: Option<String>,
    /// Default-content opt-outs. Only meaningful for workspace plugins.
    #[serde(default)]
    defaults: Option<RawDefaults>,
    #[serde(default, rename = "depends-on")]
    depends_on: crate::predicate::DependsOnList,
    /// Rejected: renamed to `depends-on`.
    #[serde(default)]
    crates: Option<toml::Value>,
    #[serde(default)]
    predicates: crate::predicate::PredicateSet,
    #[serde(default)]
    installations: Vec<RawNamedInstallation>,
    #[serde(default)]
    hooks: Vec<RawHook>,
    #[serde(default)]
    skills: Vec<RawSkillGroup>,
    #[serde(default)]
    mcp_servers: Vec<RawPluginMcpServer>,
    /// TOML key is singular (`[subcommand.<name>]`); the validated field on
    /// `Plugin` is plural (`subcommands`).
    #[serde(default)]
    subcommand: std::collections::BTreeMap<String, RawSubcommand>,
    #[serde(default)]
    predicate: Vec<RawCustomPredicate>,
    /// Chained plugin references — `[[plugins]]`.
    #[serde(default)]
    plugins: Vec<RawChainedPlugin>,
}

impl RawPluginManifest {
    /// Layer `over` on top of `self`. List-shaped content (skills, chained
    /// plugins, hooks, installations, MCP servers, custom predicates) appends
    /// in `self`-then-`over` order; the `subcommand` map and scalar fields take
    /// `over` where it sets them; `depends-on` / `predicates` gates AND
    /// together. Used to combine a crate's `[package.metadata.symposium]` (base)
    /// with its `SYMPOSIUM.toml` (over).
    fn merge(mut self, over: RawPluginManifest) -> RawPluginManifest {
        self.installations.extend(over.installations);
        self.hooks.extend(over.hooks);
        self.skills.extend(over.skills);
        self.mcp_servers.extend(over.mcp_servers);
        self.predicate.extend(over.predicate);
        self.plugins.extend(over.plugins);
        self.subcommand.extend(over.subcommand);
        self.depends_on.0.extend(over.depends_on.0);
        self.predicates
            .predicates
            .extend(over.predicates.predicates);
        if over.name.is_some() {
            self.name = over.name;
        }
        if over.defaults.is_some() {
            self.defaults = over.defaults;
        }
        if over.crates.is_some() {
            self.crates = over.crates;
        }
        self
    }
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

/// Raw `[subcommand.<name>]` entry. The TOML table-key is the subcommand
/// name; this struct carries the table body.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSubcommand {
    description: String,
    #[serde(default)]
    audience: Audience,
    /// Named installation (`"my-install"`) or inline installation table —
    /// same shape as `RawHook.command`.
    command: RawInstallationRef,
    #[serde(default, rename = "depends-on")]
    depends_on: Option<crate::predicate::DependsOnList>,
    /// Rejected: renamed to `depends-on`.
    #[serde(default)]
    crates: Option<toml::Value>,
    #[serde(default)]
    predicates: crate::predicate::PredicateSet,
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
    predicates: crate::predicate::PredicateSet,
}

/// Fetch/update git-based registries.
///
/// `update` controls freshness checking behavior (see `UpdateLevel`).
/// Only refreshes registries with `auto-update = true` (unless `update` is
/// `Fetch`). Path-based registries are skipped (no fetching needed).
pub async fn ensure_registries(sym: &Symposium, update: UpdateLevel) {
    for resolved in &sym.registries() {
        let registry = &resolved.registry;
        if !matches!(update, UpdateLevel::Fetch) && !registry.auto_update {
            tracing::debug!(registry = %registry.name, "skipping (auto-update disabled)");
            continue;
        }

        let Some(ref git_url) = registry.git else {
            tracing::debug!(registry = %registry.name, "skipping (can only auto-update git)");
            continue;
        };

        tracing::debug!(registry = %registry.name, url = %git_url, "ensuring registry");

        match fetch_registry(sym, git_url, update).await {
            Ok(path) => {
                tracing::debug!(registry = %registry.name, path = %path.display(), "registry ready");
            }
            Err(e) => {
                tracing::warn!(registry = %registry.name, git_url = %git_url, error = %e, "failed to fetch registry");
            }
        }
    }
}

/// Load all plugins from all configured registries plus the active
/// workspace, discarding load errors with warnings.
///
/// Use `load_registry_with_workspace()` instead if you also need standalone
/// skills.
pub async fn load_all_plugins(
    sym: &Symposium,
    workspace: Option<&symposium_sdk::workspace::LoadedWorkspace>,
) -> Vec<ParsedPlugin> {
    load_registry_impl(sym, workspace).await.plugins
}

/// Refresh registry content.
///
/// If `provider` is Some, sync only that registry (ignores auto-update).
/// If `provider` is None, sync all registries with auto-update = true.
pub async fn sync_registries(sym: &Symposium, provider: Option<&str>) -> Result<Vec<String>> {
    let mut synced = Vec::new();

    for resolved in &sym.registries() {
        let registry = &resolved.registry;
        if let Some(name) = provider {
            if registry.name != name {
                continue;
            }
        } else if !registry.auto_update {
            tracing::debug!(registry = %registry.name, "skipping (auto-update disabled)");
            continue;
        }

        if let Some(ref git_url) = registry.git {
            tracing::debug!(registry = %registry.name, url = %git_url, "syncing registry");
            match fetch_registry(sym, git_url, UpdateLevel::Fetch).await {
                Ok(path) => {
                    tracing::info!(registry = %registry.name, path = %path.display(), "synced");
                    synced.push(registry.name.clone());
                }
                Err(e) => {
                    tracing::warn!(registry = %registry.name, error = %e, "failed to sync");
                }
            }
        } else {
            tracing::debug!(registry = %registry.name, "skipping path-based registry");
        }
    }

    Ok(synced)
}

/// List all providers and their plugins. Routed through the same package
/// managers as registry loading, so what `plugin list` shows can't diverge
/// from what `sync` sees.
pub async fn list_plugins(sym: &Symposium) -> Vec<ProviderInfo> {
    let pms = sym.package_managers();
    let pm_cx = registry_pm_cx(sym);
    let mut by_registry: std::collections::HashMap<String, Vec<PluginInfo>> =
        std::collections::HashMap::new();

    let (offers, _warnings) = list_plugin_offers(&pms, &pm_cx).await;
    for offer in offers {
        let Some(OfferItem::Plugin(Ok(p))) = load_offer(&offer) else {
            continue;
        };
        by_registry
            .entry(offer.registry_name.clone())
            .or_default()
            .push(PluginInfo {
                name: p.plugin.name,
                hooks_count: p.plugin.hooks.len(),
                skill_groups_count: p.plugin.skills.len(),
            });
    }

    sym.registries()
        .into_iter()
        .map(|resolved| {
            let registry = resolved.registry;
            ProviderInfo {
                plugins: by_registry.remove(&registry.name).unwrap_or_default(),
                name: registry.name,
                source_type: if registry.git.is_some() {
                    "git"
                } else {
                    "path"
                },
                git_url: registry.git,
                path: registry.path,
            }
        })
        .collect()
}

/// Find a plugin by name across all registries. First match wins.
pub async fn find_plugin(sym: &Symposium, name: &str) -> Option<ParsedPlugin> {
    let pms = sym.package_managers();
    let pm_cx = registry_pm_cx(sym);
    let (offers, _warnings) = list_plugin_offers(&pms, &pm_cx).await;
    for offer in offers {
        if let Some(OfferItem::Plugin(Ok(parsed_plugin))) = load_offer(&offer)
            && parsed_plugin.plugin.name == name
        {
            return Some(*parsed_plugin);
        }
    }
    None
}

/// Fetch a git registry's repository, returning the cached directory path.
async fn fetch_registry(sym: &Symposium, git_url: &str, update: UpdateLevel) -> Result<PathBuf> {
    let cache_mgr = symposium_install::git::GitCacheManager::new(
        &sym.install_context(),
        crate::config::REGISTRY_CACHE_SUBDIR,
    );
    cache_mgr.fetch_url(git_url, update).await
}

/// PM context for registry operations: these are workspace independent, so
/// the dependency list is empty.
fn registry_pm_cx(sym: &Symposium) -> crate::pm::PmContext<'static> {
    crate::pm::PmContext {
        install: sym.install_context(),
        workspace_crates: &[],
    }
}

/// One package offered by a PM instance, located on disk: the instance's
/// display name (origin attribution), the source content root, the package's
/// directory within it, and — for recommendations entries — the dependency
/// it recommends a plugin for.
struct PluginOffer {
    registry_name: String,
    /// Registry content root. `ParsedPlugin::source_dir` — the base for a
    /// plugin's `source.path` groups — is always this, not the entry dir.
    root: PathBuf,
    /// The package's directory relative to `root`.
    subpath: PathBuf,
    recommends: Option<String>,
}

impl PluginOffer {
    /// The directory holding this offer's package.
    fn dir(&self) -> PathBuf {
        self.root.join(&self.subpath)
    }
}

/// Every package offered by the active PM instances, located on disk, plus
/// warnings for instances that could not be listed (e.g. a misconfigured
/// registry whose root is itself a plugin).
///
/// Does no network I/O — `list_plugins` and `cached_root` both serve from
/// what is already on disk; a git registry that was never fetched offers
/// nothing.
async fn list_plugin_offers(
    pms: &crate::pm::PmRegistry,
    pm_cx: &crate::pm::PmContext<'_>,
) -> (Vec<PluginOffer>, Vec<LoadWarning>) {
    let mut offers = Vec::new();
    let mut warnings = Vec::new();
    for inst in pms.instances() {
        let infos = match inst.pm.list_plugins(&[], pm_cx).await {
            Ok(infos) => infos,
            Err(e) => {
                tracing::warn!(registry = %inst.name, error = %e, "cannot list registry");
                warnings.push(LoadWarning {
                    path: PathBuf::new(),
                    message: format!("cannot list registry `{}`: {e:#}", inst.name),
                });
                continue;
            }
        };
        for info in infos {
            // Registry loading only consumes positional registry entries;
            // offers without a subpath (dependency-embedded crates) are the
            // chained-plugin flow's concern.
            let Some(subpath) = info.subpath else {
                continue;
            };
            let Some(entry_dir) = inst.pm.cached_root(&info.id, pm_cx) else {
                tracing::warn!(registry = %inst.name, id = %info.id, "cannot locate plugin package");
                continue;
            };
            // The instance built the entry dir as `<content root>/<subpath>`;
            // peel the subpath back off to recover the attribution root.
            let mut root = entry_dir;
            for _ in subpath.components() {
                root.pop();
            }
            offers.push(PluginOffer {
                registry_name: inst.name.clone(),
                root,
                subpath,
                recommends: info.recommends,
            });
        }
    }
    (offers, warnings)
}

/// One loaded package: a plugin manifest or a standalone skill.
enum OfferItem {
    Plugin(Result<Box<ParsedPlugin>>),
    Skill(PathBuf),
}

/// Interpret one offer's package. A recommendations entry loads with the
/// recommended dependency as its gate and default name; any other entry is
/// an ordinary registry plugin or standalone skill. `None` when the
/// directory is neither (e.g. it disappeared since it was listed).
fn load_offer(offer: &PluginOffer) -> Option<OfferItem> {
    let dir = offer.dir();
    match crate::pm::layout::classify(&dir)? {
        crate::pm::layout::EntryKind::Plugin(toml_path) => {
            let origin = match offer.recommends.as_deref() {
                Some(dep_name) => ManifestOrigin::Recommendations { dep_name },
                None => ManifestOrigin::Registry,
            };
            Some(OfferItem::Plugin(
                load_plugin_as(&toml_path, &offer.registry_name, &offer.root, origin)
                    .map(Box::new)
                    .with_context(|| format!("loading plugin from `{}`", toml_path.display())),
            ))
        }
        crate::pm::layout::EntryKind::Skill(skill_md) => Some(OfferItem::Skill(skill_md)),
    }
}

/// Load the plugin registry from the active package-manager instances.
///
/// Each registry instance lists the plugin-bearing entries it offers
/// (`list_plugins`, no network), and each entry is loaded as a plugin
/// manifest or a standalone skill. Refreshing git registries is a separate
/// concern ([`ensure_registries`]).
///
/// This form loads registries only; workspace-scoped callers use
/// [`load_registry_with_workspace`] to also pick up plugins defined by the
/// active workspace.
pub async fn load_registry(sym: &Symposium) -> PluginRegistry {
    load_registry_impl(sym, None).await
}

/// [`load_registry`] plus the plugins defined by the active workspace (the
/// workspace root and every member directory), stamped as workspace
/// members. `None` (not in a workspace) degrades to registries only.
pub async fn load_registry_with_workspace(
    sym: &Symposium,
    workspace: Option<&symposium_sdk::workspace::LoadedWorkspace>,
) -> PluginRegistry {
    load_registry_impl(sym, workspace).await
}

async fn load_registry_impl(
    sym: &Symposium,
    workspace: Option<&symposium_sdk::workspace::LoadedWorkspace>,
) -> PluginRegistry {
    let pms = sym.package_managers();
    let pm_cx = registry_pm_cx(sym);
    let mut plugins = Vec::new();
    let mut standalone_skills = Vec::new();

    let (offers, mut warnings) = list_plugin_offers(&pms, &pm_cx).await;
    for offer in offers {
        match load_offer(&offer) {
            Some(OfferItem::Plugin(Ok(p))) => plugins.push(*p),
            Some(OfferItem::Plugin(Err(e))) => {
                tracing::warn!(error = %e, "failed to load plugin");
                warnings.push(LoadWarning {
                    path: offer.dir().join(crate::pm::layout::MANIFEST_FILE),
                    message: format!("failed to load plugin: {e}"),
                });
            }
            Some(OfferItem::Skill(skill_md)) => {
                match crate::skills::load_standalone_skill(&skill_md) {
                    Ok(skill) => {
                        let origin_hash = skill_origin_hash(&skill_md);
                        standalone_skills.push(StandaloneSkill { skill, origin_hash });
                    }
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
            None => {}
        }
    }

    if let Some(ws) = workspace {
        let (ws_plugins, ws_warnings) =
            workspace_plugins(&ws.root, &ws.members, sym.config.agents_syncing);
        plugins.extend(ws_plugins);
        warnings.extend(ws_warnings);
    }

    tracing::debug!(
        plugins = plugins.len(),
        standalone_skills = standalone_skills.len(),
        "plugin registry loaded"
    );

    let custom_predicates = build_custom_predicate_registry(&plugins, &mut warnings);

    PluginRegistry {
        plugins,
        standalone_skills,
        warnings,
        custom_predicates,
    }
}

/// Display name workspace plugins are attributed to. Parenthesized so it
/// can't collide with a configured plugin-source name.
/// Load the plugins defined by the active workspace: the workspace root
/// plus every member package directory.
///
/// A directory defines a workspace plugin when it has a `SYMPOSIUM.toml`
/// manifest (whose `name` defaults to the directory name) or a `skills/`
/// directory (a manifest-less plugin whose only content is the default
/// skills group). Default content — the `[[skills]] source.path = "skills"`
/// group — is appended unless the manifest opts out with
/// `[defaults] skills = false`.
///
/// Workspace plugins are stamped `workspace_member = true` (the producer of
/// the `workspace-member()` predicate) and attributed to the
/// `"(workspace)"` source with skill paths relative to the workspace root,
/// so equal-named skills in different members stay distinct.
pub fn workspace_plugins(
    root: &Path,
    members: &[PathBuf],
    agents_skills: bool,
) -> (Vec<ParsedPlugin>, Vec<LoadWarning>) {
    let mut seen = std::collections::HashSet::new();
    let mut plugins = Vec::new();
    let mut warnings = Vec::new();
    for dir in std::iter::once(&root.to_path_buf()).chain(members.iter()) {
        let dir = fs::canonicalize(dir).unwrap_or_else(|_| dir.clone());
        if !seen.insert(dir.clone()) {
            continue;
        }
        match workspace_plugin_for_dir(root, &dir, agents_skills) {
            Ok(Some(parsed)) => plugins.push(parsed),
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(dir = %dir.display(), error = %e, "failed to load workspace plugin");
                warnings.push(LoadWarning {
                    path: dir.join("SYMPOSIUM.toml"),
                    message: format!("failed to load workspace plugin: {e}"),
                });
            }
        }
    }
    (plugins, warnings)
}

/// Interpret one workspace directory as a plugin, or `None` when the
/// directory defines nothing (no manifest, no `skills/`).
fn workspace_plugin_for_dir(
    workspace_root: &Path,
    dir: &Path,
    agents_skills: bool,
) -> Result<Option<ParsedPlugin>> {
    let manifest_path = dir.join("SYMPOSIUM.toml");
    let bare_convention = dir.join(CRATE_DEFAULT_SKILLS_PATH).is_dir()
        || (agents_skills && dir.join(AGENTS_SKILLS_PATH).is_dir());
    let raw: RawPluginManifest = if manifest_path.is_file() {
        toml::from_str(&fs::read_to_string(&manifest_path)?)?
    } else if bare_convention {
        // Bare convention: a `skills/` (or `.agents/skills/`) directory with
        // no manifest is an all-defaults plugin.
        toml::from_str("").expect("empty manifest parses")
    } else {
        return Ok(None);
    };

    let dir_name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace");
    let plugin = validate_manifest(
        raw,
        ManifestOrigin::WorkspaceMember {
            dir_name,
            agents_skills,
        },
    )
    .with_context(|| format!("validating `{}`", manifest_path.display()))?;

    Ok(Some(ParsedPlugin {
        canonical: PackageId::new("local", &plugin.name, ANY_VERSION),
        path: manifest_path,
        plugin,
        source_dir: workspace_root.to_path_buf(),
        workspace_member: true,
    }))
}

/// Scan a directory laid out like a plugin source, loading its plugin
/// manifests and collecting its standalone skills.
///
/// Entry discovery is the [flat layout](crate::pm::layout): a directory with
/// a `SYMPOSIUM.toml` is a plugin, one with a `SKILL.md` is a standalone
/// skill (manifest wins when both are present), and a claimed directory is
/// not recursed into.
///
/// This is the *offline* form used by the `plugin validate` CLI, which
/// points at an arbitrary directory rather than a configured registry.
/// Registry loading goes through the package-manager instances instead
/// ([`load_registry`]). `source_name` becomes each `ParsedPlugin`'s
/// canonical `pm` tag; callers that don't care pass `""`.
fn scan_source_dir<P: AsRef<Path>>(dir: P, source_name: &str) -> Result<SourceDirContents> {
    let dir = dir.as_ref();
    let mut plugins = Vec::new();
    let mut skill_files = Vec::new();

    for entry in crate::pm::layout::enumerate(dir)? {
        match crate::pm::layout::classify(&dir.join(&entry.subpath)) {
            Some(crate::pm::layout::EntryKind::Plugin(toml_path)) => {
                let plugin = load_plugin(&toml_path, source_name, dir)
                    .with_context(|| format!("loading plugin from `{}`", toml_path.display()));
                tracing::debug!(path = %toml_path.display(), plugin = ?plugin, "loaded plugin");
                plugins.push(plugin);
            }
            Some(crate::pm::layout::EntryKind::Skill(skill_md_path)) => {
                tracing::debug!(path = %skill_md_path.display(), "found standalone skill");
                skill_files.push(skill_md_path);
            }
            None => {}
        }
    }

    Ok(SourceDirContents {
        plugins,
        skill_files,
    })
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
    let contents = scan_source_dir(dir, "")?;
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
                    let found = crate::skills::discover_skills(
                        &skills_dir,
                        group.workspace_member,
                        &group.predicates,
                    );
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
/// Scans TOML plugin manifests (skill group `depends-on`) and
/// standalone SKILL.md files, returning deduplicated crate names.
/// Items that fail to load are silently skipped.
pub fn collect_crate_names_in_source_dir(dir: &Path) -> Result<Vec<String>> {
    let contents = scan_source_dir(dir, "")?;
    let mut names = std::collections::BTreeSet::new();

    for plugin_result in contents.plugins.into_iter().flatten() {
        plugin_result
            .plugin
            .predicates
            .collect_dep_names(&mut names);
        for group in &plugin_result.plugin.skills {
            group.predicates.collect_dep_names(&mut names);
        }
        for mcp in &plugin_result.plugin.mcp_servers {
            mcp.predicates.collect_dep_names(&mut names);
        }
    }

    for skill_md in contents.skill_files {
        if let Ok(skill) = crate::skills::load_standalone_skill(&skill_md) {
            skill.predicates.collect_dep_names(&mut names);
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
///
/// `source_name` and `source_dir` describe the plugin source the manifest was
/// found through. `source_name` becomes the plugin's canonical `pm` tag;
/// `source_dir` is the base for its `source.path` groups. Standalone callers —
/// like the `plugin validate` CLI — that need neither can pass an empty string
/// and the manifest's parent directory.
pub fn load_plugin(
    manifest_path: &Path,
    source_name: &str,
    source_dir: &Path,
) -> Result<ParsedPlugin> {
    load_plugin_as(
        manifest_path,
        source_name,
        source_dir,
        ManifestOrigin::Registry,
    )
}

/// [`load_plugin`] with an explicit manifest origin — the entry position
/// within its registry decides the validation rules (a recommendations
/// `cargo/<name>/` entry gains an implied gate and default name).
fn load_plugin_as(
    manifest_path: &Path,
    source_name: &str,
    source_dir: &Path,
    origin: ManifestOrigin<'_>,
) -> Result<ParsedPlugin> {
    let content = fs::read_to_string(manifest_path)?;
    let manifest: RawPluginManifest = toml::from_str(&content)?;
    let plugin = validate_manifest(manifest, origin)
        .with_context(|| format!("validating `{}`", manifest_path.display()))?;
    Ok(ParsedPlugin {
        canonical: PackageId::new(source_name, &plugin.name, ANY_VERSION),
        path: manifest_path.to_path_buf(),
        plugin,
        source_dir: source_dir.to_path_buf(),
        // Registry sources are never workspace members; the workspace-plugin
        // loader is the only place that stamps true.
        workspace_member: false,
    })
}

fn raw_crate_manifest(content: &str) -> Result<RawPluginManifest> {
    Ok(toml::from_str(content)?)
}

/// Build a crate's plugin definition by layering its manifest sources.
///
/// A crate can describe its plugin two ways, and this combines them (later
/// layers win / append, matching the merge order defaults → Cargo.toml →
/// `SYMPOSIUM.toml`):
/// 1. the crate defaults (the default `skills/` group, appended by
///    [`validate_manifest`] under [`ManifestOrigin::Crate`]) — the base;
/// 2. `[package.metadata.symposium]` from `Cargo.toml` (`metadata`);
/// 3. a `SYMPOSIUM.toml` file at the crate root (`file`).
///
/// Both `metadata` and `file` use the same schema as any plugin manifest. Each
/// is parsed independently and **leniently**: a malformed layer is logged and
/// dropped so the crate still resolves through the remaining layers (and, at
/// minimum, the default `skills/` group). A crate with neither still becomes a
/// plugin whose only content is that default group — so `load_plugin` always
/// yields a plugin for a fetchable crate.
pub(crate) fn load_crate_manifest(
    metadata: Option<toml::Table>,
    file: Option<&str>,
    crate_name: &str,
) -> Result<Plugin> {
    let meta = metadata.and_then(
        |t| match toml::Value::Table(t).try_into::<RawPluginManifest>() {
            Ok(m) => Some(m),
            Err(e) => {
                tracing::warn!(
                    crate_name = %crate_name,
                    error = %e,
                    "ignoring malformed [package.metadata.symposium]"
                );
                None
            }
        },
    );
    let file = file.and_then(|c| match raw_crate_manifest(c) {
        Ok(m) => Some(m),
        Err(e) => {
            tracing::warn!(
                crate_name = %crate_name,
                error = %e,
                "ignoring malformed crate SYMPOSIUM.toml"
            );
            None
        }
    });
    let merged = match (meta, file) {
        (Some(a), Some(b)) => a.merge(b),
        (Some(m), None) | (None, Some(m)) => m,
        (None, None) => raw_crate_manifest("")?,
    };
    validate_manifest(merged, ManifestOrigin::Crate { crate_name })
}

/// Convert a raw manifest into a validated `Plugin`.
///
/// User-declared `[[installations]]` come first in the resulting list, in
/// declaration order. Inline references on installations and hooks are
/// promoted into synthetic entries appended to the same list so that every
/// validated reference is a plain name.
fn validate_manifest(
    mut manifest: RawPluginManifest,
    origin: ManifestOrigin<'_>,
) -> Result<Plugin> {
    let name = match (manifest.name.take(), &origin) {
        (Some(n), _) => n,
        (None, ManifestOrigin::WorkspaceMember { dir_name, .. }) => dir_name.to_string(),
        (None, ManifestOrigin::Crate { crate_name }) => crate_name.to_string(),
        (None, ManifestOrigin::Recommendations { dep_name }) => dep_name.to_string(),
        (None, ManifestOrigin::Registry) => bail!("plugin manifest is missing `name`"),
    };
    match &origin {
        ManifestOrigin::Registry | ManifestOrigin::Recommendations { .. } => {
            if manifest.defaults.is_some() {
                bail!("`[defaults]` is only supported in workspace and crate plugin manifests");
            }
        }
        ManifestOrigin::WorkspaceMember { agents_skills, .. } => {
            let defaults = manifest.defaults.take().unwrap_or_default();
            if defaults.skills {
                let group: RawSkillGroup =
                    toml::from_str(r#"source.path = "skills""#).expect("static default group");
                manifest.skills.push(group);
                if *agents_skills {
                    let group: RawSkillGroup = toml::from_str(indoc::indoc! {r#"
                        predicates = ["workspace-member()"]
                        source.path = ".agents/skills"
                    "#})
                    .expect("static default group");
                    manifest.skills.push(group);
                }
            }
        }
        ManifestOrigin::Crate { .. } => {
            // A crate is a dependency, not a workspace member: it gets the
            // default `skills/` group (so a bare `SYMPOSIUM.toml` doesn't
            // silently drop skills the metadata path would have found), but
            // not the workspace-only `.agents/skills` maintainer group.
            let defaults = manifest.defaults.take().unwrap_or_default();
            if defaults.skills {
                let group: RawSkillGroup =
                    toml::from_str(r#"source.path = "skills""#).expect("static default group");
                manifest.skills.push(group);
            }
        }
    }

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

    let mut subcommands = std::collections::BTreeMap::new();
    for (name, raw) in manifest.subcommand {
        let sub = validate_subcommand(name.clone(), raw, &mut installations, &mut names)?;
        subcommands.insert(name, sub);
    }

    let mut custom_predicates = Vec::with_capacity(manifest.predicate.len());
    for raw in manifest.predicate {
        custom_predicates.push(validate_custom_predicate(
            raw,
            &mut installations,
            &mut names,
        )?);
    }

    reject_crates_field(&manifest.crates)?;
    let mut predicates =
        crate::predicate::PredicateSet::merged(Some(manifest.depends_on), manifest.predicates);
    if let ManifestOrigin::Recommendations { dep_name } = &origin {
        // The directory position implies the gate: a `cargo/<name>/` entry
        // applies when the workspace depends on <name> (version ignored, per
        // the recommendations convention).
        predicates
            .predicates
            .push(crate::predicate::Predicate::DependsOn(
                dep_name.to_string(),
                None,
            ));
    }
    let mut skills = manifest
        .skills
        .into_iter()
        .map(RawSkillGroup::validate)
        .collect::<Result<Vec<_>>>()?;
    if matches!(origin, ManifestOrigin::WorkspaceMember { .. }) {
        for group in &mut skills {
            group.workspace_member = true;
        }
    }
    let mcp_servers = manifest
        .mcp_servers
        .into_iter()
        .map(RawPluginMcpServer::validate)
        .collect::<Result<Vec<_>>>()?;

    // Every registry plugin must reference at least one dependency (or
    // custom predicate) somewhere — at the plugin, skill-group, hook, or
    // MCP-server level — via `depends-on`, a `depends-on(...)` predicate, or
    // a custom predicate. Otherwise it would never apply to any project.
    // Workspace plugins are exempt: being in the workspace is their gate, as
    // is the implied `depends-on` of a recommendations entry.
    if matches!(origin, ManifestOrigin::Registry) {
        let has_custom_predicate = predicates
            .predicates
            .iter()
            .any(|p| matches!(p, crate::predicate::Predicate::Custom { .. }));
        let mentions_dep = has_custom_predicate
            || predicates.mentions_dep()
            || skills.iter().any(|g| g.predicates.mentions_dep())
            || hooks.iter().any(|h| h.predicates.mentions_dep())
            || mcp_servers.iter().any(|m| m.predicates.mentions_dep());
        if !mentions_dep {
            bail!(
                "plugin `{name}` references no dependency — add `depends-on = [...]` or a \
                 `depends-on(...)` predicate at the plugin, `[[skills]]`, or `[[mcp_servers]]` level"
            );
        }
    }

    let chained = manifest
        .plugins
        .into_iter()
        .map(RawChainedPlugin::validate)
        .collect::<Result<Vec<_>>>()?;

    Ok(Plugin {
        name,
        predicates,
        installations,
        hooks,
        skills,
        mcp_servers,
        subcommands,
        custom_predicates,
        chained,
    })
}

/// Names a plugin subcommand cannot use because they collide with built-in
/// `cargo agents` commands.
const RESERVED_SUBCOMMAND_NAMES: &[&str] = &[
    "init",
    "sync",
    "hook",
    "plugin",
    "crate-info",
    "self-update",
    "help",
];

/// Maximum length (bytes) of a subcommand's `description` field.
const MAX_SUBCOMMAND_DESCRIPTION_LEN: usize = 1024;

/// Validate the user-typed name of a `[subcommand.<name>]` table.
fn validate_subcommand_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("subcommand name is empty");
    }
    if RESERVED_SUBCOMMAND_NAMES.contains(&name) {
        bail!("subcommand `{name}` shadows a built-in `cargo agents` command");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("subcommand `{name}` has invalid characters (allow ASCII alphanumeric, `-`, `_`)");
    }
    Ok(())
}

/// Validate a raw subcommand into a `Subcommand`, promoting any inline
/// `command` into a synthetic installation entry.
fn validate_subcommand(
    name: String,
    raw: RawSubcommand,
    installations: &mut Vec<Installation>,
    names: &mut std::collections::BTreeSet<String>,
) -> Result<Subcommand> {
    validate_subcommand_name(&name)?;

    let RawSubcommand {
        description,
        audience,
        command: raw_command,
        depends_on,
        crates,
        predicates,
    } = raw;
    reject_crates_field(&crates)?;

    if description.len() > MAX_SUBCOMMAND_DESCRIPTION_LEN {
        bail!("subcommand `{name}` description exceeds {MAX_SUBCOMMAND_DESCRIPTION_LEN} chars");
    }

    let command = resolve_or_promote(
        raw_command,
        installations,
        names,
        &mut || name.clone(),
        &format!("subcommand `{name}`"),
    )?;

    Ok(Subcommand {
        description,
        audience,
        command,
        predicates: crate::predicate::PredicateSet::merged(depends_on, predicates),
    })
}

/// Validate a `[[predicate]]` entry, promoting inline `command` if needed.
fn validate_custom_predicate(
    raw: RawCustomPredicate,
    installations: &mut Vec<Installation>,
    names: &mut std::collections::BTreeSet<String>,
) -> Result<CustomPredicate> {
    crate::predicate::validate_custom_predicate_name(&raw.name)?;

    let command = resolve_or_promote(
        raw.command,
        installations,
        names,
        &mut || format!("__pred_{}", raw.name),
        &format!("predicate `{}`", raw.name),
    )?;

    Ok(CustomPredicate {
        name: raw.name,
        command,
        args: raw.args,
    })
}

/// Collect custom predicates from all plugins, detecting collisions.
fn build_custom_predicate_registry(
    plugins: &[ParsedPlugin],
    warnings: &mut Vec<LoadWarning>,
) -> CustomPredicateRegistry {
    let mut entries = std::collections::HashMap::new();
    let mut collisions: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (plugin_idx, parsed) in plugins.iter().enumerate() {
        for cp in &parsed.plugin.custom_predicates {
            if collisions.contains(&cp.name) {
                continue;
            }
            if let Some(existing) = entries.get(&cp.name) {
                let existing: &ResolvedCustomPredicate = existing;
                let existing_plugin_name = &plugins[existing.plugin_index].plugin.name;
                warnings.push(LoadWarning {
                    path: parsed.path.clone(),
                    message: format!(
                        "custom predicate `{}` defined by both `{}` and `{}` — skipping both",
                        cp.name, existing_plugin_name, parsed.plugin.name
                    ),
                });
                entries.remove(&cp.name);
                collisions.insert(cp.name.clone());
            } else {
                entries.insert(
                    cp.name.clone(),
                    ResolvedCustomPredicate {
                        plugin_index: plugin_idx,
                        command: cp.command.clone(),
                        args: cp.args.clone(),
                    },
                );
            }
        }
    }

    CustomPredicateRegistry { entries }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use std::collections::BTreeMap;

    use crate::predicate::PredicateSet;

    fn pred_set(s: &str) -> PredicateSet {
        PredicateSet::from_depends_on(s).unwrap()
    }

    fn ctx(deps: &[crate::pm::PackageId]) -> crate::predicate::PredicateContext<'_> {
        crate::predicate::PredicateContext::new(deps)
    }

    fn from_str(s: &str) -> Result<Plugin> {
        let manifest: RawPluginManifest = toml::from_str(s)?;
        validate_manifest(manifest, ManifestOrigin::Registry)
    }

    fn from_str_as(s: &str, origin: ManifestOrigin<'_>) -> Result<Plugin> {
        let manifest: RawPluginManifest = toml::from_str(s)?;
        validate_manifest(manifest, origin)
    }

    /// A recommendations `cargo/<name>/` entry needs neither `name` nor
    /// `depends-on`: its directory position supplies both.
    #[test]
    fn recommendations_entry_infers_name_and_gate() {
        let plugin = from_str_as(
            indoc! {r#"
                [[skills]]
                source.path = "skills"
            "#},
            ManifestOrigin::Recommendations {
                dep_name: "widget-lib",
            },
        )
        .unwrap();
        assert_eq!(plugin.name, "widget-lib");
        assert!(plugin.applies(&mut ctx(&[PackageId::new("cargo", "widget-lib", "1.0.0")])));
        assert!(!plugin.applies(&mut ctx(&[PackageId::new("cargo", "serde", "1.0.0")])));
    }

    /// The implied gate is ANDed with whatever the entry declares itself.
    #[test]
    fn recommendations_entry_keeps_its_own_name_and_predicates() {
        let plugin = from_str_as(
            indoc! {r#"
                name = "widget-tools"
                depends-on = ["serde"]
            "#},
            ManifestOrigin::Recommendations {
                dep_name: "widget-lib",
            },
        )
        .unwrap();
        assert_eq!(plugin.name, "widget-tools");
        assert!(!plugin.applies(&mut ctx(&[PackageId::new("cargo", "widget-lib", "1.0.0")])));
        assert!(plugin.applies(&mut ctx(&[
            PackageId::new("cargo", "widget-lib", "1.0.0"),
            PackageId::new("cargo", "serde", "1.0.0"),
        ])));
    }

    #[test]
    fn chained_plugins_parse_cargo_source() {
        let plugin = from_str(
            r#"
            name = "recs"
            depends-on = ["serde"]

            [[plugins]]
            source.cargo = "widget>=1"

            [[plugins]]
            depends-on = ["tokio"]
            source.cargo = { name = "gadget", version = "2" }
            "#,
        )
        .unwrap();
        assert_eq!(plugin.chained.len(), 2);

        // Atom form: name + version requirement, no per-edge gate.
        assert_eq!(plugin.chained[0].name, "widget");
        assert!(plugin.chained[0].version.is_some());
        assert!(plugin.chained[0].predicates.is_empty());

        // Table form with a per-edge `depends-on(tokio)` gate.
        assert_eq!(plugin.chained[1].name, "gadget");
        assert_eq!(plugin.chained[1].version.as_deref(), Some("2"));
        assert!(!plugin.chained[1].predicates.is_empty());
    }

    #[test]
    fn chained_plugin_git_and_path_are_not_yet_implemented() {
        let err = from_str(
            r#"
            name = "recs"
            depends-on = ["serde"]
            [[plugins]]
            source.git = "https://github.com/owner/repo"
            "#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("only `source.cargo`"), "{err}");
    }

    #[test]
    fn chained_plugin_rejects_dependency_table_form() {
        let err = from_str(
            r#"
            name = "recs"
            depends-on = ["serde"]
            [[plugins]]
            source.cargo = { widget = "1" }
            "#,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("dependency-table form"), "{msg}");
        // The hint must point at a *valid* replacement — the table form, which
        // preserves the crate name and version. It must not suggest the broken
        // atom `"widget1"` (parses as a crate literally named `widget1`).
        assert!(
            msg.contains(r#"{ name = "widget", version = "1" }"#),
            "{msg}"
        );
        assert!(!msg.contains(r#""widget1""#), "{msg}");
    }

    // --- Crate-embedded manifests (`load_crate_manifest`) ---

    #[test]
    fn crate_manifest_name_defaults_to_crate_and_depends_on_waived() {
        // No `name`, no `depends-on` — both waived for a crate manifest, since
        // the chained reference that reached it is the gate. A registry
        // manifest with the same body would be rejected on both counts.
        let plugin = load_crate_manifest(
            None,
            Some(
                r#"
            [[skills]]
            source.path = "agent-docs"
            "#,
            ),
            "crate-m",
        )
        .unwrap();
        assert_eq!(plugin.name, "crate-m");
        // The declared group plus the appended default `skills/` group.
        assert_eq!(plugin.skills.len(), 2);
        assert!(
            plugin.skills.iter().any(
                |g| matches!(&g.source, PluginSource::Path(p) if p.as_os_str() == "agent-docs")
            )
        );
        assert!(plugin.skills.iter().any(
            |g| matches!(&g.source, PluginSource::Path(p) if p.as_os_str() == CRATE_DEFAULT_SKILLS_PATH)
        ));
        // Crate groups are never workspace members.
        assert!(plugin.skills.iter().all(|g| !g.workspace_member));
    }

    #[test]
    fn crate_manifest_default_skills_group_can_be_opted_out() {
        let plugin = load_crate_manifest(
            None,
            Some(indoc! {r#"
                [defaults]
                skills = false

                [[skills]]
                source.path = "agent-docs"
            "#}),
            "crate-m",
        )
        .unwrap();
        assert_eq!(plugin.skills.len(), 1);
        assert!(
            matches!(&plugin.skills[0].source, PluginSource::Path(p) if p.as_os_str() == "agent-docs")
        );
    }

    #[test]
    fn crate_manifest_bare_gets_default_skills_group() {
        // An empty manifest still yields the default `skills/` group, so
        // shipping a `SYMPOSIUM.toml` never silently drops the skills the
        // metadata path would have found.
        let plugin = load_crate_manifest(None, Some(""), "crate-m").unwrap();
        assert_eq!(plugin.name, "crate-m");
        assert_eq!(plugin.skills.len(), 1);
        assert!(matches!(
            &plugin.skills[0].source,
            PluginSource::Path(p) if p.as_os_str() == CRATE_DEFAULT_SKILLS_PATH
        ));
    }

    #[test]
    fn crate_manifest_carries_hooks_for_later_dispatch() {
        // A crate manifest may declare hooks; they validate and are carried,
        // even though the chained path does not dispatch them yet.
        let plugin = load_crate_manifest(
            None,
            Some(indoc! {r#"
                [[installations]]
                name = "tool"
                source = "cargo"
                crate = "crate-m-hooks"
                executable = "crate-m-hooks"

                [[hooks]]
                name = "check"
                event = "PreToolUse"
                command = "tool"
            "#}),
            "crate-m",
        )
        .unwrap();
        assert_eq!(plugin.hooks.len(), 1);
        assert_eq!(plugin.hooks[0].name, "check");
    }

    #[test]
    fn crate_manifest_merges_metadata_and_file() {
        // `[package.metadata.symposium]` and `SYMPOSIUM.toml` layer additively:
        // one skill group from each, plus the appended default `skills/`.
        let meta: toml::Table = toml::from_str(indoc! {r#"
            [[skills]]
            source.path = "from-metadata"
        "#})
        .unwrap();
        let plugin = load_crate_manifest(
            Some(meta),
            Some(indoc! {r#"
                [[skills]]
                source.path = "from-file"
            "#}),
            "crate-m",
        )
        .unwrap();
        let paths: Vec<_> = plugin
            .skills
            .iter()
            .filter_map(|g| match &g.source {
                PluginSource::Path(p) => Some(p.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect();
        assert!(paths.iter().any(|p| p == "from-metadata"), "{paths:?}");
        assert!(paths.iter().any(|p| p == "from-file"), "{paths:?}");
        assert!(
            paths.iter().any(|p| p == CRATE_DEFAULT_SKILLS_PATH),
            "{paths:?}"
        );
    }

    #[test]
    fn crate_manifest_malformed_metadata_is_lenient() {
        // A metadata layer that isn't a valid manifest is dropped; the file
        // layer and the default group still resolve.
        let bad: toml::Table = toml::from_str(r#"skills = "not-an-array""#).unwrap();
        let plugin = load_crate_manifest(
            Some(bad),
            Some(indoc! {r#"
                [[skills]]
                source.path = "from-file"
            "#}),
            "crate-m",
        )
        .unwrap();
        assert!(
            plugin.skills.iter().any(
                |g| matches!(&g.source, PluginSource::Path(p) if p.as_os_str() == "from-file")
            )
        );
        assert!(plugin.skills.iter().any(
            |g| matches!(&g.source, PluginSource::Path(p) if p.as_os_str() == CRATE_DEFAULT_SKILLS_PATH)
        ));
    }

    #[test]
    fn crate_manifest_metadata_redirect_is_a_chained_reference() {
        // A reschema'd `crate = {..}` redirect: metadata declares a chained
        // reference and no skills of its own; it still gets the default
        // `skills/` group (combined with defaults).
        let meta: toml::Table = toml::from_str(indoc! {r#"
            [[plugins]]
            source.cargo = "other-crate"
        "#})
        .unwrap();
        let plugin = load_crate_manifest(Some(meta), None, "crate-m").unwrap();
        assert_eq!(plugin.chained.len(), 1);
        assert_eq!(plugin.chained[0].name, "other-crate");
        assert_eq!(plugin.skills.len(), 1);
        assert!(matches!(
            &plugin.skills[0].source,
            PluginSource::Path(p) if p.as_os_str() == CRATE_DEFAULT_SKILLS_PATH
        ));
    }

    #[test]
    fn registry_manifest_still_rejects_defaults() {
        let err = from_str(indoc! {r#"
            name = "recs"
            depends-on = ["serde"]

            [defaults]
            skills = false
        "#})
        .unwrap_err();
        assert!(err.to_string().contains("[defaults]"), "{err}");
    }

    const SAMPLE: &str = indoc! {r#"
        name = "example-plugin"
        depends-on = ["*"]

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
            depends-on = ["serde"]

            [[skills]]
            depends-on = ["serde"]
            source.git = "https://github.com/org/repo/tree/main/serde"
        "#};
        let plugin = from_str(toml).expect("parse");
        assert_eq!(plugin.name, "remote-plugin");
        assert_eq!(plugin.skills.len(), 1);
        let group = &plugin.skills[0];
        assert!(group.predicates.references_dep("serde"));
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
    fn parse_predicates_top_level() {
        let toml = indoc! {r#"
            name = "env-pred-plugin"
            depends-on = ["*"]
            predicates = ["shell(command -v rg)", "path_exists(Cargo.toml)"]

            [[skills]]
            depends-on = ["serde"]
            source.path = "skills"
        "#};
        let plugin = from_str(toml).expect("parse");
        // `depends-on = ["*"]` lowers to a leading `depends-on(*)`, then the two
        // function-call predicates.
        use crate::predicate::Predicate;
        assert_eq!(
            plugin.predicates.predicates,
            vec![
                Predicate::DependsOnWildcard,
                Predicate::Shell("command -v rg".into()),
                Predicate::PathExists("Cargo.toml".into()),
            ]
        );
    }

    #[test]
    fn parse_predicates_on_skill_group() {
        let toml = indoc! {r#"
            name = "p"
            depends-on = ["*"]

            [[skills]]
            depends-on = ["serde"]
            predicates = ["shell(command -v jq)"]
            source.path = "skills"
        "#};
        let plugin = from_str(toml).expect("parse");
        // group `depends-on = ["serde"]` lowers to `depends-on(serde)`, plus the shell predicate.
        use crate::predicate::Predicate;
        assert_eq!(
            plugin.skills[0].predicates.predicates,
            vec![
                Predicate::DependsOn("serde".into(), None),
                Predicate::Shell("command -v jq".into()),
            ]
        );
    }

    #[test]
    fn parse_predicates_on_hook() {
        let toml = indoc! {r#"
            name = "p"
            depends-on = ["*"]

            [[hooks]]
            name = "h"
            event = "PreToolUse"
            command = { script = "scripts/x.sh" }
            predicates = ["path_exists(.git)"]
        "#};
        let plugin = from_str(toml).expect("parse");
        assert_eq!(plugin.hooks[0].predicates.predicates.len(), 1);
    }

    #[test]
    fn predicates_default_empty() {
        // With no `predicates`, the plugin gate is just the lowered `depends-on`
        // (here `depends-on(*)`), and hooks default to no predicates.
        let plugin = from_str(SAMPLE).expect("parse");
        assert_eq!(
            plugin.predicates.predicates,
            vec![crate::predicate::Predicate::DependsOnWildcard]
        );
        assert!(plugin.hooks[0].predicates.is_empty());
    }

    #[test]
    fn parse_manifest_depends_on_as_array() {
        let toml = indoc! {r#"
            name = "array-depends-on"
            depends-on = ["*"]

            [[skills]]
            depends-on = ["serde"]
            source.path = "skills"
        "#};
        let plugin = from_str(toml).expect("parse");
        let group = &plugin.skills[0];
        assert!(group.predicates.predicates[0].references_dep("serde"));
    }

    #[test]
    fn parse_manifest_rejects_renamed_crates_field() {
        // Plugin level, group level, and MCP-server level all reject the old
        // `crates` spelling with a migration hint.
        for toml in [
            indoc! {r#"
                name = "old-spelling"
                crates = ["serde"]
            "#},
            indoc! {r#"
                name = "old-spelling"
                depends-on = ["*"]

                [[skills]]
                crates = ["serde"]
            "#},
            indoc! {r#"
                name = "old-spelling"
                depends-on = ["*"]

                [[mcp_servers]]
                name = "server"
                command = "/usr/bin/true"
                args = ["--stdio"]
                env = []
                crates = ["serde"]
            "#},
        ] {
            let err = from_str(toml).unwrap_err();
            assert!(
                err.to_string().contains("use `depends-on` instead"),
                "expected migration hint, got: {err}"
            );
        }
    }

    #[test]
    fn parse_manifest_rejects_renamed_crate_predicate() {
        let toml = indoc! {r#"
            name = "old-predicate"
            predicates = ["crate(serde)"]
        "#};
        let err = from_str(toml).unwrap_err();
        assert!(
            err.to_string().contains("use `depends-on(serde)` instead"),
            "expected migration hint, got: {err}"
        );
    }

    #[test]
    fn scan_source_dir_finds_plugins_and_standalone_skills() {
        use crate::test_utils::{File, instantiate_fixture};
        let tmp = instantiate_fixture(&[
            File(
                "my-plugin/SYMPOSIUM.toml",
                indoc! {r#"
                name = "my-plugin"
                depends-on = ["*"]

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
                depends-on: serde
                ---

                Use this skill.
            "},
            ),
        ]);
        // Also create a random directory (should be ignored)
        std::fs::create_dir_all(tmp.path().join("not-a-plugin-or-skill")).unwrap();

        let contents = scan_source_dir(tmp.path(), "").unwrap();
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
        let contents = scan_source_dir(tmp.path(), "").unwrap();
        assert!(contents.plugins.is_empty());
        assert!(contents.skill_files.is_empty());
    }

    #[test]
    fn scan_source_dir_missing() {
        let contents = scan_source_dir("/nonexistent/path/abc123", "").unwrap();
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
                depends-on: serde
                ---

                Root level skill.
            "},
        )]);

        let err = scan_source_dir(tmp.path(), "").unwrap_err();
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
                depends-on = ["*"]
            "#},
        )]);

        let err = scan_source_dir(tmp.path(), "").unwrap_err();
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
                depends-on = ["*"]
            "#},
            ),
            File(
                "mixed/SKILL.md",
                indoc! {"
                ---
                name: ignored-skill
                depends-on: serde
                ---

                This should be ignored.
            "},
            ),
        ]);

        let contents = scan_source_dir(tmp.path(), "").unwrap();
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
                depends-on = ["*"]
            "#},
            ),
            File(
                "precedence-test/other.toml",
                indoc! {r#"
                name = "ignored-plugin"
            "#},
            ),
        ]);

        let contents = scan_source_dir(tmp.path(), "").unwrap();
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
                depends-on = ["*"]
            "#},
            ),
            File(
                "foo/bar/SKILL.md",
                indoc! {"
                ---
                name: foo-bar-skill
                depends-on: serde
                ---

                Should be pruned.
            "},
            ),
            File(
                "baz/SKILL.md",
                indoc! {"
                ---
                name: baz-skill
                depends-on: tokio
                ---

                Should be found.
            "},
            ),
            File(
                "baz/qux/SYMPOSIUM.toml",
                indoc! {r#"
                name = "qux-plugin"
                depends-on = ["*"]
            "#},
            ),
            File(
                "baz/qux/SKILL.md",
                indoc! {"
                ---
                name: qux-skill
                depends-on: anyhow
                ---

                Should be pruned.
            "},
            ),
        ]);

        let contents = scan_source_dir(tmp.path(), "").unwrap();
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
                depends-on = ["serde"]
            "#},
            ),
            File("bad-plugin/SYMPOSIUM.toml", "not valid toml {{{"),
            File(
                "my-skill/SKILL.md",
                indoc! {"
                ---
                name: my-skill
                description: A skill
                depends-on: serde
                ---

                Body.
            "},
            ),
            File(
                "bad-skill/SKILL.md",
                indoc! {"
                ---
                description: No name
                depends-on: serde
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
                depends-on: serde
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
                depends-on = ["*"]

                [[skills]]
                depends-on = ["serde", "serde_json>=1.0"]
                source.path = "skills"
            "#},
            ),
            File(
                "my-skill/SKILL.md",
                indoc! {"
                ---
                name: my-skill
                description: A skill
                depends-on: anyhow
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
                depends-on: serde
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
            depends-on = ["*"]

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
            depends-on = ["*"]

            [[skills]]
            depends-on = ["serde"]
            source.path = "serde-skills"

            [[skills]]
            depends-on = ["tokio"]
            source.path = "tokio-skills"
        "#};
        let plugin = from_str(toml).expect("parse");
        assert_eq!(plugin.name, "multi-group");
        assert_eq!(plugin.skills.len(), 2);
        assert!(plugin.skills[0].predicates.predicates[0].references_dep("serde"));
        assert!(plugin.skills[1].predicates.predicates[0].references_dep("tokio"));
    }

    #[test]
    fn plugin_crate_filtering() {
        let workspace_crates = vec![
            crate::pm::PackageId::new(crate::pm::CARGO_PM, "serde", "1.0.0"),
            crate::pm::PackageId::new(crate::pm::CARGO_PM, "tokio", "1.0.0"),
        ];

        // Plugin with wildcard - should apply to all
        let plugin_wildcard = Plugin {
            name: "wildcard".to_string(),
            predicates: pred_set("*"),
            hooks: vec![],
            skills: vec![],
            mcp_servers: vec![],
            installations: Vec::new(),
            subcommands: BTreeMap::new(),
            custom_predicates: vec![],
            chained: vec![],
        };
        assert!(plugin_wildcard.applies(&mut ctx(&workspace_crates)));

        // Plugin targeting serde - should apply
        let plugin_serde = Plugin {
            name: "serde-plugin".to_string(),
            predicates: pred_set("serde"),
            hooks: vec![],
            skills: vec![],
            mcp_servers: vec![],
            installations: Vec::new(),
            subcommands: BTreeMap::new(),
            custom_predicates: vec![],
            chained: vec![],
        };
        assert!(plugin_serde.applies(&mut ctx(&workspace_crates)));

        // Plugin targeting non-existent crate - should not apply
        let plugin_other = Plugin {
            name: "other-plugin".to_string(),
            predicates: pred_set("other-crate"),
            hooks: vec![],
            skills: vec![],
            mcp_servers: vec![],
            installations: Vec::new(),
            subcommands: BTreeMap::new(),
            custom_predicates: vec![],
            chained: vec![],
        };
        assert!(!plugin_other.applies(&mut ctx(&workspace_crates)));

        // Plugin with version predicate - should reject wrong version
        let plugin_version = Plugin {
            name: "version-plugin".to_string(),
            predicates: pred_set("tokio>=2.0"),
            hooks: vec![],
            skills: vec![],
            mcp_servers: vec![],
            installations: Vec::new(),
            subcommands: BTreeMap::new(),
            custom_predicates: vec![],
            chained: vec![],
        };
        assert!(!plugin_version.applies(&mut ctx(&workspace_crates)));
    }

    #[test]
    fn workspace_plugins_interpret_member_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        // Root: manifest without a name — name falls back to the dir name,
        // default skills group is appended.
        std::fs::write(root.join("SYMPOSIUM.toml"), "").unwrap();

        // member-bare: no manifest, but a skills/ dir — bare convention.
        let bare = root.join("member-bare");
        std::fs::create_dir_all(bare.join("skills")).unwrap();

        // member-optout: manifest opting out of default content.
        let optout = root.join("member-optout");
        std::fs::create_dir_all(&optout).unwrap();
        std::fs::write(
            optout.join("SYMPOSIUM.toml"),
            indoc! {r#"
                name = "explicit-name"

                [defaults]
                skills = false
            "#},
        )
        .unwrap();

        // member-empty: neither manifest nor skills/ — defines nothing.
        let empty = root.join("member-empty");
        std::fs::create_dir_all(&empty).unwrap();

        let members = vec![bare.clone(), optout.clone(), empty.clone()];
        let (plugins, warnings) = workspace_plugins(root, &members, true);
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");

        let names: Vec<&str> = plugins.iter().map(|p| p.plugin.name.as_str()).collect();
        let root_name = root.file_name().unwrap().to_str().unwrap();
        assert_eq!(names, vec![root_name, "member-bare", "explicit-name"]);

        for parsed in &plugins {
            assert!(parsed.workspace_member);
            assert_eq!(parsed.source_dir, root);
            // Groups carry the provenance too: workspace skills load with
            // lenient frontmatter rules.
            assert!(parsed.plugin.skills.iter().all(|g| g.workspace_member));
        }

        // Root and bare member each get the two default groups: `skills/`
        // and the `workspace-member()`-gated `.agents/skills`.
        assert_eq!(plugins[0].plugin.skills.len(), 2);
        assert_eq!(
            plugins[1].plugin.skills[0].source,
            PluginSource::Path(PathBuf::from("skills"))
        );
        assert_eq!(
            plugins[1].plugin.skills[1].source,
            PluginSource::Path(PathBuf::from(".agents/skills"))
        );
        assert!(!plugins[1].plugin.skills[1].predicates.predicates.is_empty());
        // The opt-out member has no groups.
        assert!(plugins[2].plugin.skills.is_empty());
    }

    #[test]
    fn agents_syncing_disabled_omits_agents_skills_group() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("skills")).unwrap();
        // A member defined only by `.agents/skills/`.
        let member = root.join("member");
        std::fs::create_dir_all(member.join(".agents/skills")).unwrap();
        let members = vec![member.clone()];

        let (plugins, _) = workspace_plugins(root, &members, true);
        let names: Vec<&str> = plugins.iter().map(|p| p.plugin.name.as_str()).collect();
        assert!(names.contains(&"member"), "{names:?}");

        let (plugins, _) = workspace_plugins(root, &members, false);
        let names: Vec<&str> = plugins.iter().map(|p| p.plugin.name.as_str()).collect();
        assert!(!names.contains(&"member"), "{names:?}");
        assert_eq!(plugins[0].plugin.skills.len(), 1);
    }

    #[test]
    fn workspace_manifest_may_omit_dependency_gate() {
        // A registry manifest without any depends-on is rejected; the same
        // manifest is fine as a workspace plugin (membership is the gate).
        let manifest: RawPluginManifest = toml::from_str(indoc! {r#"
                name = "gateless"

                [[skills]]
                source.path = "extra-skills"
            "#})
        .unwrap();
        let err = validate_manifest(manifest, ManifestOrigin::Registry).unwrap_err();
        assert!(err.to_string().contains("references no dependency"));

        let manifest: RawPluginManifest = toml::from_str(indoc! {r#"
                name = "gateless"

                [[skills]]
                source.path = "extra-skills"
            "#})
        .unwrap();
        let plugin = validate_manifest(
            manifest,
            ManifestOrigin::WorkspaceMember {
                dir_name: "d",
                agents_skills: true,
            },
        )
        .unwrap();
        // Explicit group plus the two appended default groups.
        assert_eq!(plugin.skills.len(), 3);
    }

    #[test]
    fn registry_manifest_rejects_defaults_section() {
        let manifest: RawPluginManifest = toml::from_str(indoc! {r#"
                name = "p"
                depends-on = ["*"]

                [defaults]
                skills = false
            "#})
        .unwrap();
        let err = validate_manifest(manifest, ManifestOrigin::Registry).unwrap_err();
        assert!(err.to_string().contains("[defaults]"));
    }

    #[test]
    fn parsed_plugin_applies_stamps_workspace_member() {
        let plugin = Plugin {
            name: "ws-plugin".to_string(),
            predicates: PredicateSet {
                predicates: vec![crate::predicate::Predicate::WorkspaceMember],
            },
            hooks: vec![],
            skills: vec![],
            mcp_servers: vec![],
            installations: Vec::new(),
            subcommands: BTreeMap::new(),
            custom_predicates: vec![],
            chained: vec![],
        };
        let mut parsed = ParsedPlugin {
            path: PathBuf::from("/test/SYMPOSIUM.toml"),
            plugin,
            source_dir: PathBuf::from("/test"),
            workspace_member: false,
            canonical: PackageId::new("test", "test", ANY_VERSION),
        };
        let deps: Vec<crate::pm::PackageId> = Vec::new();
        let mut c = ctx(&deps);
        assert!(!parsed.applies(&mut c));
        parsed.workspace_member = true;
        assert!(parsed.applies(&mut c));
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
                depends-on = ["serde"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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
            depends-on = ["*"]

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

    // --- source = "crate" parsing ---

    #[test]
    fn parse_source_crate_shorthand_is_error() {
        // `source = "crate"` is retired — a crate now provides a plugin via a
        // `[[plugins]] source.cargo` reference.
        let toml = indoc! {r#"
            name = "crate-shorthand"
            depends-on = ["serde"]

            [[skills]]
            source = "crate"
        "#};
        let err = from_str(toml).unwrap_err();
        assert!(
            err.to_string().contains("[[plugins]] source.cargo"),
            "expected migration hint, got: {err}"
        );
    }

    #[test]
    fn parse_source_crate_path_is_error() {
        let toml = indoc! {r#"
            name = "bad"
            depends-on = ["serde"]

            [[skills]]
            source.crate_path = "skills"
        "#};
        let err = from_str(toml).unwrap_err();
        assert!(
            err.to_string().contains("[[plugins]] source.cargo"),
            "expected migration hint, got: {err}"
        );
    }

    #[test]
    fn parse_source_crate_table_is_error() {
        let toml = indoc! {r#"
            name = "bad"
            depends-on = ["serde"]

            [[skills]]
            source.crate = { name = "foo" }
        "#};
        let err = from_str(toml).unwrap_err();
        assert!(
            err.to_string().contains("[[plugins]] source.cargo"),
            "expected migration hint, got: {err}"
        );
    }

    #[test]
    fn reject_path_and_git() {
        let toml = indoc! {r#"
            name = "bad"
            depends-on = ["serde"]

            [[skills]]
            source.path = "."
            source.git = "https://github.com/org/repo/tree/main/x"
        "#};
        let err = from_str(toml).unwrap_err();
        assert!(err.to_string().contains("mutually exclusive"), "{err}");
    }

    // --- dependency-requirement validation ---

    #[test]
    fn crate_reference_on_hook_satisfies_requirement() {
        // A plugin whose only dependency reference is a `depends-on(...)`
        // predicate on a hook is valid — the hook is dependency-gated even
        // with no plugin-level `depends-on`.
        let toml = indoc! {r#"
            name = "hook-crate"

            [[hooks]]
            name = "h"
            event = "PreToolUse"
            command = { script = "scripts/x.sh" }
            predicates = ["depends-on(serde)"]
        "#};
        let plugin = from_str(toml).expect("should be valid");
        assert!(plugin.hooks[0].predicates.references_dep("serde"));
    }

    // --- TOML serialization round-trip tests ---

    fn roundtrip(plugin: &Plugin) -> Plugin {
        let toml_str = toml::to_string_pretty(plugin).expect("serialize");
        from_str(&toml_str).unwrap_or_else(|e| panic!("round-trip parse failed:\n{toml_str}\n{e}"))
    }

    #[test]
    fn roundtrip_source_path() {
        let plugin = from_str(indoc! {r#"
            name = "rt"
            depends-on = ["serde"]

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
            depends-on = ["serde"]

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
    fn skill_group_without_source_errors() {
        let err = from_str(indoc! {r#"
            name = "rt"
            depends-on = ["serde"]

            [[skills]]
            depends-on = ["serde"]
        "#})
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("must set `source.path` or `source.git`"),
            "expected missing-source error, got: {err}"
        );
    }

    #[test]
    fn skill_group_empty_source_errors() {
        let err = from_str(indoc! {r#"
            name = "rt"
            depends-on = ["serde"]

            [[skills]]
            depends-on = ["serde"]
            source = {}
        "#})
        .unwrap_err();
        assert!(
            err.to_string().contains("must set `path` or `git`"),
            "expected empty-source error, got: {err}"
        );
    }

    #[test]
    fn parse_subcommand_minimal_named() {
        let toml = indoc! {r#"
            name = "p"
            depends-on = ["*"]

            [[installations]]
            name = "tool"
            source = "cargo"
            crate = "example-tool"

            [subcommand.foo]
            description = "Run foo"
            command = "tool"
        "#};
        let plugin = from_str(toml).expect("parse");
        assert_eq!(plugin.subcommands.len(), 1);
        let sub = &plugin.subcommands["foo"];
        assert_eq!(sub.description, "Run foo");
        assert_eq!(sub.audience, Audience::Agents); // default
        assert_eq!(sub.command, "tool");
    }

    #[test]
    fn parse_subcommand_audience_humans() {
        let toml = indoc! {r#"
            name = "p"
            depends-on = ["*"]

            [[installations]]
            name = "tool"
            source = "cargo"
            crate = "example-tool"

            [subcommand.foo]
            description = "Run foo"
            command = "tool"
            audience = "humans"
        "#};
        let plugin = from_str(toml).expect("parse");
        assert_eq!(plugin.subcommands["foo"].audience, Audience::Humans);
    }

    #[test]
    fn parse_subcommand_inline_command_is_promoted() {
        let toml = indoc! {r#"
            name = "p"
            depends-on = ["*"]

            [subcommand.foo]
            description = "Run foo"
            command = { source = "cargo", crate = "example-tool", executable = "example-tool" }
        "#};
        let plugin = from_str(toml).expect("parse");
        // The inline command is promoted to a synthetic installation named
        // after the subcommand.
        assert_eq!(plugin.subcommands["foo"].command, "foo");
        let install = plugin
            .installations
            .iter()
            .find(|i| i.name == "foo")
            .expect("synthetic installation present");
        assert_eq!(install.executable.as_deref(), Some("example-tool"));
    }

    #[test]
    fn parse_subcommand_rejects_unknown_field() {
        let toml = indoc! {r#"
            name = "p"
            depends-on = ["*"]

            [subcommand.foo]
            description = "Run foo"
            command = "tool"
            bogus = 42
        "#};
        let err = from_str(toml).err().expect("expected error");
        let msg = format!("{err:#}");
        assert!(msg.contains("bogus") || msg.contains("unknown"), "{msg}");
    }

    #[test]
    fn parse_subcommand_rejects_reserved_name() {
        let toml = indoc! {r#"
            name = "p"
            depends-on = ["*"]

            [[installations]]
            name = "tool"
            source = "cargo"
            crate = "example-tool"

            [subcommand.init]
            description = "Try to shadow init"
            command = "tool"
        "#};
        let err = from_str(toml).err().expect("expected error");
        let msg = format!("{err:#}");
        assert!(msg.contains("shadows") && msg.contains("init"), "{msg}");
    }

    #[test]
    fn parse_subcommand_rejects_invalid_name_chars() {
        let toml = indoc! {r#"
            name = "p"
            depends-on = ["*"]

            [[installations]]
            name = "tool"
            source = "cargo"
            crate = "example-tool"

            [subcommand."foo.bar"]
            description = "dotted name"
            command = "tool"
        "#};
        let err = from_str(toml).err().expect("expected error");
        let msg = format!("{err:#}");
        assert!(msg.contains("invalid characters"), "{msg}");
    }

    #[test]
    fn parse_subcommand_rejects_oversized_description() {
        let huge = "x".repeat(1100);
        let toml = format!(
            r#"
            name = "p"
            depends-on = ["*"]

            [[installations]]
            name = "tool"
            source = "cargo"
            crate = "example-tool"

            [subcommand.foo]
            description = "{huge}"
            command = "tool"
            "#
        );
        let err = from_str(&toml).err().expect("expected error");
        let msg = format!("{err:#}");
        assert!(msg.contains("1024"), "{msg}");
    }

    #[test]
    fn parse_subcommand_unknown_command_reference_fails() {
        let toml = indoc! {r#"
            name = "p"
            depends-on = ["*"]

            [subcommand.foo]
            description = "..."
            command = "missing"
        "#};
        let err = from_str(toml).err().expect("expected error");
        let msg = format!("{err:#}");
        assert!(msg.contains("unknown installation"), "{msg}");
    }

    #[test]
    fn scan_source_dir_loads_plugin_with_subcommand() {
        use crate::test_utils::{File, instantiate_fixture};
        let tmp = instantiate_fixture(&[File(
            "demo-plugin/SYMPOSIUM.toml",
            indoc! {r#"
                name = "demo-plugin"
                depends-on = ["example-crate"]

                [[installations]]
                name = "example-tool"
                source = "cargo"
                crate = "example-tool"
                executable = "example-tool"
                args = ["serve"]

                [subcommand.demo]
                description = "Run the demo tool"
                audience = "agents"
                command = "example-tool"
            "#},
        )]);

        let contents = scan_source_dir(tmp.path(), "").unwrap();
        assert_eq!(contents.plugins.len(), 1);
        let parsed = contents.plugins[0].as_ref().unwrap();
        let sub = &parsed.plugin.subcommands["demo"];
        assert_eq!(sub.description, "Run the demo tool");
        assert_eq!(sub.audience, Audience::Agents);
        assert_eq!(sub.command, "example-tool");

        let install = parsed
            .plugin
            .installations
            .iter()
            .find(|i| i.name == "example-tool")
            .expect("named installation present");
        assert_eq!(install.executable.as_deref(), Some("example-tool"));
        assert_eq!(install.args, vec!["serve".to_string()]);
    }

    #[test]
    fn parse_subcommand_with_crates_predicate() {
        let toml = indoc! {r#"
            name = "p"
            depends-on = ["*"]

            [[installations]]
            name = "tool"
            source = "cargo"
            crate = "example-tool"

            [subcommand.foo]
            description = "Only for serde projects"
            command = "tool"
            depends-on = ["serde"]
        "#};
        let plugin = from_str(toml).expect("parse");
        let sub = &plugin.subcommands["foo"];
        assert!(sub.predicates.references_dep("serde"));
    }

    // --- custom predicate collision tests ---

    fn make_plugin_with_predicate(plugin_name: &str, predicate_name: &str) -> ParsedPlugin {
        ParsedPlugin {
            path: std::path::PathBuf::from(format!("{plugin_name}.toml")),
            plugin: Plugin {
                name: plugin_name.to_string(),
                predicates: pred_set("*"),
                installations: vec![Installation {
                    name: "checker".to_string(),
                    source: None,
                    executable: Some("/bin/true".to_string()),
                    script: None,
                    args: vec![],
                    requirements: vec![],
                    install_commands: vec![],
                }],
                hooks: vec![],
                skills: vec![],
                mcp_servers: vec![],
                subcommands: BTreeMap::new(),
                custom_predicates: vec![CustomPredicate {
                    name: predicate_name.to_string(),
                    command: "checker".to_string(),
                    args: vec![],
                }],
                chained: vec![],
            },
            source_dir: std::path::PathBuf::from("/test"),
            workspace_member: false,
            canonical: PackageId::new("test", plugin_name, ANY_VERSION),
        }
    }

    #[test]
    fn custom_predicate_registry_no_collision() {
        let plugins = vec![
            make_plugin_with_predicate("alpha", "foo"),
            make_plugin_with_predicate("beta", "bar"),
        ];
        let mut warnings = vec![];
        let registry = build_custom_predicate_registry(&plugins, &mut warnings);
        assert!(warnings.is_empty());
        assert_eq!(registry.len(), 2);
        assert!(registry.contains_key("foo"));
        assert!(registry.contains_key("bar"));
    }

    #[test]
    fn custom_predicate_registry_two_way_collision() {
        let plugins = vec![
            make_plugin_with_predicate("alpha", "shared"),
            make_plugin_with_predicate("beta", "shared"),
        ];
        let mut warnings = vec![];
        let registry = build_custom_predicate_registry(&plugins, &mut warnings);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("shared"));
        assert!(warnings[0].message.contains("alpha"));
        assert!(warnings[0].message.contains("beta"));
        assert!(
            !registry.contains_key("shared"),
            "collided predicate must be removed"
        );
    }

    #[test]
    fn custom_predicate_registry_three_way_collision() {
        let plugins = vec![
            make_plugin_with_predicate("alpha", "shared"),
            make_plugin_with_predicate("beta", "shared"),
            make_plugin_with_predicate("gamma", "shared"),
        ];
        let mut warnings = vec![];
        let registry = build_custom_predicate_registry(&plugins, &mut warnings);
        // Warning is emitted only on the second occurrence (alpha vs beta);
        // the third (gamma) sees the name in the collision set and skips.
        assert_eq!(warnings.len(), 1);
        assert!(
            !registry.contains_key("shared"),
            "collided predicate must be removed"
        );
    }
}
