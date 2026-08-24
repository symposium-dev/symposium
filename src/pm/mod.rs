//! Package managers: the in-process seam from the [registry-centric plugin
//! distribution RFD](../../md/rfds/registry-centric-plugins/README.md).
//!
//! A [`PackageId`] names a package as a `(pm, name, version)` tuple, and a
//! [`PackageManager`] resolves ids of its ecosystem to content on disk.
//!
//! A `PackageManager` value is an *instance*, not just an ecosystem. A
//! **transport** ([`CargoPm`]) can `fetch` any id of its ecosystem, because the
//! id carries the source; a **registry instance** ([`PathPm`]) fronts one
//! configured source and enumerates the packages it contains. [`PmRegistry`]
//! holds them as one flat set: an id is dispatched to the instance whose
//! [`PackageManager::name`] matches its [`PackageId::pm`], and plugin loading
//! ([`active_plugins`](PackageManager::active_plugins) /
//! [`load_plugin`](PackageManager::load_plugin) / `search`) iterates every
//! instance.
//!
//! A registry instance's [`PackageManager::name`] is the *configured registry
//! name* (`user-plugins`, `symposium-recommendations`, …), which is also the
//! `pm` component of every id it mints and the name its plugins are attributed
//! to. Registry instances resolve their own ids, so those ids are never routed
//! through the ecosystem transports.
//!
//! In-process for now — when PMs move out of process, [`PmRegistry`] becomes
//! the seam that spawns and talks to them.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use symposium_install::UpdateLevel;

use crate::plugins::ParsedPlugin;

mod cargo;
mod git;
pub mod layout;
mod path;
pub use cargo::{
    CargoPm, LoadedWorkspace, WorkspaceCrate, WorkspaceDeps, file_mtime, workspace_dir_name,
};
pub use git::GitPm;
pub use path::PathPm;

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

/// What [`search`](PackageManager::search) knows about a candidate package
/// before its content is on disk: its identity and an optional description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInfo {
    /// Canonical identity. The version component may still be a requirement
    /// that fetch canonicalizes.
    pub id: PackageId,
    /// Human-oriented description when the PM's registry provides one.
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

/// A fetched package: the exact id it resolved to, plus the directory
/// holding its content.
#[derive(Debug, Clone)]
pub struct FetchedPackage {
    pub id: PackageId,
    pub root: PathBuf,
}

/// The operations every package manager implements (per the registry-centric
/// plugin distribution RFD).
///
/// A PM is self-contained: it holds whatever it needs to resolve its own
/// ecosystem ([`CargoPm`] owns an [`Arc<WorkspaceDeps>`], [`PathPm`] owns its
/// directory), so operations take no ambient context. This mirrors the
/// out-of-process shape — a PM spawned for a workspace answers RPC calls from
/// its own state, with nothing workspace-shaped threaded per call.
///
/// Loading has two forms. [`active_plugins`](Self::active_plugins) is what the
/// PM activates for the workspace's dependency set (a registry lists its
/// entries; the cargo transport surfaces dependency-embedded plugins);
/// [`load_plugin`](Self::load_plugin) resolves a *specific* id named elsewhere
/// (a `[[plugins]]` chained reference, an explicitly enabled crate). Both return
/// fully-resolved [`ParsedPlugin`]s (absolute skill dirs) and are best-effort —
/// failures are logged and dropped, not surfaced, so one bad plugin never
/// aborts a sync or hook.
#[async_trait::async_trait]
pub trait PackageManager {
    /// The PM's registry name — the `pm` component of every id it owns. For
    /// an ecosystem transport this is the ecosystem (`cargo`); for a registry
    /// instance it is the configured registry's name.
    fn name(&self) -> &str;

    /// The plugins this PM activates for the workspace's dependency set. A
    /// registry lists its own entries (deps ignored); the cargo transport
    /// surfaces the plugins its dependencies embed. Whether a dependency-embedded
    /// plugin is *trusted* is the caller's decision — see [`PmInstance::trusted`].
    async fn active_plugins(&self, deps: &[PackageId]) -> Vec<ParsedPlugin>;

    /// The plugin(s) a specific id maps to — zero, one, or many. Used for
    /// `[[plugins]]` chained references and explicitly enabled crates.
    async fn load_plugin(&self, id: &PackageId) -> Vec<ParsedPlugin>;

    /// The package ids the current workspace depends on. Empty for PMs with no
    /// workspace notion.
    async fn list_deps(&self) -> Result<Vec<PackageId>>;

    /// Find packages matching a partial query (backs `use` / `search`). PMs
    /// without a searchable registry return an empty list.
    async fn search(&self, query: &str) -> Result<Vec<PluginInfo>>;

    /// Acquire a package's source content, canonicalizing the id's version.
    /// `update` controls how aggressively an already-cached package is refreshed.
    async fn fetch(&self, id: &PackageId, update: UpdateLevel) -> Result<FetchedPackage>;

    /// Refresh this PM's backing source — for a registry, pull the latest
    /// content onto disk. `force` ignores the source's auto-update opt-out
    /// (used by an explicit `plugin sync <name>`). Returns whether a remote
    /// source was actually refreshed, so callers can report what synced. The
    /// default is a no-op: a PM whose content is already local (the cargo
    /// transport, a path registry) has nothing to pull.
    async fn refresh(&self, _update: UpdateLevel, _force: bool) -> Result<bool> {
        Ok(false)
    }

    /// How a registry instance's content is sourced, for `plugin list`
    /// display. `None` for the cargo transport, which is not a configured
    /// registry.
    fn registry_source(&self) -> Option<RegistrySource> {
        None
    }

    /// Whether this instance's source can be read right now.
    ///
    /// A source that cannot be listed yields no plugins, exactly like one that
    /// is genuinely empty. Callers that *remove* things need the difference:
    /// an unreadable registry must not read as "these plugins no longer apply".
    /// An absent source is readable — that is an empty registry, not a failure.
    async fn source_readable(&self) -> bool {
        true
    }
}

/// Where a registry instance's content comes from — the git-vs-path
/// distinction, made visible on the PM itself rather than inferred elsewhere.
pub enum RegistrySource {
    /// A git repository, fetched into the plugin-source cache.
    Git { url: String },
    /// A local directory.
    Path { dir: PathBuf },
}

/// A package-manager instance: its attribution name (the config registry name,
/// or `cargo` for the transport — the `pm` component of every id it owns), a
/// trust marker, and the PM itself.
pub struct PmInstance {
    pub name: String,
    /// Whether this instance's [`active_plugins`](PackageManager::active_plugins)
    /// are trust roots. Registries and the workspace are trusted; the cargo
    /// transport is not, since its `active_plugins` are the plugins *embedded in
    /// dependencies*, which run only with the user's consent.
    pub trusted: bool,
    pub pm: Box<dyn PackageManager + Send + Sync>,
}

/// The active set of package-manager instances — one flat collection, the cargo
/// transport alongside one instance per configured registry. Ids are dispatched
/// by their `pm` component ([`PackageId::pm`]) to the instance that owns them.
pub struct PmRegistry {
    instances: Vec<PmInstance>,
}

impl PmRegistry {
    pub fn new(instances: Vec<PmInstance>) -> Self {
        Self { instances }
    }

    /// Every instance, in order (cargo transport first, then registries).
    pub fn instances(&self) -> impl Iterator<Item = &PmInstance> {
        self.instances.iter()
    }

    /// The instance owning the named ecosystem/registry.
    fn owner(&self, pm: &str, id: &PackageId) -> Result<&(dyn PackageManager + Send + Sync)> {
        self.instances
            .iter()
            .find(|inst| inst.pm.name() == pm)
            .map(|inst| inst.pm.as_ref())
            .ok_or_else(|| anyhow::anyhow!("unknown package manager `{pm}` in package id `{id}`"))
    }

    /// Fetch a package via the instance named in its id.
    pub async fn fetch(&self, id: &PackageId, update: UpdateLevel) -> Result<FetchedPackage> {
        self.owner(&id.pm, id)?.fetch(id, update).await
    }

    /// Union of `list_deps` across the instances — the workspace's full
    /// dependency set for discovery and `depends-on` predicate evaluation.
    pub async fn list_deps(&self) -> Result<Vec<PackageId>> {
        let mut deps = Vec::new();
        for inst in &self.instances {
            deps.extend(inst.pm.list_deps().await?);
        }
        Ok(deps)
    }

    /// Load the plugin(s) an id maps to, asking every instance. Any instance may
    /// contribute a plugin relevant to the id, so this can return several.
    pub async fn load_plugin(&self, id: &PackageId) -> Vec<ParsedPlugin> {
        let mut out = Vec::new();
        for inst in &self.instances {
            out.extend(inst.pm.load_plugin(id).await);
        }
        out
    }

    /// Search every instance for packages matching `query`, tagged with the
    /// instance's display name. A failing instance is skipped with a debug log
    /// rather than failing the union.
    pub async fn search(&self, query: &str) -> Vec<(String, PluginInfo)> {
        let mut out = Vec::new();
        for inst in self.instances() {
            match inst.pm.search(query).await {
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
    deps: &Arc<WorkspaceDeps>,
) -> Vec<PackageId> {
    match sym.package_managers(deps).list_deps().await {
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
        let _ = tmp;
        let id = PackageId::any_version("npm", "leftpad");
        let err = PmRegistry::new(vec![])
            .fetch(&id, UpdateLevel::None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("unknown package manager `npm`"));
    }
}
