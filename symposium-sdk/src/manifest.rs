//! The `SYMPOSIUM.toml` manifest schema, unvalidated.
//!
//! This is the shape a plugin manifest deserializes into, and the shape that
//! crosses the package-manager boundary. A PM's job is to *produce* one of
//! these: by parsing a `SYMPOSIUM.toml` it found, by translating another
//! ecosystem's manifest, or by synthesizing one for a package that describes
//! itself not at all. Symposium then validates it, applies defaults, and
//! decides trust.
//!
//! Everything here is deliberately permissive: fields that Symposium rejects
//! (the retired `crates` key, `source.crate`, git/path chained sources) still
//! *parse*, so the rejection can carry a useful message instead of an unknown
//! field error. Validation is not this crate's job.
//!
//! The types keep the `Raw` prefix to distinguish them from Symposium's
//! validated counterparts, several of which share a name.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use symposium_install::Source;

use crate::hook::{HookAgent, HookEvent};
use crate::predicate::{DependsOnList, PredicateSet};

/// Default subdirectory scanned for skills when a manifest says nothing.
pub const DEFAULT_SKILLS_PATH: &str = "skills";

/// Default location for skills that apply while *maintaining* a workspace
/// (as opposed to using its published packages).
pub const AGENTS_SKILLS_PATH: &str = ".agents/skills";

/// A whole `SYMPOSIUM.toml`, unvalidated.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawPluginManifest {
    /// Required for registry plugins; defaults to a positional fallback (the
    /// directory or package name) elsewhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Default-content opt-outs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defaults: Option<RawDefaults>,
    #[serde(default, rename = "depends-on", skip_serializing_if = "is_empty_deps")]
    pub depends_on: DependsOnList,
    /// Rejected by validation: renamed to `depends-on`. Parsed so the error
    /// can say so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crates: Option<toml::Value>,
    #[serde(default, skip_serializing_if = "PredicateSet::is_empty")]
    pub predicates: PredicateSet,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub installations: Vec<RawNamedInstallation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hooks: Vec<RawHook>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<RawSkillGroup>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<RawPluginMcpServer>,
    /// TOML key is singular (`[subcommand.<name>]`); the validated field is
    /// plural (`subcommands`).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub subcommand: std::collections::BTreeMap<String, RawSubcommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub predicate: Vec<RawCustomPredicate>,
    /// Chained plugin references — `[[plugins]]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plugins: Vec<RawChainedPlugin>,
}

fn is_empty_deps(list: &DependsOnList) -> bool {
    list.0.is_empty()
}

impl RawPluginManifest {
    /// Parse a manifest from TOML source.
    pub fn parse(toml_str: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(toml_str)
    }

    /// Layer `over` on top of `self`. List-shaped content (skills, chained
    /// plugins, hooks, installations, MCP servers, custom predicates) appends
    /// in `self`-then-`over` order; the `subcommand` map and scalar fields take
    /// `over` where it sets them; `depends-on` / `predicates` gates AND
    /// together.
    ///
    /// Used to combine a package's ecosystem-native metadata (base) with a
    /// `SYMPOSIUM.toml` (over).
    pub fn merge(mut self, over: RawPluginManifest) -> RawPluginManifest {
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

    /// Append a `[[skills]]` group reading `path`, gated by `predicates`.
    /// The shape every default skill group takes.
    pub fn push_skill_group(&mut self, path: &str, predicates: PredicateSet) {
        self.skills.push(RawSkillGroup {
            depends_on: None,
            crates: None,
            predicates,
            source: Some(RawPluginSource::Table(RawPluginSourceTable {
                path: Some(PathBuf::from(path)),
                git: None,
                crate_field: None,
                crate_path: None,
            })),
        });
    }

    /// Whether `[defaults] skills` is on (the default when unset).
    pub fn wants_default_skills(&self) -> bool {
        self.defaults.as_ref().is_none_or(|d| d.skills)
    }
}

/// `[defaults]` section: opt-outs for automatically added content.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawDefaults {
    /// Add the default `[[skills]] source.path = "skills"` group.
    #[serde(default = "default_skills_flag")]
    pub skills: bool,
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

/// A `[[mcp_servers]]` entry.
#[derive(Debug, Deserialize, Serialize)]
pub struct RawPluginMcpServer {
    #[serde(
        default,
        rename = "depends-on",
        skip_serializing_if = "Option::is_none"
    )]
    pub depends_on: Option<DependsOnList>,
    /// Rejected by validation: renamed to `depends-on`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crates: Option<toml::Value>,
    #[serde(default, skip_serializing_if = "PredicateSet::is_empty")]
    pub predicates: PredicateSet,
    #[serde(flatten)]
    pub server: sacp::schema::McpServer,
}

/// Source declaration for a skill group: `source.path` or `source.git`.
///
/// The shorthand string form and the `crate` keys are retired; they parse so
/// validation can reject them with a migration hint.
#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum RawPluginSource {
    Shorthand(String),
    Table(RawPluginSourceTable),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawPluginSourceTable {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<String>,
    /// Rejected: `source.crate = { ... }` is no longer valid.
    #[serde(default, rename = "crate", skip_serializing_if = "Option::is_none")]
    pub crate_field: Option<toml::Value>,
    /// Rejected: `source.crate_path = "..."` is no longer valid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crate_path: Option<toml::Value>,
}

/// A `[[skills]]` entry.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawSkillGroup {
    #[serde(
        default,
        rename = "depends-on",
        skip_serializing_if = "Option::is_none"
    )]
    pub depends_on: Option<DependsOnList>,
    /// Rejected by validation: renamed to `depends-on`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crates: Option<toml::Value>,
    #[serde(default, skip_serializing_if = "PredicateSet::is_empty")]
    pub predicates: PredicateSet,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<RawPluginSource>,
}

/// A `[[plugins]]` chained reference.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawChainedPlugin {
    #[serde(
        default,
        rename = "depends-on",
        skip_serializing_if = "Option::is_none"
    )]
    pub depends_on: Option<DependsOnList>,
    #[serde(default, skip_serializing_if = "PredicateSet::is_empty")]
    pub predicates: PredicateSet,
    pub source: RawChainedSource,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawChainedSource {
    /// Dependency-atom string (`source.cargo = "widget>=1"`) or explicit
    /// table (`source.cargo = { name = "widget", version = ">=1" }`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cargo: Option<RawChainedCargo>,
    /// Not yet implemented — reserved so the error is a clear message rather
    /// than an unknown-field parse failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<toml::Value>,
    /// Not yet implemented — reserved like `git`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<toml::Value>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum RawChainedCargo {
    Atom(String),
    Table(RawChainedCargoTable),
    /// Anything else — rejected with a migration hint.
    Other(toml::Value),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawChainedCargoTable {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Raw command reference as it appears in TOML: a string (named installation
/// reference) or an inline installation table.
///
/// Inline forms are promoted at validation time into synthetic
/// `[[installations]]` entries, so a validated plugin only ever stores
/// installation references as plain names.
#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum RawInstallationRef {
    Named(String),
    Inline(RawInlineInstallation),
}

/// Inline installation table. Carries the same fields as a
/// `[[installations]]` entry minus `name`.
#[derive(Debug, Deserialize, Serialize)]
pub struct RawInlineInstallation {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub install_commands: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requirements: Vec<RawInstallationRef>,
    #[serde(flatten, default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
}

/// A `[[installations]]` entry.
#[derive(Debug, Deserialize, Serialize)]
pub struct RawNamedInstallation {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requirements: Vec<RawInstallationRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub install_commands: Vec<String>,
    #[serde(flatten, default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
}

/// A `[[predicate]]` entry: a plugin-defined predicate.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawCustomPredicate {
    pub name: String,
    /// Named installation or inline installation table.
    pub command: RawInstallationRef,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
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

/// Raw `[subcommand.<name>]` entry. The TOML table key is the subcommand
/// name; this struct carries the table body.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawSubcommand {
    pub description: String,
    #[serde(default)]
    pub audience: Audience,
    /// Named installation (`"my-install"`) or inline installation table —
    /// same shape as `RawHook.command`.
    pub command: RawInstallationRef,
    #[serde(
        default,
        rename = "depends-on",
        skip_serializing_if = "Option::is_none"
    )]
    pub depends_on: Option<DependsOnList>,
    /// Rejected by validation: renamed to `depends-on`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crates: Option<toml::Value>,
    #[serde(default, skip_serializing_if = "PredicateSet::is_empty")]
    pub predicates: PredicateSet,
}

/// The wire format a plugin hook expects for input/output.
///
/// Distinct from [`HookAgent`] because `Symposium` is a wire format but not an
/// agent, and not every agent has a shell-hook JSON format.
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
    /// Convert to the corresponding [`HookAgent`], if this is an agent format.
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

/// A `[[hooks]]` entry.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawHook {
    pub name: String,
    pub event: HookEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<HookAgent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matcher: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requirements: Vec<RawInstallationRef>,
    /// Named installation (`"my-install"`) or inline installation table.
    pub command: RawInstallationRef,
    /// What to run from the installation. Across hook + installation, at most
    /// one of `executable` / `script` may be set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    /// Invocation arguments. Forbidden when the installation also declares `args`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default)]
    pub format: HookFormat,
    #[serde(default, skip_serializing_if = "PredicateSet::is_empty")]
    pub predicates: PredicateSet,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The manifest is the wire form between a package manager and Symposium,
    /// so it has to survive a JSON round trip without losing anything.
    #[test]
    fn manifest_round_trips_through_json() {
        let src = r#"
name = "demo"
depends-on = ["serde", "tokio>=1.0.0"]
predicates = ["env(CI)"]

[defaults]
skills = false

[[skills]]
source.path = "extra"
predicates = ["workspace-member()"]

[[skills]]
source.git = "https://github.com/org/repo"

[[installations]]
name = "linter"
source = "cargo"
package = "my-linter"
executable = "my-linter"

[[hooks]]
name = "lint"
event = "PreToolUse"
command = "linter"
format = "claude"
args = ["--strict"]

[[mcp_servers]]
name = "srv"
command = "srv-bin"
args = ["--stdio"]
env = []
depends-on = "serde"

[[plugins]]
source.cargo = "widget>=1"

[[predicate]]
name = "my_pred"
command = "linter"

[subcommand.doit]
description = "does it"
audience = "humans"
command = "linter"
"#;
        let parsed = RawPluginManifest::parse(src).expect("fixture parses");
        let json = serde_json::to_string(&parsed).expect("serializes");
        let back: RawPluginManifest = serde_json::from_str(&json).expect("deserializes");

        // Compare by re-serializing: the schema has no PartialEq (toml::Value
        // and McpServer don't uniformly provide one), so the JSON text is the
        // equality we can assert.
        let json2 = serde_json::to_string(&back).expect("re-serializes");
        assert_eq!(json, json2);
    }

    #[test]
    fn empty_manifest_round_trips_to_an_empty_object() {
        let parsed = RawPluginManifest::parse("").unwrap();
        assert_eq!(serde_json::to_string(&parsed).unwrap(), "{}");
    }

    #[test]
    fn merge_appends_lists_and_overrides_scalars() {
        let base = RawPluginManifest::parse(
            r#"
name = "base"
depends-on = ["serde"]
[[skills]]
source.path = "a"
"#,
        )
        .unwrap();
        let over = RawPluginManifest::parse(
            r#"
name = "over"
depends-on = ["tokio"]
[[skills]]
source.path = "b"
"#,
        )
        .unwrap();

        let merged = base.merge(over);
        assert_eq!(merged.name.as_deref(), Some("over"));
        assert_eq!(merged.skills.len(), 2);
        assert_eq!(merged.depends_on.0.len(), 2);
    }

    #[test]
    fn retired_fields_still_parse_so_validation_can_explain() {
        let parsed = RawPluginManifest::parse(r#"crates = ["serde"]"#).unwrap();
        assert!(parsed.crates.is_some());

        let parsed = RawPluginManifest::parse(
            r#"
[[skills]]
source.crate = { name = "x" }
"#,
        )
        .unwrap();
        match parsed.skills[0].source.as_ref().unwrap() {
            RawPluginSource::Table(t) => assert!(t.crate_field.is_some()),
            _ => panic!("expected a source table"),
        }
    }
}
