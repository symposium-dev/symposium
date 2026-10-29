//! Package managers: the identity types and the wire protocol.
//!
//! A [`PackageId`] names a package as a `(pm, name, version)` tuple and a
//! [`UnvalidatedPlugin`] is what a PM answers with: the canonical shapes from the
//! [registry-centric plugin distribution
//! RFD](https://github.com/symposium-dev/symposium/blob/main/md/rfds/registry-centric-plugins/README.md).
//! They live in the SDK because they cross the PM boundary: both Symposium and
//! a package-manager binary speak in them.
//!
//! [`PackageManager`] is the operation set every package manager implements,
//! whether Symposium calls it in its own process or over the wire.
//! [`protocol`] is the JSON-RPC wire format, and [`server`] is the harness that
//! turns an implementation into a PM binary.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use anyhow::Result;
/// How aggressively a cached package is refreshed: part of [`PackageManager::fetch`],
/// so re-exported for a PM author.
pub use symposium_install::UpdateLevel;

use crate::manifest::RawPluginManifest;

pub mod protocol;
pub mod server;

/// The `pm` component of cargo package ids.
pub const CARGO_PM: &str = "cargo";

/// Version placeholder for "no requirement": the package manager resolves it
/// (for cargo: a workspace pin, or the newest published version).
pub const ANY_VERSION: &str = "*";

/// Canonical package coordinates: which package manager, which package,
/// which version.
///
/// `version` may still be a *requirement* (a semver range, or
/// [`ANY_VERSION`]); fetching canonicalizes it: the id on a fetched package
/// always names the exact resolved version.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
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

    /// An id with no version requirement — the PM resolves it at fetch.
    pub fn any_version(pm: impl Into<String>, name: impl Into<String>) -> Self {
        Self::new(pm, name, ANY_VERSION)
    }
}

impl std::fmt::Display for PackageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:{}", self.pm, self.name, self.version)
    }
}

/// What a search knows about a candidate package before its content is on
/// disk: its identity and an optional description.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginInfo {
    /// Canonical identity. The version component may still be a requirement
    /// that fetch canonicalizes.
    pub id: PackageId,
    /// Human-oriented description when the PM's registry provides one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl PluginInfo {
    /// An info with just the identity.
    pub fn from_id(id: PackageId) -> Self {
        Self {
            id,
            description: None,
        }
    }
}

/// What a package manager answers with when asked for a plugin: the package's
/// resolved identity, where its content is on disk, and a manifest describing
/// it.
///
/// The manifest is the point. A PM may have parsed it from a `Symposium.toml`,
/// translated it from its ecosystem's own manifest, or synthesized it for a
/// package that describes itself not at all. Symposium cannot tell, and does
/// not need to. A package manager says which plugins exist and what they
/// contain; whether any of them runs is decided separately, from the user's
/// configuration and from the source the plugin came from.
#[derive(Debug, Serialize, Deserialize)]
pub struct UnvalidatedPlugin {
    /// The exact resolved identity: never a version requirement.
    pub id: PackageId,
    /// Directory holding the package's content. Relative `source.path` skill
    /// groups resolve against this. The PM owns it and guarantees it stays
    /// valid for the connection's lifetime; Symposium only reads.
    pub root: PathBuf,
    /// The plugin definition, unvalidated.
    pub manifest: RawPluginManifest,
    /// Directory that resolved skill paths are *displayed* relative to, when
    /// that differs from `root`: a registry entry shows as
    /// `path:<entry>/skills` rather than `path:skills`. Purely cosmetic; the
    /// PM knows its own layout, so it is the one that can say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_root: Option<PathBuf>,
}

impl UnvalidatedPlugin {
    /// A plugin whose paths are displayed relative to its own root.
    pub fn new(id: PackageId, root: impl Into<PathBuf>, manifest: RawPluginManifest) -> Self {
        Self {
            id,
            root: root.into(),
            manifest,
            label_root: None,
        }
    }

    /// Display resolved skill paths relative to `label_root` instead of `root`.
    pub fn with_label_root(mut self, label_root: impl Into<PathBuf>) -> Self {
        self.label_root = Some(label_root.into());
        self
    }

    /// The directory paths are displayed relative to.
    pub fn label_root(&self) -> &std::path::Path {
        self.label_root.as_deref().unwrap_or(&self.root)
    }
}

/// What a package manager offers: its plugins, and each entry it could not
/// read, saying which and why. A listing with errors is not every plugin
/// there is, so sync keeps what a failed entry installed rather than reading
/// its absence as its removal.
#[derive(Debug, Serialize, Deserialize)]
pub struct PluginListing<P = UnvalidatedPlugin> {
    pub plugins: Vec<P>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

impl<P> PluginListing<P> {
    /// Every plugin there is.
    pub fn complete(plugins: Vec<P>) -> Self {
        Self {
            plugins,
            errors: Vec::new(),
        }
    }

    /// Whether every entry was read.
    pub fn is_complete(&self) -> bool {
        self.errors.is_empty()
    }
}

/// The project a package manager finds from the directory it was given at
/// `initialize`: where its ecosystem says the project is rooted, and what it
/// holds.
///
/// Symposium's workspace is the project its package managers report. Where a
/// project is rooted is each ecosystem's own rule (cargo's is the workspace
/// root above any member), so only a package manager can say.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectInfo {
    /// The project's root directory, or `None` when this PM finds no project
    /// there. Every package manager that finds one must agree on it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<PathBuf>,
    /// Directories of the packages that are part of the project rather than
    /// dependencies of it: cargo workspace members, npm workspace packages.
    /// Each may define a workspace plugin. Symposium uses only the ones inside
    /// the root.
    #[serde(default)]
    pub local_packages: Vec<PathBuf>,
    /// Files whose change means this PM's answers may have changed: a
    /// lockfile, say. Symposium records their modification times when it
    /// syncs, and syncs again once one changes, appears, or disappears, so
    /// deciding whether to sync never needs the PM itself.
    #[serde(default)]
    pub inputs: Vec<PathBuf>,
}

/// A fetched package: the exact id it resolved to, and the directory holding
/// its content. The PM owns that directory and keeps it valid for as long as
/// the PM runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchedPackage {
    /// The exact resolved id: never a requirement.
    pub id: PackageId,
    pub root: PathBuf,
}

/// The operations every package manager implements: a registry Symposium
/// reads in its own process, the cargo PM, and a binary speaking the
/// [`protocol`]. One implementation serves both ways of running a PM:
/// Symposium can call it directly, or [`server::run`] can put it on the wire.
///
/// A PM is self-contained: it holds whatever it needs to resolve its own
/// ecosystem, so operations take no ambient context. A PM for a workspace is
/// built for that workspace.
///
/// Every operation can fail, and a failure is not an empty answer: a PM that
/// could not say what it offers must not be read as offering nothing, since
/// sync removes what stops being offered.
#[async_trait::async_trait]
pub trait PackageManager: Send + Sync {
    /// The name this PM owns: the `pm` component of every id it mints. For an
    /// ecosystem this is the ecosystem (`cargo`); for a registry, the
    /// configured registry's name.
    fn name(&self) -> &str;

    /// `name` in the form in which two of this PM's names for the same package
    /// compare equal. Exact unless the ecosystem says otherwise: cargo treats
    /// `widget-lib` and `widget_lib` as one crate.
    fn canonical_name(&self, name: &str) -> String {
        name.to_string()
    }

    /// The plugins this PM activates for the workspace's dependency set. A
    /// registry lists its own entries and ignores `deps`; an ecosystem
    /// surfaces the plugins its dependencies embed. Whether any of them may
    /// run is the caller's decision.
    async fn active_plugins(&self, _deps: &[PackageId]) -> Result<PluginListing> {
        Ok(PluginListing::complete(Vec::new()))
    }

    /// The plugin(s) a specific id maps to: zero, one, or many. Under
    /// [`UpdateLevel::None`] only what is already on disk is served.
    async fn load_plugin(
        &self,
        id: &PackageId,
        update: UpdateLevel,
    ) -> Result<Vec<UnvalidatedPlugin>>;

    /// The workspace's dependencies in this PM's ecosystem.
    async fn list_deps(&self) -> Result<Vec<PackageId>> {
        Ok(Vec::new())
    }

    /// The project this PM finds from the directory it was built for.
    async fn project_info(&self) -> Result<ProjectInfo> {
        Ok(ProjectInfo::default())
    }

    /// Packages matching a partial query.
    async fn search(&self, _query: &str) -> Result<Vec<PluginInfo>> {
        Ok(Vec::new())
    }

    /// Acquire a package's content, canonicalizing the id's version. `update`
    /// controls how aggressively an already-cached package is refreshed.
    async fn fetch(&self, id: &PackageId, _update: UpdateLevel) -> Result<FetchedPackage> {
        anyhow::bail!("{} cannot fetch `{id}`", self.name())
    }

    /// Pull this PM's backing source onto disk: for a git registry, the
    /// repository. Returns whether a remote source was refreshed.
    async fn refresh(&self, _update: UpdateLevel) -> Result<bool> {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_id_display_is_colon_tuple() {
        let id = PackageId::new("cargo", "serde", "1.0.210");
        assert_eq!(id.to_string(), "cargo:serde:1.0.210");
    }

    #[test]
    fn package_id_round_trips_through_json() {
        let id = PackageId::new("cargo", "serde", "1.0.210");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(serde_json::from_str::<PackageId>(&json).unwrap(), id);
    }
}
