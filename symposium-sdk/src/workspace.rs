//! Workspace dependency types.
//!
//! These types represent the crates in a workspace's dependency graph. They
//! mirror what symposium resolves internally via `cargo metadata` and may be
//! passed to plugin binaries in the future (e.g. on stdin).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A crate in the workspace's direct dependency graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceCrate {
    /// The crate name as published (e.g. `"serde"`, `"tokio"`).
    pub name: String,
    /// The resolved version.
    pub version: semver::Version,
    /// Local source path for path dependencies.
    /// `None` for registry crates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}
