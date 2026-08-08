//! Package-manager identity types.
//!
//! A [`PackageId`] names a package as a `(pm, name, version)` tuple — the
//! canonical identity from the [registry-centric plugin distribution
//! RFD](https://github.com/symposium-dev/symposium/blob/main/md/rfds/registry-centric-plugins/README.md).
//! These live in the SDK because they cross the PM boundary: both Symposium
//! and a package-manager binary speak in them.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::manifest::RawPluginManifest;

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
/// The manifest is the point. A PM may have parsed it from a `SYMPOSIUM.toml`,
/// translated it from its ecosystem's own manifest, or synthesized it for a
/// package that describes itself not at all. Symposium cannot tell, and does
/// not need to. An offer says which plugins exist and what they contain;
/// whether any of them runs is decided separately, from the user's
/// configuration and from the source the offer came from.
#[derive(Debug, Serialize, Deserialize)]
pub struct PluginOffer {
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

impl PluginOffer {
    /// An offer whose paths are displayed relative to its own root.
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
