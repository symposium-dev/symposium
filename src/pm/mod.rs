//! Package managers: the in-process seam from the [registry-centric plugin
//! distribution RFD](../../md/rfds/registry-centric-plugins/README.md).
//!
//! A [`PackageId`] names a package as a `(pm, name, version)` tuple, and a
//! [`PackageManager`] resolves ids of its ecosystem to content on disk.
//!
//! A `PackageManager` value is an *instance*, not just an ecosystem. A
//! **transport** ([`CargoPm`]) can `fetch` any id of its ecosystem, because the
//! id carries the source; a **registry instance** fronts one configured source
//! and enumerates the packages it contains. [`PmRegistry`] holds both tiers:
//! transports are dispatched by [`PackageId::pm`] for `fetch` / `cached_root` /
//! `list_deps`, while discovery (`list_plugins` / `search`) iterates everything.
//!
//! Cargo is the only package manager today; `path` and `recommendations`
//! registries follow. In-process for now — when PMs move out of process,
//! [`PmRegistry`] becomes the seam that spawns and talks to them.

use std::path::PathBuf;

use anyhow::Result;
use symposium_install::{InstallContext, UpdateLevel};
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

/// What a PM knows about a plugin *before* its content is on disk: the
/// canonical identity plus whatever metadata its registry offers. This is what
/// [`list_plugins`](PackageManager::list_plugins) and
/// [`search`](PackageManager::search) return — pass [`Self::id`] to
/// [`fetch`](PackageManager::fetch) to materialize the content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInfo {
    /// Canonical identity. The version component may still be a requirement
    /// that fetch canonicalizes.
    pub id: PackageId,
    /// Human-oriented description when the PM's registry provides one.
    pub description: Option<String>,
    /// For registry-instance offers: the package's directory within the
    /// registry source. `None` for offers that aren't positional registry
    /// entries (e.g. dependency-embedded crates).
    pub subpath: Option<PathBuf>,
    /// The dependency this entry recommends a plugin for. Name-only and
    /// ecosystem-agnostic on purpose: the gate it implies
    /// (`depends-on(<name>)`) matches a dependency from any PM.
    pub recommends: Option<String>,
}

impl PluginInfo {
    /// An info with no registry metadata — just the identity.
    pub fn from_id(id: PackageId) -> Self {
        Self {
            id,
            description: None,
            subpath: None,
            recommends: None,
        }
    }
}

/// A fetched package: the exact id it resolved to, plus the directory
/// holding its content.
#[derive(Debug, Clone)]
pub struct FetchedPackage {
    pub id: PackageId,
    pub root: PathBuf,
}

/// Everything a PM operation may need from the surrounding invocation.
pub struct PmContext<'a> {
    /// Cache root and cargo override for acquisition.
    pub install: InstallContext,
    /// The resolved workspace dependency list. Cargo uses it for path-dep
    /// overrides, version pinning, and `list_deps`.
    pub workspace_crates: &'a [WorkspaceCrate],
}

impl<'a> PmContext<'a> {
    /// The context for a given invocation and workspace.
    pub fn new(sym: &crate::config::Symposium, workspace_crates: &'a [WorkspaceCrate]) -> Self {
        Self {
            install: sym.install_context(),
            workspace_crates,
        }
    }
}

/// The operations every package manager implements (per the registry-centric
/// plugin distribution RFD).
#[async_trait::async_trait]
pub trait PackageManager {
    /// The PM's registry name — the `pm` component of every id it owns.
    fn name(&self) -> &'static str;

    /// The plugin-bearing packages this PM offers for the given workspace
    /// dependency set. This is the input-less form of the RFD's `resolve`: a
    /// registry instance lists its own source; ecosystem PMs use `deps` to
    /// surface dependency-matched plugins.
    ///
    /// Must not fetch or touch the network — read-only callers (help
    /// rendering, hook dispatch) rely on this serving from cache.
    async fn list_plugins(&self, deps: &[PackageId], cx: &PmContext<'_>)
    -> Result<Vec<PluginInfo>>;

    /// Find packages matching a partial query. PMs without a searchable
    /// registry return an empty list.
    async fn search(&self, query: &str, cx: &PmContext<'_>) -> Result<Vec<PluginInfo>>;

    /// Acquire the package's content into a local directory, canonicalizing
    /// the id's version component. `update` controls how aggressively an
    /// already-cached package is refreshed.
    async fn fetch(
        &self,
        id: &PackageId,
        cx: &PmContext<'_>,
        update: UpdateLevel,
    ) -> Result<FetchedPackage>;

    /// The package ids the current workspace depends on. PMs with no
    /// workspace notion return an empty list.
    async fn list_deps(&self, cx: &PmContext<'_>) -> Result<Vec<PackageId>>;

    /// Where a previously fetched package's content lives on disk, computed
    /// without fetching or touching the network. `None` when the PM can't
    /// answer from the id alone.
    fn cached_root(&self, id: &PackageId, cx: &PmContext<'_>) -> Option<PathBuf>;
}

/// A package-manager instance paired with its attribution name: the config
/// source name for registry instances, the pm name for the built-in
/// transports. The name labels what the instance's plugins are loaded *as*.
pub struct PmInstance {
    pub name: String,
    pub pm: Box<dyn PackageManager + Send + Sync>,
}

/// The active set of package-manager instances, in two tiers: the fixed
/// per-ecosystem transports and the config-derived registry instances.
pub struct PmRegistry {
    /// One transport per ecosystem, always present.
    transports: Vec<PmInstance>,
    /// One instance per configured registry.
    registries: Vec<PmInstance>,
}

impl PmRegistry {
    /// The fixed transports plus the given registry instances.
    /// `new(Vec::new())` is a transport-only set for callers that just need
    /// `fetch` or `list_deps`.
    pub fn new(registries: Vec<PmInstance>) -> Self {
        Self {
            transports: vec![PmInstance {
                name: CARGO_PM.to_string(),
                pm: Box::new(CargoPm),
            }],
            registries,
        }
    }

    /// All active instances: transports first, then registries in config order.
    pub fn instances(&self) -> impl Iterator<Item = &PmInstance> {
        self.transports.iter().chain(self.registries.iter())
    }

    /// The transport owning the named ecosystem.
    fn transport_for(
        &self,
        pm: &str,
        id: &PackageId,
    ) -> Result<&(dyn PackageManager + Send + Sync)> {
        self.transports
            .iter()
            .find(|inst| inst.pm.name() == pm)
            .map(|inst| inst.pm.as_ref())
            .ok_or_else(|| anyhow::anyhow!("unknown package manager `{pm}` in package id `{id}`"))
    }

    /// Fetch a package via the transport named in its id.
    pub async fn fetch(
        &self,
        id: &PackageId,
        cx: &PmContext<'_>,
        update: UpdateLevel,
    ) -> Result<FetchedPackage> {
        self.transport_for(&id.pm, id)?.fetch(id, cx, update).await
    }

    /// Union of `list_deps` across the ecosystems — the workspace's full
    /// dependency set for discovery and `depends-on` predicate evaluation.
    pub async fn list_deps(&self, cx: &PmContext<'_>) -> Result<Vec<PackageId>> {
        let mut deps = Vec::new();
        for inst in &self.transports {
            deps.extend(inst.pm.list_deps(cx).await?);
        }
        Ok(deps)
    }

    /// Where a fetched package's content lives, via the transport named in its
    /// id. No fetching, no network.
    pub fn cached_root(&self, id: &PackageId, cx: &PmContext<'_>) -> Result<Option<PathBuf>> {
        Ok(self.transport_for(&id.pm, id)?.cached_root(id, cx))
    }

    /// Search every instance for packages matching `query`, tagged with the
    /// instance's display name. A failing instance is skipped with a debug log
    /// rather than failing the union.
    pub async fn search(&self, query: &str, cx: &PmContext<'_>) -> Vec<(String, PluginInfo)> {
        let mut out = Vec::new();
        for inst in self.instances() {
            match inst.pm.search(query, cx).await {
                Ok(infos) => out.extend(infos.into_iter().map(|i| (inst.name.clone(), i))),
                Err(e) => {
                    tracing::debug!(instance = %inst.name, error = %e, "search failed, skipping");
                }
            }
        }
        out
    }
}

/// The workspace's dependency set as package ids — every PM's `list_deps`
/// unioned. This is what `depends-on` predicates evaluate against
/// ([`crate::predicate::PredicateContext`]). Failures are logged and yield an
/// empty list so predicate evaluation degrades to "no deps" rather than
/// aborting the caller.
pub async fn workspace_dep_ids(
    sym: &crate::config::Symposium,
    workspace_crates: &[WorkspaceCrate],
) -> Vec<PackageId> {
    let cx = PmContext::new(sym, workspace_crates);
    match sym.package_managers().list_deps(&cx).await {
        Ok(deps) => deps,
        Err(e) => {
            tracing::warn!(error = %e, "failed to list workspace dependencies");
            Vec::new()
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

    #[tokio::test]
    async fn registry_rejects_unknown_pm() {
        let tmp = tempfile::tempdir().unwrap();
        let cx = PmContext {
            install: InstallContext::new(tmp.path().to_path_buf()),
            workspace_crates: &[],
        };
        let id = PackageId::any_version("npm", "leftpad");
        let err = PmRegistry::new(vec![])
            .fetch(&id, &cx, UpdateLevel::None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("unknown package manager `npm`"));
    }
}
