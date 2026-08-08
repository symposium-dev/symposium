//! Predicate *syntax*: the tree, its two surface spellings, and parsing.
//!
//! A predicate gates activation of a plugin or one of its components. This
//! module owns everything about what a predicate *is*: the [`Predicate`] tree,
//! parsing, `Display`, and serde, but nothing about evaluating one. Evaluation
//! needs the workspace dependency graph and the live environment, so it lives
//! in Symposium rather than here.
//!
//! That split is what lets a package manager parse and re-emit a manifest
//! without linking any of Symposium's evaluation machinery.
//!
//! Two surface syntaxes lower to the same tree:
//!
//! - The `depends-on` field uses **dependency-atom** syntax (`serde`,
//!   `serde>=1.0`, `*`) and lowers to `depends-on(...)` / `depends-on(*)`
//!   predicates, OR-combined into a single `any(...)` that is appended to the
//!   same predicate list.
//! - The `predicates` field uses **function-call** syntax:
//!   - `depends-on(<atom>)` — a workspace dependency is present (and its version
//!     satisfies the optional requirement); `depends-on(*)` matches any workspace.
//!   - `shell(<command>)` — `sh -c <command>` exits 0.
//!   - `path_exists(<arg>)` — `<arg>` exists on disk, falling back to a `$PATH`
//!     lookup for bare names.
//!   - `env(<name>)` / `env(<name>=<value>)` — env var presence / equality.
//!   - `workspace-member()`: the plugin this predicate belongs to is defined by
//!     a member of the active workspace.
//!   - `not(<p>)` — negation.
//!   - `any(<p>, …)` — OR.
//!   - `all(<p>, …)` — AND.
//!
//! Within a [`PredicateSet`] the entries are ANDed.

use anyhow::{Context, Result, bail};

/// Names reserved for builtin predicates. Custom predicates must not use
/// these. `crate` is retired syntax but stays reserved so a custom predicate
/// can never squat on it.
pub const BUILTIN_PREDICATE_NAMES: &[&str] = &[
    "depends-on",
    "crate",
    "shell",
    "path_exists",
    "env",
    "workspace-member",
    "not",
    "any",
    "all",
];

/// A single predicate node.
#[derive(Debug, Clone, PartialEq)]
pub enum Predicate {
    /// `depends-on(<name>)` / `depends-on(<name><req>)` — a workspace dep matches.
    DependsOn(String, Option<semver::VersionReq>),
    /// `depends-on(*)` / bare `*` — matches any workspace (even with zero deps).
    DependsOnWildcard,
    /// `shell(<command>)` — passes when `sh -c <command>` exits 0.
    Shell(String),
    /// `path_exists(<arg>)` — passes when `<arg>` exists (disk, then `$PATH`).
    PathExists(String),
    /// `env(<name>)` / `env(<name>=<value>)` — env var presence / equality.
    Env(String, Option<String>),
    /// `workspace-member()` — the plugin being evaluated is defined by a
    /// member of the active workspace (provenance, stamped by the loader).
    /// Selects content by audience: gate a component on it to activate only
    /// for people developing the defining package, not for dependents.
    WorkspaceMember,
    /// `not(<p>)` — passes when the inner predicate does not.
    Not(Box<Predicate>),
    /// `any(<p>, …)` — passes when at least one inner predicate does.
    Any(Vec<Predicate>),
    /// `all(<p>, …)` — passes when every inner predicate does.
    All(Vec<Predicate>),
    /// A plugin-defined predicate evaluated by spawning an external command.
    Custom { name: String, arg: String },
}

impl Predicate {
    /// Returns true if this predicate references the given dependency name
    /// anywhere (including inside combinators and negations).
    pub fn references_dep(&self, name: &str) -> bool {
        match self {
            Predicate::DependsOn(n, _) => n == name,
            Predicate::Not(p) => p.references_dep(name),
            Predicate::Any(v) | Predicate::All(v) => v.iter().any(|p| p.references_dep(name)),
            Predicate::Custom { .. } => false,
            _ => false,
        }
    }

    /// True if this predicate mentions any dependency (concrete or
    /// `depends-on(*)`).
    pub fn mentions_dep(&self) -> bool {
        match self {
            Predicate::DependsOn(..) | Predicate::DependsOnWildcard => true,
            Predicate::Not(p) => p.mentions_dep(),
            Predicate::Any(v) | Predicate::All(v) => v.iter().any(Predicate::mentions_dep),
            Predicate::Custom { .. } => false,
            _ => false,
        }
    }

    /// True if this predicate names a *concrete* dependency
    /// (`depends-on(serde)`), as opposed to only `depends-on(*)`.
    /// Non-allocating — used on the hook hot path.
    pub fn has_concrete_dep(&self) -> bool {
        match self {
            Predicate::DependsOn(..) => true,
            Predicate::Not(p) => p.has_concrete_dep(),
            Predicate::Any(v) | Predicate::All(v) => v.iter().any(Predicate::has_concrete_dep),
            Predicate::Custom { .. } => false,
            _ => false,
        }
    }

    /// Collect every dependency name referenced anywhere in this predicate.
    ///
    /// Used for crates.io existence validation, so it ignores tree position
    /// (a dependency named under `not(...)` is still validated). Custom
    /// predicates are a no-op — their names are dynamic.
    pub fn collect_dep_names(&self, out: &mut std::collections::BTreeSet<String>) {
        match self {
            Predicate::DependsOn(name, _) => {
                out.insert(name.clone());
            }
            Predicate::Not(p) => p.collect_dep_names(out),
            Predicate::Any(v) | Predicate::All(v) => {
                for p in v {
                    p.collect_dep_names(out);
                }
            }
            Predicate::Custom { .. } => {}
            _ => {}
        }
    }
}

/// A list of predicates, ANDed together.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PredicateSet {
    pub predicates: Vec<Predicate>,
}

impl PredicateSet {
    /// Parse a comma-separated list of **function-call** predicate expressions.
    pub fn parse(input: &str) -> Result<Self> {
        Ok(Self {
            predicates: parse_comma_separated(input)?,
        })
    }

    /// Build a set from **dependency-atom** syntax (the `depends-on` field),
    /// lowering the OR-combined atoms into a single `any(...)` predicate.
    /// Empty input yields an empty set.
    pub fn from_depends_on(input: &str) -> Result<Self> {
        Ok(Self {
            predicates: DependsOnList::parse(input)?
                .into_predicate()
                .into_iter()
                .collect(),
        })
    }

    /// Combine a lowered `depends-on` field with a `predicates` field into one
    /// set. The `depends-on` atoms become a single leading `any(...)` predicate.
    pub fn merged(depends_on: Option<DependsOnList>, predicates: PredicateSet) -> PredicateSet {
        let mut list = Vec::new();
        if let Some(p) = depends_on.and_then(DependsOnList::into_predicate) {
            list.push(p);
        }
        list.extend(predicates.predicates);
        PredicateSet { predicates: list }
    }

    pub fn is_empty(&self) -> bool {
        self.predicates.is_empty()
    }

    pub fn collect_dep_names(&self, out: &mut std::collections::BTreeSet<String>) {
        for p in &self.predicates {
            p.collect_dep_names(out);
        }
    }

    /// True if any `depends-on(...)` predicate (non-wildcard) appears anywhere.
    pub fn has_concrete_dep(&self) -> bool {
        self.predicates.iter().any(Predicate::has_concrete_dep)
    }

    /// True if any dependency predicate (including `depends-on(*)`) appears
    /// anywhere.
    pub fn mentions_dep(&self) -> bool {
        self.predicates.iter().any(Predicate::mentions_dep)
    }

    /// True if any predicate references the given dependency name.
    pub fn references_dep(&self, name: &str) -> bool {
        self.predicates.iter().any(|p| p.references_dep(name))
    }
}

// --- the `depends-on` field: a list of dependency atoms, OR-combined ---

/// The parsed `depends-on = [...]` field — a list of crate atoms. Lowers to a
/// single `any(...)` predicate appended to the enclosing predicate list.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DependsOnList(pub Vec<Predicate>);

#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum RawDependsOnList {
    One(String),
    Many(Vec<String>),
}

impl DependsOnList {
    /// Parse comma-separated dependency atoms (`serde, tokio>=1.0, *`).
    ///
    /// Commas inside balanced parentheses are preserved so that custom
    /// predicates like `battery_pack(a, b)` are not split incorrectly.
    pub fn parse(input: &str) -> Result<Self> {
        let atoms = split_top_level(input)
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| {
                parse_dep_atom(s)
                    .with_context(|| format!("failed to parse depends-on predicate: {s:?}"))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self(atoms))
    }

    /// Lower to a single predicate (`any(...)` over the atoms), or `None` if
    /// empty. A single atom is returned directly rather than wrapped.
    pub fn into_predicate(self) -> Option<Predicate> {
        match self.0.len() {
            0 => None,
            1 => self.0.into_iter().next(),
            _ => Some(Predicate::Any(self.0)),
        }
    }
}

impl serde::Serialize for DependsOnList {
    /// Serialized back to the atom spellings, so a manifest round-trips.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for p in &self.0 {
            seq.serialize_element(&dep_atom_string(p))?;
        }
        seq.end()
    }
}

/// Render a lowered `depends-on` atom back to its surface spelling. The atom
/// field accepts only `DependsOn` / `DependsOnWildcard`, so nothing else can
/// appear here; anything else falls back to the function-call form rather than
/// panicking.
fn dep_atom_string(p: &Predicate) -> String {
    match p {
        Predicate::DependsOn(name, None) => name.clone(),
        Predicate::DependsOn(name, Some(req)) => format!("{name}{req}"),
        Predicate::DependsOnWildcard => "*".to_string(),
        other => other.to_string(),
    }
}

impl<'de> serde::Deserialize<'de> for DependsOnList {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Accept either a single string (`depends-on = "serde"`) or a sequence
        // (`depends-on = ["serde", "tokio>=1.0"]`).
        let atoms = match RawDependsOnList::deserialize(deserializer)? {
            RawDependsOnList::One(s) => vec![s],
            RawDependsOnList::Many(v) => v,
        };
        let predicates = atoms
            .iter()
            .map(|s| parse_dep_atom(s.trim()))
            .collect::<Result<Vec<_>>>()
            .map_err(serde::de::Error::custom)?;
        Ok(Self(predicates))
    }
}

// --- function-call predicate parsing ---

/// Validate that `name` is a legal custom predicate identifier:
/// `[a-zA-Z][a-zA-Z0-9_]*`, must not collide with a builtin name.
///
/// Shared by both the expression parser (encountering an unknown function
/// name) and the `[[predicate]]` definition validator.
pub fn validate_custom_predicate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("predicate name is empty");
    }
    if !name.as_bytes()[0].is_ascii_alphabetic() {
        bail!("predicate `{name}` must start with a letter");
    }
    if let Some(pos) = name.find(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
        bail!(
            "predicate `{name}` contains invalid character '{}' at position {pos} \
             (only ASCII alphanumeric and `_` allowed)",
            name.as_bytes()[pos] as char,
        );
    }
    if BUILTIN_PREDICATE_NAMES.contains(&name) {
        bail!("predicate `{name}` collides with a builtin predicate name");
    }
    Ok(())
}

/// Parse a single function-call predicate expression (`depends-on(serde)`,
/// `any(env(CI), shell(test -f x))`).
pub fn parse_predicate(input: &str) -> Result<Predicate> {
    let trimmed = input.trim();
    let Some(open) = trimmed.find('(') else {
        bail!("predicate {trimmed:?} is not a function call (expected `name(arg)`)");
    };
    if !trimmed.ends_with(')') {
        bail!("predicate {trimmed:?} must end with `)`");
    }
    let name = trimmed[..open].trim();
    // Everything between the first `(` and the final `)` is the argument; an
    // inner `)` (as in `shell(echo $(date))`) is preserved.
    let arg = trimmed[open + 1..trimmed.len() - 1].trim();

    match name {
        "depends-on" => parse_dep_atom(arg),
        "crate" => bail!("`crate({arg})` is no longer supported; use `depends-on({arg})` instead"),
        "shell" => Ok(Predicate::Shell(arg.to_string())),
        "path_exists" => Ok(Predicate::PathExists(arg.to_string())),
        "env" => parse_env(arg),
        "workspace-member" => {
            if !arg.is_empty() {
                bail!("`workspace-member()` takes no argument, got {arg:?}");
            }
            Ok(Predicate::WorkspaceMember)
        }
        "not" => Ok(Predicate::Not(Box::new(parse_predicate(arg)?))),
        "any" => {
            let preds = parse_comma_separated(arg)?;
            if preds.is_empty() {
                bail!("`any(...)` requires at least one predicate");
            }
            Ok(Predicate::Any(preds))
        }
        "all" => {
            let preds = parse_comma_separated(arg)?;
            if preds.is_empty() {
                bail!("`all(...)` requires at least one predicate");
            }
            Ok(Predicate::All(preds))
        }
        other => {
            validate_custom_predicate_name(other)?;
            Ok(Predicate::Custom {
                name: other.to_string(),
                arg: arg.to_string(),
            })
        }
    }
}

/// Parse a comma-separated list of function-call predicate expressions.
/// Commas inside parentheses are not separators.
pub fn parse_comma_separated(input: &str) -> Result<Vec<Predicate>> {
    split_top_level(input)
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(parse_predicate)
        .collect()
}

fn parse_env(arg: &str) -> Result<Predicate> {
    match arg.split_once('=') {
        Some((name, value)) => {
            let name = name.trim();
            if name.is_empty() {
                bail!("`env(...)` variable name must not be empty");
            }
            Ok(Predicate::Env(name.to_string(), Some(value.to_string())))
        }
        None => {
            if arg.is_empty() {
                bail!("`env(...)` requires a variable name");
            }
            Ok(Predicate::Env(arg.to_string(), None))
        }
    }
}

/// Split on top-level commas, ignoring commas nested inside `(...)`.
fn split_top_level(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth: i32 = 0;
    let mut current = String::new();
    for c in input.chars() {
        match c {
            '(' => {
                depth += 1;
                current.push(c);
            }
            ')' => {
                depth -= 1;
                current.push(c);
            }
            ',' if depth == 0 => out.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    out.push(current);
    out
}

// --- dependency-atom parsing (`serde`, `serde>=1.0`, `*`) ---

/// Parse a single dependency atom into a `DependsOn` / `DependsOnWildcard`
/// predicate.
pub fn parse_dep_atom(input: &str) -> Result<Predicate> {
    let input = input.trim();
    if input.is_empty() {
        bail!("empty depends-on predicate");
    }
    if input == "*" {
        return Ok(Predicate::DependsOnWildcard);
    }
    let mut parser = AtomParser::new(input);
    let pred = parser.parse_atom()?;
    parser.skip_whitespace();
    if parser.pos < parser.input.len() {
        bail!(
            "unexpected trailing input at position {}: {:?}",
            parser.pos,
            &parser.input[parser.pos..]
        );
    }
    Ok(pred)
}

struct AtomParser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> AtomParser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.input.len() && self.input.as_bytes()[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn remaining(&self) -> &'a str {
        &self.input[self.pos..]
    }

    fn parse_atom(&mut self) -> Result<Predicate> {
        self.skip_whitespace();
        let start = self.pos;

        // Consume dependency name: [a-zA-Z0-9_-]+
        while self.pos < self.input.len() {
            let c = self.input.as_bytes()[self.pos];
            if c.is_ascii_alphanumeric() || c == b'_' || c == b'-' {
                self.pos += 1;
            } else {
                break;
            }
        }

        let name = &self.input[start..self.pos];
        if name.is_empty() {
            bail!(
                "expected dependency name at position {}: {:?}",
                start,
                self.remaining()
            );
        }

        // Function-call syntax is NOT valid in dependency-atom position. The
        // `depends-on` field accepts only bare names + optional version
        // constraints. Full predicate expressions (including custom
        // predicates) belong in the `predicates` field.
        if self.pos < self.input.len() && self.input.as_bytes()[self.pos] == b'(' {
            bail!(
                "function-call syntax `{name}(...)` is not valid in the `depends-on` field; \
                 use the `predicates` field instead"
            );
        }

        // Version constraint (starts with >=, <=, >, <, =, ^, ~). Bare `=` is
        // treated as `^` (compatible), matching Cargo's default.
        let version_req = if self.pos < self.input.len() {
            let next = self.input.as_bytes()[self.pos];
            if matches!(next, b'>' | b'<' | b'=' | b'^' | b'~') {
                let vstart = self.pos;
                while self.pos < self.input.len() {
                    let c = self.input.as_bytes()[self.pos];
                    if c.is_ascii_whitespace() {
                        break;
                    }
                    self.pos += 1;
                }
                let raw = self.input[vstart..self.pos].trim();
                let constraint = if let Some(rest) = raw.strip_prefix("==") {
                    std::borrow::Cow::Owned(format!("={rest}"))
                } else if let Some(rest) = raw.strip_prefix('=') {
                    std::borrow::Cow::Owned(format!("^{rest}"))
                } else {
                    std::borrow::Cow::Borrowed(raw)
                };
                Some(semver::VersionReq::parse(&constraint)?)
            } else {
                None
            }
        } else {
            None
        };

        Ok(Predicate::DependsOn(name.to_string(), version_req))
    }
}

// --- serde + Display ---

impl serde::Serialize for Predicate {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for Predicate {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        parse_predicate(&s).map_err(serde::de::Error::custom)
    }
}

impl serde::Serialize for PredicateSet {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.predicates.serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for PredicateSet {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self {
            predicates: Vec::<Predicate>::deserialize(deserializer)?,
        })
    }
}

impl std::fmt::Display for Predicate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Predicate::DependsOn(name, None) => write!(f, "depends-on({name})"),
            Predicate::DependsOn(name, Some(req)) => write!(f, "depends-on({name}{req})"),
            Predicate::DependsOnWildcard => write!(f, "depends-on(*)"),
            Predicate::Shell(cmd) => write!(f, "shell({cmd})"),
            Predicate::PathExists(arg) => write!(f, "path_exists({arg})"),
            Predicate::Env(name, None) => write!(f, "env({name})"),
            Predicate::Env(name, Some(value)) => write!(f, "env({name}={value})"),
            Predicate::WorkspaceMember => write!(f, "workspace-member()"),
            Predicate::Not(inner) => write!(f, "not({inner})"),
            Predicate::Any(preds) => write!(f, "any({})", join(preds)),
            Predicate::All(preds) => write!(f, "all({})", join(preds)),
            Predicate::Custom { name, arg } => write!(f, "{name}({arg})"),
        }
    }
}

fn join(preds: &[Predicate]) -> String {
    preds
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predicate_round_trips_through_its_display_form() {
        for input in [
            "depends-on(serde)",
            "depends-on(serde>=1.0.0)",
            "depends-on(*)",
            "shell(which cargo-nextest)",
            "path_exists(build.rs)",
            "env(CI)",
            "env(MODE=release)",
            "workspace-member()",
            "not(depends-on(serde))",
            "any(depends-on(serde), depends-on(tokio))",
            "all(depends-on(serde), env(CI))",
            "my_pred(arg)",
        ] {
            let parsed = parse_predicate(input).unwrap();
            let redisplayed = parsed.to_string();
            assert_eq!(
                parse_predicate(&redisplayed).unwrap(),
                parsed,
                "{input} did not round-trip (rendered as {redisplayed})"
            );
        }
    }

    #[test]
    fn depends_on_list_round_trips_through_json() {
        let list: DependsOnList =
            serde_json::from_str(r#"["serde", "tokio>=1.0.0", "*"]"#).unwrap();
        let json = serde_json::to_string(&list).unwrap();
        let back: DependsOnList = serde_json::from_str(&json).unwrap();
        assert_eq!(back, list);
    }

    #[test]
    fn depends_on_accepts_a_bare_string() {
        let list: DependsOnList = serde_json::from_str(r#""serde""#).unwrap();
        assert_eq!(list.0, vec![Predicate::DependsOn("serde".into(), None)]);
    }
}
