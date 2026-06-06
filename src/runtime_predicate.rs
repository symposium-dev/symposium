//! Runtime predicates that gate plugin / skill / hook / MCP activation.
//!
//! Each predicate is a small function-call expression. Leaf predicates take
//! their argument verbatim between the parentheses (no quoting); the
//! combinators `not` / `any` take nested predicates:
//!
//! - `shell(<command>)` — run `<command>` via `sh -c`; exit 0 means the
//!   predicate holds, any other exit (including spawn failure) means it fails.
//! - `path_exists(<arg>)` — holds when `<arg>` resolves to an existing path.
//!   An argument containing a path separator is checked directly on the
//!   filesystem (relative to the cwd, or absolute). A bare name with no
//!   separator is checked against the cwd first and then searched on `$PATH`,
//!   so it matches either a local entry (e.g. `path_exists(.git)`) or an
//!   installed binary (e.g. `path_exists(rg)`).
//! - `env(<name>)` — holds when the environment variable `<name>` is set.
//!   `env(<name>=<value>)` holds when it is set and equals `<value>`.
//! - `not(<predicate>)` — holds when the inner predicate does not.
//! - `any(<p>, <p>, …)` — holds when at least one inner predicate does.
//!
//! Predicates compose with **AND** semantics — within a level, every predicate
//! in the set must hold for that level to match. `any(...)` provides OR within
//! a single entry, and `not(...)` provides negation.
//!
//! Predicates are evaluated at the same points where [`crate::predicate`]
//! crate predicates are evaluated:
//!
//! - Plugin-level: at sync time (gating skills/MCP) and at hook dispatch.
//! - Skill-group / skill-level: at sync time.
//! - Hook-level: at hook dispatch time.

use std::path::Path;
use std::process::Command;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

/// A single runtime predicate, written as a function call in TOML / YAML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimePredicate {
    /// `shell(<command>)` — passes when `sh -c <command>` exits 0.
    Shell(String),
    /// `path_exists(<arg>)` — passes when `<arg>` exists on disk, falling
    /// back to a `$PATH` lookup for bare names.
    PathExists(String),
    /// `env(<name>)` — passes when `<name>` is set. `env(<name>=<value>)` —
    /// passes when `<name>` is set and equals `<value>`.
    Env(String, Option<String>),
    /// `not(<predicate>)` — passes when the inner predicate does not.
    Not(Box<RuntimePredicate>),
    /// `any(<p>, <p>, …)` — passes when at least one inner predicate does
    /// (OR; the enclosing list is AND).
    Any(Vec<RuntimePredicate>),
}

impl RuntimePredicate {
    /// Evaluate this predicate against the live environment.
    pub fn evaluate(&self) -> bool {
        match self {
            RuntimePredicate::Shell(cmd) => run_shell(cmd),
            RuntimePredicate::PathExists(arg) => path_exists(arg),
            RuntimePredicate::Env(name, expected) => match expected {
                None => std::env::var_os(name).is_some(),
                Some(value) => std::env::var(name).ok().as_deref() == Some(value.as_str()),
            },
            RuntimePredicate::Not(inner) => !inner.evaluate(),
            RuntimePredicate::Any(preds) => preds.iter().any(|p| p.evaluate()),
        }
    }
}

/// A set of runtime predicates that must all hold for the enclosing item
/// to be active.
///
/// Serialized as a plain list of predicate strings (`["shell(...)", ...]`).
/// The empty list is vacuously true and is omitted from serialized output.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RuntimePredicateSet {
    pub predicates: Vec<RuntimePredicate>,
}

impl RuntimePredicateSet {
    /// True if every predicate holds (or the set is empty).
    pub fn evaluate(&self) -> bool {
        self.predicates.iter().all(|p| p.evaluate())
    }

    pub fn is_empty(&self) -> bool {
        self.predicates.is_empty()
    }
}

/// Parse a single predicate expression, e.g. `shell(command -v rg)` or
/// `path_exists(/usr/bin/jq)`. Leaf arguments (`shell`, `path_exists`, `env`)
/// are taken verbatim between the parentheses — they are not quoted. The
/// combinators `not(...)` and `any(...)` parse their arguments as nested
/// predicates.
pub fn parse(input: &str) -> Result<RuntimePredicate> {
    let trimmed = input.trim();
    let Some(open) = trimmed.find('(') else {
        bail!("predicate {trimmed:?} is not a function call (expected `name(arg)`)");
    };
    if !trimmed.ends_with(')') {
        bail!("predicate {trimmed:?} must end with `)`");
    }
    let name = trimmed[..open].trim();
    // Everything between the first `(` and the final `)` is the argument,
    // verbatim — an inner `)` (as in `shell(echo $(date))`) is preserved
    // because we slice to the last `)`.
    let arg = trimmed[open + 1..trimmed.len() - 1].trim();

    match name {
        "shell" => Ok(RuntimePredicate::Shell(arg.to_string())),
        "path_exists" => Ok(RuntimePredicate::PathExists(arg.to_string())),
        "env" => parse_env(arg),
        "not" => Ok(RuntimePredicate::Not(Box::new(parse(arg)?))),
        "any" => {
            let preds = parse_comma_separated(arg)?;
            if preds.is_empty() {
                bail!("`any(...)` requires at least one predicate");
            }
            Ok(RuntimePredicate::Any(preds))
        }
        other => bail!(
            "unknown predicate `{other}` \
             (expected `shell`, `path_exists`, `env`, `not`, or `any`)"
        ),
    }
}

/// Parse the argument of `env(...)`: either a bare `<name>` (presence check) or
/// `<name>=<value>` (equality check). The value is taken verbatim after the
/// first `=`.
fn parse_env(arg: &str) -> Result<RuntimePredicate> {
    match arg.split_once('=') {
        Some((name, value)) => {
            let name = name.trim();
            if name.is_empty() {
                bail!("`env(...)` variable name must not be empty");
            }
            Ok(RuntimePredicate::Env(
                name.to_string(),
                Some(value.to_string()),
            ))
        }
        None => {
            if arg.is_empty() {
                bail!("`env(...)` requires a variable name");
            }
            Ok(RuntimePredicate::Env(arg.to_string(), None))
        }
    }
}

/// Parse a comma-separated list of predicate expressions from SKILL.md
/// frontmatter. Commas inside parentheses are not treated as separators, so
/// `shell(test -a, -b)` stays a single predicate.
pub(crate) fn parse_comma_separated(input: &str) -> Result<Vec<RuntimePredicate>> {
    split_top_level(input)
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(parse)
        .collect()
}

/// Split on top-level commas, ignoring commas nested inside `(...)`. Every
/// predicate is a `name(...)` call, so the separators are always at paren
/// depth 0 and a comma inside an argument stays attached to it.
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

fn run_shell(command: &str) -> bool {
    // Per-evaluation results are at `trace` — call sites emit the user-visible
    // "<level> predicates failed, skipping" message at `debug`.
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
            tracing::trace!(
                command = %command,
                error = %e,
                "shell predicate failed to spawn",
            );
            false
        }
    }
}

fn path_exists(arg: &str) -> bool {
    // A literal path (absolute or cwd-relative) takes precedence; this also
    // covers bare names like `.git` that name a cwd entry.
    if Path::new(arg).exists() {
        return true;
    }
    // Anything with a separator is a path and only a path — no `$PATH` lookup.
    if arg.contains('/') || arg.contains(std::path::MAIN_SEPARATOR) {
        return false;
    }
    // Bare name: fall back to a `$PATH` search, like `command -v`.
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(arg).exists()))
        .unwrap_or(false)
}

impl serde::Serialize for RuntimePredicate {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for RuntimePredicate {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        parse(&s).map_err(serde::de::Error::custom)
    }
}

impl std::fmt::Display for RuntimePredicate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimePredicate::Shell(cmd) => write!(f, "shell({cmd})"),
            RuntimePredicate::PathExists(arg) => write!(f, "path_exists({arg})"),
            RuntimePredicate::Env(name, None) => write!(f, "env({name})"),
            RuntimePredicate::Env(name, Some(value)) => write!(f, "env({name}={value})"),
            RuntimePredicate::Not(inner) => write!(f, "not({inner})"),
            RuntimePredicate::Any(preds) => {
                let joined = preds
                    .iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "any({joined})")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(cmd: &str) -> RuntimePredicate {
        RuntimePredicate::Shell(cmd.into())
    }

    // --- parse ---

    #[test]
    fn parse_shell() {
        assert_eq!(
            parse("shell(command -v rg)").unwrap(),
            shell("command -v rg")
        );
    }

    #[test]
    fn parse_shell_preserves_inner_parens() {
        assert_eq!(parse("shell(echo $(date))").unwrap(), shell("echo $(date)"));
    }

    #[test]
    fn parse_arg_is_verbatim_quotes_not_stripped() {
        // Quotes are part of the argument now; they are not special syntax.
        assert_eq!(parse(r#"shell("x")"#).unwrap(), shell(r#""x""#));
    }

    #[test]
    fn parse_path_exists_absolute() {
        assert_eq!(
            parse("path_exists(/usr/bin/jq)").unwrap(),
            RuntimePredicate::PathExists("/usr/bin/jq".into())
        );
    }

    #[test]
    fn parse_path_exists_bare() {
        assert_eq!(
            parse("path_exists(rg)").unwrap(),
            RuntimePredicate::PathExists("rg".into())
        );
    }

    #[test]
    fn parse_env_presence() {
        assert_eq!(
            parse("env(CI)").unwrap(),
            RuntimePredicate::Env("CI".into(), None)
        );
    }

    #[test]
    fn parse_env_equality() {
        assert_eq!(
            parse("env(MODE=debug)").unwrap(),
            RuntimePredicate::Env("MODE".into(), Some("debug".into()))
        );
    }

    #[test]
    fn parse_env_value_keeps_inner_equals() {
        // Only the first `=` separates name from value.
        assert_eq!(
            parse("env(KEY=a=b)").unwrap(),
            RuntimePredicate::Env("KEY".into(), Some("a=b".into()))
        );
    }

    #[test]
    fn parse_env_empty_name_errors() {
        assert!(parse("env()").is_err());
        assert!(parse("env(=x)").is_err());
    }

    #[test]
    fn parse_not() {
        assert_eq!(
            parse("not(path_exists(rg))").unwrap(),
            RuntimePredicate::Not(Box::new(RuntimePredicate::PathExists("rg".into())))
        );
    }

    #[test]
    fn parse_any() {
        assert_eq!(
            parse("any(path_exists(fd), path_exists(fdfind))").unwrap(),
            RuntimePredicate::Any(vec![
                RuntimePredicate::PathExists("fd".into()),
                RuntimePredicate::PathExists("fdfind".into()),
            ])
        );
    }

    #[test]
    fn parse_nested_combinators() {
        assert_eq!(
            parse("not(any(env(CI), shell(test -f a, b)))").unwrap(),
            RuntimePredicate::Not(Box::new(RuntimePredicate::Any(vec![
                RuntimePredicate::Env("CI".into(), None),
                // The comma inside shell(...) is part of its verbatim argument.
                shell("test -f a, b"),
            ])))
        );
    }

    #[test]
    fn parse_any_empty_errors() {
        assert!(parse("any()").is_err());
    }

    #[test]
    fn parse_unknown_function_errors() {
        assert!(parse("contains(foo)").is_err());
    }

    #[test]
    fn parse_not_a_call_errors() {
        assert!(parse("command -v rg").is_err());
    }

    #[test]
    fn parse_missing_close_paren_errors() {
        assert!(parse("shell(x").is_err());
    }

    // --- Display round-trip ---

    #[test]
    fn display_round_trip() {
        for input in [
            "shell(command -v rg)",
            "path_exists(/usr/bin/jq)",
            "path_exists(rg)",
            "shell(echo $(date))",
            "env(CI)",
            "env(MODE=debug)",
            "not(path_exists(rg))",
            "any(path_exists(fd), path_exists(fdfind))",
            "not(any(env(CI), shell(true)))",
        ] {
            let p = parse(input).unwrap();
            assert_eq!(p.to_string(), input, "display drift: {input}");
            assert_eq!(
                parse(&p.to_string()).unwrap(),
                p,
                "round-trip failed: {input}"
            );
        }
    }

    // --- evaluate ---

    #[test]
    fn empty_set_is_true() {
        assert!(RuntimePredicateSet::default().evaluate());
    }

    #[test]
    fn shell_true_and_false() {
        assert!(shell("true").evaluate());
        assert!(!shell("false").evaluate());
        assert!(!shell("exit 3").evaluate());
    }

    #[test]
    fn shell_missing_binary_is_false() {
        assert!(!shell("definitely-not-a-real-binary-xyz").evaluate());
    }

    #[test]
    fn set_all_must_pass() {
        let set = RuntimePredicateSet {
            predicates: vec![shell("true"), shell("false"), shell("true")],
        };
        assert!(!set.evaluate());
    }

    #[test]
    fn not_inverts() {
        assert!(RuntimePredicate::Not(Box::new(shell("false"))).evaluate());
        assert!(!RuntimePredicate::Not(Box::new(shell("true"))).evaluate());
    }

    #[test]
    fn any_is_or() {
        assert!(RuntimePredicate::Any(vec![shell("false"), shell("true")]).evaluate());
        assert!(!RuntimePredicate::Any(vec![shell("false"), shell("false")]).evaluate());
    }

    #[test]
    fn env_presence_and_equality() {
        // Drive a fixed var via a shell predicate is not possible, so assert on
        // a var we know is present (`PATH`) and one we know is absent.
        assert!(RuntimePredicate::Env("PATH".into(), None).evaluate());
        assert!(!RuntimePredicate::Env("SYMPOSIUM_DEFINITELY_UNSET_XYZ".into(), None).evaluate());
        // Equality against an almost-certainly-wrong value fails.
        assert!(
            !RuntimePredicate::Env("PATH".into(), Some("\u{0}not-a-real-path".into())).evaluate()
        );
    }

    #[test]
    fn path_exists_absolute() {
        assert!(RuntimePredicate::PathExists("/".into()).evaluate());
        assert!(!RuntimePredicate::PathExists("/definitely/not/here/xyz".into()).evaluate());
    }

    #[test]
    fn path_exists_bare_name_on_path() {
        // `sh` is what shell predicates run through, so it must be on PATH.
        assert!(RuntimePredicate::PathExists("sh".into()).evaluate());
        assert!(
            !RuntimePredicate::PathExists("definitely-not-a-real-binary-xyz".into()).evaluate()
        );
    }

    #[test]
    fn path_with_separator_does_not_search_path() {
        // A separator-bearing arg is a filesystem path only; `sh` exists on
        // PATH but `./sh` does not exist in the cwd.
        assert!(!RuntimePredicate::PathExists("./sh".into()).evaluate());
    }

    // --- comma-separated frontmatter ---

    #[test]
    fn comma_separated_parsing() {
        let preds = parse_comma_separated("shell(command -v rg), path_exists(rg)").unwrap();
        assert_eq!(
            preds,
            vec![
                shell("command -v rg"),
                RuntimePredicate::PathExists("rg".into())
            ]
        );
    }

    #[test]
    fn comma_separated_ignores_commas_inside_predicates() {
        let preds = parse_comma_separated("shell(test -a, -b), path_exists(c)").unwrap();
        assert_eq!(
            preds,
            vec![
                shell("test -a, -b"),
                RuntimePredicate::PathExists("c".into())
            ]
        );
    }

    #[test]
    fn comma_separated_empty_input() {
        assert!(parse_comma_separated("").unwrap().is_empty());
    }

    // --- TOML round-trip ---

    #[test]
    fn toml_round_trip() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Container {
            #[serde(default, skip_serializing_if = "RuntimePredicateSet::is_empty")]
            predicates: RuntimePredicateSet,
        }
        let empty: Container = toml::from_str("").unwrap();
        assert!(empty.predicates.is_empty());

        let parsed: Container =
            toml::from_str(r#"predicates = ["shell(command -v rg)", "path_exists(rg)"]"#).unwrap();
        assert_eq!(parsed.predicates.predicates.len(), 2);
        let reserialized = toml::to_string(&parsed).unwrap();
        let reparsed: Container = toml::from_str(&reserialized).unwrap();
        assert_eq!(reparsed.predicates, parsed.predicates);
    }
}
