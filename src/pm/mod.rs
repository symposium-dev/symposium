//! Package managers: the in-process seam from the [registry-centric plugin
//! distribution RFD](../../md/rfds/registry-centric-plugins/README.md).
//!
//! A [`PackageId`] names a package as a `(pm, name, version)` tuple, and a
//! [`PackageManager`] resolves ids of its ecosystem to content on disk.
//! Cargo is the only package manager today and fetching the only operation
//! routed through the seam: callers that used to construct a
//! [`RustCrateFetch`](crate::crate_sources::RustCrateFetch) directly go
//! through [`CargoPm`] instead.

use std::path::PathBuf;

use anyhow::Result;
use symposium_sdk::workspace::WorkspaceCrate;

mod cargo;
pub use cargo::CargoPm;

/// The `pm` component of cargo package ids.
pub const CARGO_PM: &str = "cargo";

/// Version placeholder for "no requirement": the package manager resolves it
/// (for cargo: a workspace pin, or the newest published version).
pub const ANY_VERSION: &str = "*";

/// Canonical package coordinates: which package manager, which package,
/// which version.
///
/// `version` may still be a *requirement* (a semver range, or
/// [`ANY_VERSION`]); [`PackageManager::fetch`] canonicalizes it — the id on
/// a [`FetchedPackage`] always names the exact resolved version.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PackageId {
    pub pm: String,
    pub name: String,
    pub version: String,
}

impl PackageId {
    pub fn new(pm: impl Into<String>, name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            pm: pm.into(),
            name: name.into(),
            version: version.into(),
        }
    }
}

impl std::fmt::Display for PackageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:{}", self.pm, self.name, self.version)
    }
}

/// A fetched package: the exact id it resolved to, plus the directory
/// holding its content.
#[derive(Debug, Clone)]
pub struct FetchedPackage {
    pub id: PackageId,
    pub root: PathBuf,
}

/// The package-manager interface.
// Auto trait bounds can't be named on an `async fn` trait method; fine here
// because nothing holds the future across threads, and a `Send` bound would
// be premature with a single in-process implementation.
#[allow(async_fn_in_trait)]
pub trait PackageManager {
    /// Resolve `id` and return its content directory.
    ///
    /// `workspace` supplies the active workspace's dependency resolution:
    /// path-dependency overrides and version pins.
    async fn fetch(&self, id: &PackageId, workspace: &[WorkspaceCrate]) -> Result<FetchedPackage>;
}
