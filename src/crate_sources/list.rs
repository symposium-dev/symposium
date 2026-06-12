//! List workspace crates with available guidance

use symposium_sdk::workspace::WorkspaceCrate;

/// The workspace crates as `(name, version)` pairs — the form predicate
/// evaluation consumes (see [`crate::predicate::PredicateContext`]).
pub fn crate_pairs(crates: &[WorkspaceCrate]) -> Vec<(String, semver::Version)> {
    crates
        .iter()
        .map(|c| (c.name.clone(), c.version.clone()))
        .collect()
}
