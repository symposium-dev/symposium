//! Package managers: identity, resolved-source claims, and the wire protocol.
//!
//! A [`PackageId`] names a package as a `(pm, name, version)` tuple and a
//! [`PackageResolution`] couples a reported coordinate with the source class
//! and opaque key its package manager resolved. An [`UnvalidatedPlugin`] is
//! what a PM answers with: the canonical shapes from the
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

/// Maximum UTF-8 byte length of an opaque [`PackageKey`].
///
/// Keys may contain package-manager-private paths or URLs, so callers must
/// treat them as sensitive even though the wire bounds their size.
pub const MAX_PACKAGE_KEY_BYTES: usize = 16 * 1024;

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

/// A package manager's source classification for one resolved package.
///
/// Claims deliberately carry no URL, path, registry name, or authority. Core
/// decides whether a claim may produce a public name after admitting the
/// package through its supplying instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageSourceClaim {
    /// Cargo's logical crates.io source, including an equivalent configured
    /// source replacement.
    CratesIo,
    /// A non-crates.io package registry.
    Registry,
    /// A Git source.
    Git,
    /// A local path source outside the active workspace membership set.
    Path,
    /// Membership in the active workspace as determined by the package
    /// manager's project model.
    Workspace,
    /// A source representation the package manager cannot classify.
    Unknown,
}

/// Why an opaque package key could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageKeyError {
    /// The key was empty.
    Empty,
    /// The key exceeded [`MAX_PACKAGE_KEY_BYTES`].
    TooLong,
}

impl std::fmt::Display for PackageKeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => f.write_str("package key is empty"),
            Self::TooLong => f.write_str("package key exceeds the wire limit"),
        }
    }
}

impl std::error::Error for PackageKeyError {}

/// A package-manager-owned identity for one resolved package.
///
/// The key is meaningful only to the package manager that minted it and only
/// during that supplier's current resolution session. It may contain private
/// paths or URLs, so it deliberately implements neither [`Debug`] nor
/// [`std::fmt::Display`]. Package-manager implementations must likewise keep
/// it out of errors and diagnostic output, including stderr.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct PackageKey(String);

impl PackageKey {
    /// Construct a non-empty key within the wire-size ceiling.
    pub fn new(value: impl Into<String>) -> std::result::Result<Self, PackageKeyError> {
        let value = value.into();
        if value.is_empty() {
            return Err(PackageKeyError::Empty);
        }
        if value.len() > MAX_PACKAGE_KEY_BYTES {
            return Err(PackageKeyError::TooLong);
        }
        Ok(Self(value))
    }

    /// Borrow the supplier-private representation.
    ///
    /// Callers must not place this value in diagnostics or durable state.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for PackageKey {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// A package manager's complete resolution fact for one package.
///
/// Core lowers this supplier-provided value into a private, session-scoped
/// handle before source-sensitive consumers may use it. This wire type carries
/// no public-naming authority.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageResolution {
    id: PackageId,
    source: PackageSourceClaim,
    key: PackageKey,
}

impl PackageResolution {
    /// Couple a reported coordinate with its classification and opaque key.
    pub fn new(id: PackageId, source: PackageSourceClaim, key: PackageKey) -> Self {
        Self { id, source, key }
    }

    /// The coordinate reported by the package manager.
    pub fn id(&self) -> &PackageId {
        &self.id
    }

    /// The package manager's payload-free source classification.
    pub fn source(&self) -> PackageSourceClaim {
        self.source
    }

    /// The supplier-private key.
    pub fn key(&self) -> &PackageKey {
        &self.key
    }

    /// Consume the resolution into its wire parts.
    pub fn into_parts(self) -> (PackageId, PackageSourceClaim, PackageKey) {
        (self.id, self.source, self.key)
    }
}

/// What a package-manager plugin load resolves or inspects.
///
/// A coordinate starts a new resolution. A key asks the same supplier to
/// inspect one package it resolved earlier in the current session. Core never
/// falls back from a stale key to its package's coordinates.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PackageLoadTarget {
    /// Resolve the supplied coordinate.
    Coordinate {
        /// A package coordinate whose version may still be a requirement.
        id: PackageId,
    },
    /// Inspect a package already resolved by this supplier.
    Resolved {
        /// The supplier-private session key.
        key: PackageKey,
    },
}

impl PackageLoadTarget {
    /// Construct a coordinate load.
    pub fn coordinate(id: PackageId) -> Self {
        Self::Coordinate { id }
    }

    /// Construct an exact load from a supplier-session key.
    pub fn resolved(key: PackageKey) -> Self {
        Self::Resolved { key }
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
#[derive(Debug, Serialize, Deserialize)]
pub struct UnvalidatedPlugin {
    /// The exact resolved identity: never a version requirement.
    pub id: PackageId,
    /// Directory holding the package's content: what a relative path in the
    /// manifest names a file in. Relative `source.path` skill groups resolve
    /// against it, and so do the `executable` and `script` paths of
    /// installations with no `source`, which is where a plugin's own hook
    /// scripts run from. The PM owns it and guarantees it stays valid for the
    /// connection's lifetime; Symposium only reads.
    pub root: PathBuf,
    /// The plugin definition, unvalidated.
    pub manifest: RawPluginManifest,
    /// Directory that resolved skill paths are *displayed* relative to, when
    /// that differs from `root`: a registry entry shows as
    /// `path:<entry>/skills` rather than `path:skills`. Purely cosmetic; the
    /// PM knows its own layout, so it is the one that can say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_root: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub agent_plugin: bool,
}

impl UnvalidatedPlugin {
    /// A plugin whose paths are displayed relative to its own root.
    pub fn new(id: PackageId, root: impl Into<PathBuf>, manifest: RawPluginManifest) -> Self {
        Self {
            id,
            root: root.into(),
            manifest,
            label_root: None,
            agent_plugin: false,
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
// `async_trait` marks its boxed future as `must_use`, while several default
// methods return `Result`, which is independently `must_use`.
#[allow(clippy::double_must_use)]
#[async_trait::async_trait]
pub trait PackageManager: Send + Sync {
    /// The name this PM owns: the `pm` component of every id it mints. For an
    /// ecosystem this is the ecosystem (`cargo`); for a registry, the
    /// configured registry's name.
    fn name(&self) -> &str;

    /// `name` in the form in which two of this PM's names for the same package
    /// compare equal. Exact unless the ecosystem says otherwise: cargo treats
    /// `widget-lib` and `widget_lib` as one crate.
    ///
    /// PMs should return canonical names in their ids. Lookup compares exact
    /// ids first, then normalizes user input. A PM returning a consistent
    /// display spelling can still be matched through batched normalization.
    async fn canonical_name(&self, name: &str) -> Result<String> {
        Ok(name.to_string())
    }

    /// Normalize a batch in input order. Remote clients use the existing
    /// `canonical_names` wire method so comparison does not need one round
    /// trip per dependency. An implementation may override this for bulk work.
    async fn canonical_names(&self, names: &[String]) -> Result<Vec<String>> {
        let mut canonical = Vec::with_capacity(names.len());
        for name in names {
            canonical.push(self.canonical_name(name).await?);
        }
        Ok(canonical)
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

    #[test]
    fn package_source_claims_use_stable_wire_labels() {
        let cases = [
            (PackageSourceClaim::CratesIo, "crates_io"),
            (PackageSourceClaim::Registry, "registry"),
            (PackageSourceClaim::Git, "git"),
            (PackageSourceClaim::Path, "path"),
            (PackageSourceClaim::Workspace, "workspace"),
            (PackageSourceClaim::Unknown, "unknown"),
        ];

        for (claim, label) in cases {
            assert_eq!(
                serde_json::to_string(&claim).unwrap(),
                format!("\"{label}\"")
            );
        }
    }

    #[test]
    fn package_key_is_non_empty_and_bounded() {
        assert!(matches!(PackageKey::new(""), Err(PackageKeyError::Empty)));
        assert!(PackageKey::new("k".repeat(MAX_PACKAGE_KEY_BYTES)).is_ok());
        assert!(matches!(
            PackageKey::new("k".repeat(MAX_PACKAGE_KEY_BYTES + 1)),
            Err(PackageKeyError::TooLong)
        ));
    }

    #[test]
    fn package_key_limit_counts_multibyte_utf8_bytes() {
        let two_byte = "é";
        assert_eq!(two_byte.len(), 2);

        let at_limit = two_byte.repeat(MAX_PACKAGE_KEY_BYTES / two_byte.len());
        assert_eq!(at_limit.len(), MAX_PACKAGE_KEY_BYTES);
        assert!(PackageKey::new(at_limit.clone()).is_ok());
        assert!(matches!(
            PackageKey::new(format!("{at_limit}{two_byte}")),
            Err(PackageKeyError::TooLong)
        ));
    }

    #[test]
    fn package_key_decode_errors_do_not_echo_the_key() {
        let recognizable = "private-registry.example/secret/";
        let private_key = format!(
            "{recognizable}{}",
            "x".repeat(MAX_PACKAGE_KEY_BYTES + 1 - recognizable.len())
        );
        let encoded = serde_json::to_string(&private_key).unwrap();
        let error = match serde_json::from_str::<PackageKey>(&encoded) {
            Ok(_) => panic!("an oversized package key must not decode"),
            Err(error) => error,
        };

        assert!(!error.to_string().contains(recognizable));
        assert!(error.to_string().contains("wire limit"));
    }

    #[test]
    fn package_resolution_round_trips_without_defaulting_source_facts() {
        let resolution = PackageResolution::new(
            PackageId::new("cargo", "serde", "1.0.210"),
            PackageSourceClaim::CratesIo,
            PackageKey::new("registry+crates-io#serde@1.0.210").unwrap(),
        );
        let encoded = serde_json::to_string(&resolution).unwrap();
        let decoded: PackageResolution = serde_json::from_str(&encoded).unwrap();

        assert!(decoded == resolution);
        assert!(
            serde_json::from_str::<PackageResolution>(
                r#"{"id":{"pm":"cargo","name":"serde","version":"1.0.210"},"key":"serde"}"#
            )
            .is_err()
        );
        assert!(serde_json::from_str::<PackageResolution>(
            r#"{"id":{"pm":"cargo","name":"serde","version":"1.0.210"},"source":"future_source","key":"serde"}"#
        )
        .is_err());
        assert!(serde_json::from_str::<PackageResolution>(
            r#"{"id":{"pm":"cargo","name":"serde","version":"1.0.210"},"source":"crates_io","key":"serde","authority":"built_in_cargo"}"#
        )
        .is_err());
    }

    #[test]
    fn plugin_load_targets_keep_coordinate_and_exact_requests_distinct() {
        let coordinate = PackageLoadTarget::coordinate(PackageId::any_version("cargo", "serde"));
        let resolved = PackageLoadTarget::resolved(PackageKey::new("serde-key").unwrap());

        assert_eq!(
            serde_json::to_value(&coordinate).unwrap(),
            serde_json::json!({
                "kind": "coordinate",
                "id": {"pm": "cargo", "name": "serde", "version": "*"}
            })
        );
        assert_eq!(
            serde_json::to_value(&resolved).unwrap(),
            serde_json::json!({"kind": "resolved", "key": "serde-key"})
        );
        assert!(
            serde_json::from_str::<PackageLoadTarget>(
                r#"{"kind":"resolved","key":"serde-key","id":{"pm":"cargo","name":"serde","version":"1"}}"#
            )
            .is_err()
        );
    }
}
