//! Shell-command predicates that gate plugin / skill / hook activation.
//!
//! A shell predicate is a string evaluated via `sh -c`. Exit code 0 means the
//! predicate holds; any other exit (including spawn failure) means it fails.
//! Predicates compose with **AND** semantics — within a level, all predicates
//! in the list must hold for that level to match.
//!
//! Predicates are evaluated at the same points where [`crate::predicate`]
//! crate predicates are evaluated:
//!
//! - Plugin-level: at sync time (gating skills/MCP) and at hook dispatch.
//! - Skill-group / skill-level: at sync time.
//! - Hook-level: at hook dispatch time.

use std::process::Command;

use serde::{Deserialize, Serialize};

/// A set of shell-command predicates that must all exit 0 for the
/// enclosing item to be active.
///
/// Serialized as a plain `Vec<String>` of shell commands. The empty list
/// is vacuously true and is omitted from serialized output.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ShellPredicateSet {
    pub commands: Vec<String>,
}

impl ShellPredicateSet {
    /// True if every command exits 0 (or the list is empty).
    pub fn evaluate(&self) -> bool {
        self.commands.iter().all(|cmd| run_one(cmd))
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

fn run_one(command: &str) -> bool {
    // Per-evaluation results are at `trace` — call sites emit the user-visible
    // "<level> shell_predicates failed, skipping" message at `debug`.
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

/// Helper: parse a comma-separated string of shell predicates from
/// SKILL.md frontmatter. Commas inside individual predicates aren't
/// supported here — use the TOML form for anything beyond simple
/// `command -v foo` / `test -f bar` style checks.
pub(crate) fn parse_comma_separated(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_set_is_true() {
        let set = ShellPredicateSet::default();
        assert!(set.evaluate());
    }

    #[test]
    fn single_true_predicate() {
        let set = ShellPredicateSet {
            commands: vec!["true".into()],
        };
        assert!(set.evaluate());
    }

    #[test]
    fn single_false_predicate() {
        let set = ShellPredicateSet {
            commands: vec!["false".into()],
        };
        assert!(!set.evaluate());
    }

    #[test]
    fn all_must_pass() {
        let set = ShellPredicateSet {
            commands: vec!["true".into(), "false".into(), "true".into()],
        };
        assert!(!set.evaluate());
    }

    #[test]
    fn non_zero_non_one_is_failure() {
        let set = ShellPredicateSet {
            commands: vec!["exit 3".into()],
        };
        assert!(!set.evaluate());
    }

    #[test]
    fn spawn_failure_treated_as_false() {
        // Use a command that exits non-zero — a missing binary inside `sh -c`
        // exits 127, which is still "failed".
        let set = ShellPredicateSet {
            commands: vec!["definitely-not-a-real-binary-xyz".into()],
        };
        assert!(!set.evaluate());
    }

    #[test]
    fn comma_separated_parsing() {
        let preds = parse_comma_separated("command -v foo, test -f bar");
        assert_eq!(preds, vec!["command -v foo", "test -f bar"]);
    }

    #[test]
    fn comma_separated_empty_input() {
        let preds = parse_comma_separated("");
        assert!(preds.is_empty());
    }

    #[test]
    fn comma_separated_trims_whitespace() {
        let preds = parse_comma_separated("  true  ,  false  ");
        assert_eq!(preds, vec!["true", "false"]);
    }

    // --- TOML round-trip ---

    #[test]
    fn toml_round_trip_empty() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Container {
            #[serde(default, skip_serializing_if = "ShellPredicateSet::is_empty")]
            shell_predicates: ShellPredicateSet,
        }
        let parsed: Container = toml::from_str("").unwrap();
        assert!(parsed.shell_predicates.is_empty());
    }

    #[test]
    fn toml_round_trip_populated() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Container {
            #[serde(default, skip_serializing_if = "ShellPredicateSet::is_empty")]
            shell_predicates: ShellPredicateSet,
        }
        let parsed: Container =
            toml::from_str(r#"shell_predicates = ["command -v rg", "test -f Cargo.toml"]"#)
                .unwrap();
        assert_eq!(parsed.shell_predicates.commands.len(), 2);
    }
}
