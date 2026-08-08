//! Package-manager identity types.
//!
//! A [`PackageId`] names a package as a `(pm, name, version)` tuple — the
//! canonical identity from the [registry-centric plugin distribution
//! RFD](https://github.com/symposium-dev/symposium/blob/main/md/rfds/registry-centric-plugins/README.md).
//! These live in the SDK because they cross the PM boundary: both Symposium
//! and a package-manager binary speak in them.

use serde::{Deserialize, Serialize};

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
