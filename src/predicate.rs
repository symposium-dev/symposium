//! Predicates that gate plugin / skill / hook / MCP / subcommand activation.
//!
//! A predicate is a boolean expression evaluated against the workspace
//! dependency graph and the live environment. Two surface syntaxes lower to the
//! same [`Predicate`] tree:
//!
//! - The `crates` field uses **crate-atom** syntax (`serde`, `serde>=1.0`, `*`)
//!   and lowers to `crate(...)` / `crate(*)` predicates, OR-combined into a
//!   single `any(...)` that is appended to the same predicate list.
//! - The `predicates` field uses **function-call** syntax:
//!   - `crate(<atom>)` — a workspace dependency is present (and its version
//!     satisfies the optional requirement); `crate(*)` matches any workspace.
//!   - `shell(<command>)` — `sh -c <command>` exits 0.
//!   - `path_exists(<arg>)` — `<arg>` exists on disk, falling back to a `$PATH`
//!     lookup for bare names.
//!   - `env(<name>)` / `env(<name>=<value>)` — env var presence / equality.
//!   - `not(<p>)` — negation.
//!   - `any(<p>, …)` — OR.
//!   - `all(<p>, …)` — AND.
//!
//! Within a [`PredicateSet`] the entries are ANDed.
//!
//! Besides the boolean gate ([`PredicateSet::evaluate`]), predicates carry a
//! **witness**: the set of workspace crates that participate in a satisfying
//! evaluation. This drives `source = "crate"` skill resolution — see
//! [`PredicateSet::witness`] and [`union_matched_crates`].

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

/// The evaluation environment a predicate is checked against.
///
/// The crate graph is passed explicitly; the OS environment (`shell`,
/// `path_exists`, `env`) is read ambiently at evaluation time.
#[derive(Debug, Clone, Copy)]
pub struct PredicateContext<'a> {
    pub crates: &'a [(String, semver::Version)],
}

impl<'a> PredicateContext<'a> {
    pub fn new(crates: &'a [(String, semver::Version)]) -> Self {
        Self { crates }
    }
}

/// A single predicate node.
#[derive(Debug, Clone, PartialEq)]
pub enum Predicate {
    /// `crate(<name>)` / `crate(<name><req>)` — a workspace dep matches.
    Crate(String, Option<semver::VersionReq>),
    /// `crate(*)` / bare `*` — matches any workspace (even with zero deps).
    CrateWildcard,
    /// `shell(<command>)` — passes when `sh -c <command>` exits 0.
    Shell(String),
    /// `path_exists(<arg>)` — passes when `<arg>` exists (disk, then `$PATH`).
    PathExists(String),
    /// `env(<name>)` / `env(<name>=<value>)` — env var presence / equality.
    Env(String, Option<String>),
    /// `not(<p>)` — passes when the inner predicate does not.
    Not(Box<Predicate>),
    /// `any(<p>, …)` — passes when at least one inner predicate does.
    Any(Vec<Predicate>),
    /// `all(<p>, …)` — passes when every inner predicate does.
    All(Vec<Predicate>),
}

impl Predicate {
    /// True if this predicate holds in `ctx`.
    ///
    /// Short-circuits (`any` stops at the first true child, `all` at the first
    /// false) and allocates nothing — this is the gating hot path. Use
    /// [`Predicate::witness`] when the satisfying crate set is also needed.
    pub fn evaluate(&self, ctx: &PredicateContext) -> bool {
        match self {
            Predicate::Crate(name, version_req) => ctx.crates.iter().any(|(dep_name, dep_ver)| {
                dep_name == name && version_req.as_ref().is_none_or(|req| req.matches(dep_ver))
            }),
            Predicate::CrateWildcard => true,
            Predicate::Shell(cmd) => run_shell(cmd),
            Predicate::PathExists(arg) => path_exists(arg),
            Predicate::Env(name, expected) => env_matches(name, expected.as_deref()),
            Predicate::Not(inner) => !inner.evaluate(ctx),
            Predicate::Any(children) => children.iter().any(|p| p.evaluate(ctx)),
            Predicate::All(children) => children.iter().all(|p| p.evaluate(ctx)),
        }
    }

    /// Evaluate, returning `None` when false and `Some(witness)` when true.
    ///
    /// The witness is the set of workspace crates that participate in the
    /// satisfying evaluation: `crate(c)` contributes `c` when present, `any`
    /// unions the witnesses of its *true* children, `all` unions all children's
    /// witnesses (when all hold), and `not` contributes nothing (negation is
    /// about absence). Non-crate leaves contribute an empty witness.
    pub fn witness(&self, ctx: &PredicateContext) -> Option<Vec<(String, semver::Version)>> {
        match self {
            Predicate::Crate(name, version_req) => {
                let hits: Vec<_> = ctx
                    .crates
                    .iter()
                    .filter(|(dep_name, dep_ver)| {
                        dep_name == name
                            && version_req.as_ref().is_none_or(|req| req.matches(dep_ver))
                    })
                    .cloned()
                    .collect();
                if hits.is_empty() { None } else { Some(hits) }
            }
            Predicate::CrateWildcard => Some(Vec::new()),
            Predicate::Shell(cmd) => run_shell(cmd).then(Vec::new),
            Predicate::PathExists(arg) => path_exists(arg).then(Vec::new),
            Predicate::Env(name, expected) => env_matches(name, expected.as_deref()).then(Vec::new),
            Predicate::Not(inner) => match inner.witness(ctx) {
                Some(_) => None,
                None => Some(Vec::new()),
            },
            Predicate::Any(children) => {
                let mut crates = Vec::new();
                let mut any_true = false;
                for child in children {
                    if let Some(w) = child.witness(ctx) {
                        any_true = true;
                        crates.extend(w);
                    }
                }
                any_true.then_some(crates)
            }
            Predicate::All(children) => {
                let mut crates = Vec::new();
                for child in children {
                    crates.extend(child.witness(ctx)?);
                }
                Some(crates)
            }
        }
    }

    /// Returns true if this predicate references the given crate name anywhere
    /// (including inside combinators and negations).
    pub fn references_crate(&self, name: &str) -> bool {
        match self {
            Predicate::Crate(n, _) => n == name,
            Predicate::Not(p) => p.references_crate(name),
            Predicate::Any(v) | Predicate::All(v) => v.iter().any(|p| p.references_crate(name)),
            _ => false,
        }
    }

    /// True if this predicate mentions any crate (concrete or `crate(*)`).
    pub fn mentions_crate(&self) -> bool {
        match self {
            Predicate::Crate(..) | Predicate::CrateWildcard => true,
            Predicate::Not(p) => p.mentions_crate(),
            Predicate::Any(v) | Predicate::All(v) => v.iter().any(Predicate::mentions_crate),
            _ => false,
        }
    }

    /// True if this predicate names a *concrete* crate (`crate(serde)`), as
    /// opposed to only `crate(*)`. Non-allocating — used on the hook hot path.
    pub fn has_concrete_crate(&self) -> bool {
        match self {
            Predicate::Crate(..) => true,
            Predicate::Not(p) => p.has_concrete_crate(),
            Predicate::Any(v) | Predicate::All(v) => v.iter().any(Predicate::has_concrete_crate),
            _ => false,
        }
    }

    /// Collect every crate name referenced anywhere in this predicate.
    ///
    /// Used for crates.io existence validation, so it ignores tree position
    /// (a crate named under `not(...)` is still validated).
    pub fn collect_crate_names(&self, out: &mut std::collections::BTreeSet<String>) {
        match self {
            Predicate::Crate(name, _) => {
                out.insert(name.clone());
            }
            Predicate::Not(p) => p.collect_crate_names(out),
            Predicate::Any(v) | Predicate::All(v) => {
                for p in v {
                    p.collect_crate_names(out);
                }
            }
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

    /// Build a set from **crate-atom** syntax (the `crates` field), lowering the
    /// OR-combined atoms into a single `any(...)` predicate. Empty input yields
    /// an empty set.
    pub fn from_crates(input: &str) -> Result<Self> {
        Ok(Self {
            predicates: CrateList::parse(input)?
                .into_predicate()
                .into_iter()
                .collect(),
        })
    }

    /// Combine a lowered `crates` field with a `predicates` field into one set.
    /// The `crates` atoms become a single leading `any(...)` predicate.
    pub fn merged(crates: Option<CrateList>, predicates: PredicateSet) -> PredicateSet {
        let mut list = Vec::new();
        if let Some(p) = crates.and_then(CrateList::into_predicate) {
            list.push(p);
        }
        list.extend(predicates.predicates);
        PredicateSet { predicates: list }
    }

    /// True if every predicate holds (or the set is empty).
    pub fn evaluate(&self, ctx: &PredicateContext) -> bool {
        self.predicates.iter().all(|p| p.evaluate(ctx))
    }

    /// Witness for the whole set (treated as one big `all(...)`): `None` if any
    /// predicate is false, otherwise the deduplicated union of witnesses.
    pub fn witness(&self, ctx: &PredicateContext) -> Option<Vec<(String, semver::Version)>> {
        let mut crates = Vec::new();
        for p in &self.predicates {
            crates.extend(p.witness(ctx)?);
        }
        Some(dedup_crates(crates))
    }

    pub fn is_empty(&self) -> bool {
        self.predicates.is_empty()
    }

    pub fn collect_crate_names(&self, out: &mut std::collections::BTreeSet<String>) {
        for p in &self.predicates {
            p.collect_crate_names(out);
        }
    }

    /// True if any `crate(...)` predicate (non-wildcard) appears anywhere.
    pub fn has_concrete_crate(&self) -> bool {
        self.predicates.iter().any(Predicate::has_concrete_crate)
    }

    /// True if any crate predicate (including `crate(*)`) appears anywhere.
    pub fn mentions_crate(&self) -> bool {
        self.predicates.iter().any(Predicate::mentions_crate)
    }

    /// True if any predicate references the given crate name.
    pub fn references_crate(&self, name: &str) -> bool {
        self.predicates.iter().any(|p| p.references_crate(name))
    }
}

/// Union the witnesses of several predicate sets, deduplicated by crate name.
///
/// A set whose gate is false contributes nothing. Drives `source = "crate"`
/// resolution: the concrete crates whose source trees to fetch skills from.
pub fn union_matched_crates(
    sets: &[&PredicateSet],
    ctx: &PredicateContext,
) -> Vec<(String, semver::Version)> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for set in sets {
        if let Some(matched) = set.witness(ctx) {
            for pair in matched {
                if seen.insert(pair.0.clone()) {
                    result.push(pair);
                }
            }
        }
    }
    result
}

fn dedup_crates(crates: Vec<(String, semver::Version)>) -> Vec<(String, semver::Version)> {
    let mut seen = std::collections::HashSet::new();
    crates
        .into_iter()
        .filter(|(name, _)| seen.insert(name.clone()))
        .collect()
}

// --- the `crates` field: a list of crate atoms, OR-combined ---

/// The parsed `crates = [...]` field — a list of crate atoms. Lowers to a
/// single `any(...)` predicate appended to the enclosing predicate list.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CrateList(pub Vec<Predicate>);

impl CrateList {
    /// Parse comma-separated crate atoms (`serde, tokio>=1.0, *`).
    pub fn parse(input: &str) -> Result<Self> {
        let atoms = input
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| {
                parse_crate_atom(s)
                    .with_context(|| format!("failed to parse crate predicate: {s:?}"))
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

impl<'de> serde::Deserialize<'de> for CrateList {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Accept either a single string (`crates = "serde"`) or a sequence
        // (`crates = ["serde", "tokio>=1.0"]`).
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum Raw {
            One(String),
            Many(Vec<String>),
        }
        let atoms = match Raw::deserialize(deserializer)? {
            Raw::One(s) => vec![s],
            Raw::Many(v) => v,
        };
        let predicates = atoms
            .iter()
            .map(|s| parse_crate_atom(s.trim()))
            .collect::<Result<Vec<_>>>()
            .map_err(serde::de::Error::custom)?;
        Ok(Self(predicates))
    }
}

// --- function-call predicate parsing ---

/// Parse a single function-call predicate expression.
pub fn parse(input: &str) -> Result<Predicate> {
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
        "crate" => parse_crate_atom(arg),
        "shell" => Ok(Predicate::Shell(arg.to_string())),
        "path_exists" => Ok(Predicate::PathExists(arg.to_string())),
        "env" => parse_env(arg),
        "not" => Ok(Predicate::Not(Box::new(parse(arg)?))),
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
        other => bail!(
            "unknown predicate `{other}` \
             (expected `crate`, `shell`, `path_exists`, `env`, `not`, `any`, or `all`)"
        ),
    }
}

/// Parse a comma-separated list of function-call predicate expressions.
/// Commas inside parentheses are not separators.
pub fn parse_comma_separated(input: &str) -> Result<Vec<Predicate>> {
    split_top_level(input)
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(parse)
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

// --- crate-atom parsing (`serde`, `serde>=1.0`, `*`) ---

/// Parse a single crate atom into a `Crate` / `CrateWildcard` predicate.
pub fn parse_crate_atom(input: &str) -> Result<Predicate> {
    let input = input.trim();
    if input.is_empty() {
        bail!("empty crate predicate");
    }
    if input == "*" {
        return Ok(Predicate::CrateWildcard);
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

        // Consume crate name: [a-zA-Z0-9_-]+
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
                "expected crate name at position {}: {:?}",
                start,
                self.remaining()
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

        Ok(Predicate::Crate(name.to_string(), version_req))
    }
}

// --- environment evaluation ---

fn env_matches(name: &str, expected: Option<&str>) -> bool {
    match expected {
        None => std::env::var_os(name).is_some(),
        Some(value) => std::env::var(name).ok().as_deref() == Some(value),
    }
}

fn run_shell(command: &str) -> bool {
    match Command::new("sh").arg("-c").arg(command).output() {
        Ok(out) if out.status.success() => {
            tracing::trace!(command = %command, "shell predicate passed");
            true
        }
        Ok(out) => {
            tracing::trace!(
                command = %command,
                exit_code = ?out.status.code(),
                stderr = %String::from_utf8_lossy(&out.stderr),
                "shell predicate failed",
            );
            false
        }
        Err(e) => {
            tracing::trace!(command = %command, error = %e, "shell predicate failed to spawn");
            false
        }
    }
}

fn path_exists(arg: &str) -> bool {
    if arg.is_empty() {
        return false;
    }
    if Path::new(arg).exists() {
        return true;
    }
    if arg.contains('/') || arg.contains(std::path::MAIN_SEPARATOR) {
        return false;
    }
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(arg).exists()))
        .unwrap_or(false)
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
        parse(&s).map_err(serde::de::Error::custom)
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
            Predicate::Crate(name, None) => write!(f, "crate({name})"),
            Predicate::Crate(name, Some(req)) => write!(f, "crate({name}{req})"),
            Predicate::CrateWildcard => write!(f, "crate(*)"),
            Predicate::Shell(cmd) => write!(f, "shell({cmd})"),
            Predicate::PathExists(arg) => write!(f, "path_exists({arg})"),
            Predicate::Env(name, None) => write!(f, "env({name})"),
            Predicate::Env(name, Some(value)) => write!(f, "env({name}={value})"),
            Predicate::Not(inner) => write!(f, "not({inner})"),
            Predicate::Any(preds) => write!(f, "any({})", join(preds)),
            Predicate::All(preds) => write!(f, "all({})", join(preds)),
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

    fn v(s: &str) -> semver::Version {
        semver::Version::parse(s).unwrap()
    }

    fn ctx<'a>(crates: &'a [(String, semver::Version)]) -> PredicateContext<'a> {
        PredicateContext::new(crates)
    }

    fn ws(pairs: &[(&str, &str)]) -> Vec<(String, semver::Version)> {
        pairs
            .iter()
            .map(|(n, ver)| (n.to_string(), v(ver)))
            .collect()
    }

    // --- crate-atom parsing ---

    #[test]
    fn parse_crate_atom_bare_and_versioned() {
        assert_eq!(
            parse_crate_atom("serde").unwrap(),
            Predicate::Crate("serde".into(), None)
        );
        assert_eq!(
            parse_crate_atom("serde>=1.0").unwrap(),
            Predicate::Crate(
                "serde".into(),
                Some(semver::VersionReq::parse(">=1.0").unwrap())
            )
        );
        assert_eq!(parse_crate_atom("*").unwrap(), Predicate::CrateWildcard);
    }

    #[test]
    fn crate_list_lowers_to_any() {
        assert_eq!(CrateList::parse("").unwrap().into_predicate(), None);
        assert_eq!(
            CrateList::parse("serde").unwrap().into_predicate(),
            Some(Predicate::Crate("serde".into(), None))
        );
        assert_eq!(
            CrateList::parse("serde, tokio").unwrap().into_predicate(),
            Some(Predicate::Any(vec![
                Predicate::Crate("serde".into(), None),
                Predicate::Crate("tokio".into(), None),
            ]))
        );
    }

    // --- function-call parsing ---

    #[test]
    fn parse_function_calls() {
        assert_eq!(
            parse("crate(serde)").unwrap(),
            Predicate::Crate("serde".into(), None)
        );
        assert_eq!(parse("crate(*)").unwrap(), Predicate::CrateWildcard);
        assert_eq!(
            parse("shell(command -v rg)").unwrap(),
            Predicate::Shell("command -v rg".into())
        );
        assert_eq!(parse("env(CI)").unwrap(), Predicate::Env("CI".into(), None));
        assert_eq!(
            parse("not(crate(serde))").unwrap(),
            Predicate::Not(Box::new(Predicate::Crate("serde".into(), None)))
        );
        assert_eq!(
            parse("any(crate(a), path_exists(rg))").unwrap(),
            Predicate::Any(vec![
                Predicate::Crate("a".into(), None),
                Predicate::PathExists("rg".into()),
            ])
        );
        assert!(parse("all()").is_err());
        assert!(parse("bogus(x)").is_err());
    }

    // --- evaluation ---

    #[test]
    fn evaluate_crate_and_wildcard() {
        let w = ws(&[("serde", "1.0.0")]);
        assert!(parse("crate(serde)").unwrap().evaluate(&ctx(&w)));
        assert!(!parse("crate(tokio)").unwrap().evaluate(&ctx(&w)));
        assert!(parse("crate(*)").unwrap().evaluate(&ctx(&[])));
    }

    #[test]
    fn evaluate_combinators() {
        let w = ws(&[("serde", "1.0.0")]);
        assert!(parse("not(crate(tokio))").unwrap().evaluate(&ctx(&w)));
        assert!(
            parse("any(crate(tokio), crate(serde))")
                .unwrap()
                .evaluate(&ctx(&w))
        );
        assert!(
            !parse("all(crate(serde), crate(tokio))")
                .unwrap()
                .evaluate(&ctx(&w))
        );
    }

    #[test]
    fn evaluate_agrees_with_witness() {
        // `evaluate` is a standalone short-circuiting path; it must agree with
        // `witness(...).is_some()` for every shape.
        let w = ws(&[("serde", "1.0.0")]);
        for input in [
            "crate(serde)",
            "crate(tokio)",
            "crate(*)",
            "not(crate(tokio))",
            "any(crate(tokio), shell(true))",
            "all(crate(serde), env(PATH))",
            "all(crate(serde), crate(tokio))",
            "not(any(crate(serde), env(PATH)))",
        ] {
            let p = parse(input).unwrap();
            assert_eq!(
                p.evaluate(&ctx(&w)),
                p.witness(&ctx(&w)).is_some(),
                "evaluate/witness disagree: {input}"
            );
        }
    }

    #[test]
    fn path_exists_empty_is_false() {
        // `path_exists()` must not resolve to a `$PATH` dir via `dir.join("")`.
        assert!(!Predicate::PathExists(String::new()).evaluate(&ctx(&[])));
    }

    // --- witness: the source="crate" fetch set ---

    #[test]
    fn witness_example_one_all_gates_crate2() {
        // any(crate(c1), all(crate(c2), env(USE_C2)))
        let p = parse("any(crate(c1), all(crate(c2), env(SYMPOSIUM_TEST_UNSET_XYZ)))").unwrap();
        let w = ws(&[("c1", "1.0.0"), ("c2", "1.0.0")]);
        // env unset -> all(...) is a dead branch -> only c1
        let names: Vec<_> = p
            .witness(&ctx(&w))
            .unwrap()
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        assert_eq!(names, vec!["c1"]);
    }

    #[test]
    fn witness_example_three_not_excludes_crate2() {
        // any(crate(c1), all(not(env(SKIP)), crate(c2))) with SKIP "set"
        // Model "SKIP set" by asserting against an env-equality we force true via
        // a value compare on an unset var is false; instead use a present var.
        let p = parse("any(crate(c1), all(not(env(PATH)), crate(c2)))").unwrap();
        let w = ws(&[("c1", "1.0.0"), ("c2", "1.0.0")]);
        // PATH is set -> not(env(PATH)) false -> all dead -> only c1
        let names: Vec<_> = p
            .witness(&ctx(&w))
            .unwrap()
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        assert_eq!(names, vec!["c1"]);
    }

    #[test]
    fn witness_unions_all_true_branches() {
        // any(crate(c1), any(env(PATH), crate(c2))) — both c1 and c2 present and
        // their crate(...) branches are independently true.
        let p = parse("any(crate(c1), any(env(PATH), crate(c2)))").unwrap();
        let w = ws(&[("c1", "1.0.0"), ("c2", "1.0.0")]);
        let mut names: Vec<_> = p
            .witness(&ctx(&w))
            .unwrap()
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        names.sort();
        assert_eq!(names, vec!["c1", "c2"]);
    }

    #[test]
    fn witness_false_gate_is_none() {
        let p = parse("crate(absent)").unwrap();
        assert!(p.witness(&ctx(&[])).is_none());
    }

    #[test]
    fn union_matched_crates_dedups_across_sets() {
        let plugin = PredicateSet::from_crates("serde").unwrap();
        let group = PredicateSet::from_crates("serde, tokio").unwrap();
        let w = ws(&[("serde", "1.0.0"), ("tokio", "1.0.0")]);
        let result = union_matched_crates(&[&plugin, &group], &ctx(&w));
        let mut names: Vec<_> = result.into_iter().map(|(n, _)| n).collect();
        names.sort();
        assert_eq!(names, vec!["serde", "tokio"]);
    }

    // --- introspection ---

    #[test]
    fn collect_and_references_walk_the_tree() {
        let p = parse("any(crate(serde), not(crate(tokio)))").unwrap();
        let mut names = std::collections::BTreeSet::new();
        p.collect_crate_names(&mut names);
        assert_eq!(
            names.into_iter().collect::<Vec<_>>(),
            vec!["serde", "tokio"]
        );
        assert!(p.references_crate("serde"));
        assert!(p.references_crate("tokio"));
        assert!(!p.references_crate("anyhow"));
    }

    #[test]
    fn has_concrete_crate() {
        assert!(
            PredicateSet::from_crates("serde")
                .unwrap()
                .has_concrete_crate()
        );
        assert!(!PredicateSet::from_crates("*").unwrap().has_concrete_crate());
        assert!(
            !PredicateSet::parse("shell(true)")
                .unwrap()
                .has_concrete_crate()
        );
    }

    // --- Display round-trip ---

    #[test]
    fn display_round_trip() {
        for input in [
            "crate(serde)",
            "crate(serde>=1.0)",
            "crate(*)",
            "shell(command -v rg)",
            "path_exists(rg)",
            "env(CI)",
            "env(MODE=debug)",
            "not(crate(serde))",
            "any(crate(a), path_exists(b))",
            "all(crate(a), not(env(CI)))",
        ] {
            let p = parse(input).unwrap();
            assert_eq!(p.to_string(), input, "display drift: {input}");
            assert_eq!(parse(&p.to_string()).unwrap(), p, "round-trip: {input}");
        }
    }

    // --- TOML deserialization of the two fields ---

    #[test]
    fn toml_fields_deserialize() {
        #[derive(serde::Deserialize)]
        struct Container {
            #[serde(default)]
            crates: CrateList,
            #[serde(default)]
            predicates: PredicateSet,
        }
        let c: Container = toml::from_str(
            r#"crates = ["serde", "tokio>=1.0"]
               predicates = ["path_exists(jq)", "not(crate(foo))"]"#,
        )
        .unwrap();
        assert_eq!(c.crates.0.len(), 2);
        assert_eq!(c.predicates.predicates.len(), 2);

        // single-string crates form
        let c2: Container = toml::from_str(r#"crates = "serde""#).unwrap();
        assert_eq!(c2.crates.0, vec![Predicate::Crate("serde".into(), None)]);
    }
}
