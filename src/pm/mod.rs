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
//! An instance may be in-process ([`PathPm`], [`GitPm`], [`CargoPm`]) or a
//! subprocess ([`RemotePm`], which speaks the SDK's JSON-RPC protocol over
//! stdio). Both implement the same trait, so nothing above this module knows
//! which it is talking to.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use symposium_install::UpdateLevel;

use crate::plugins::ParsedPlugin;

mod cargo;
mod git;
pub mod layout;
mod path;
pub mod remote;
pub use cargo::{
    CargoPm, LoadedWorkspace, WorkspaceCrate, WorkspaceDeps, file_mtime, workspace_dir_name,
};
pub use git::GitPm;
pub use path::PathPm;
pub use remote::{RemotePm, RemotePmCommand};

/// The identity types are the SDK's, since they cross the PM boundary: a
/// package-manager binary speaks in them too.
pub use symposium_sdk::pm::{ANY_VERSION, CARGO_PM, PackageId, PluginInfo, PluginOffer};

/// Translate Symposium's update level to the wire spelling. `UpdateLevel` is
/// `#[non_exhaustive]`, and a level this build does not know is safest treated
/// as the cache-only one: it can only under-fetch, never surprise the network.
pub(crate) fn update_of(update: UpdateLevel) -> symposium_sdk::pm::protocol::Update {
    use symposium_sdk::pm::protocol::Update;
    match update {
        UpdateLevel::Check => Update::Check,
        UpdateLevel::Fetch => Update::Fetch,
        _ => Update::None,
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
/// (a `[[plugins]]` chained reference, an explicitly enabled crate). Both
/// answer with [`PluginOffer`]s: an id, a content directory, and an
/// *unvalidated* manifest, and are best-effort: failures are logged and
/// dropped, not surfaced, so one bad plugin never aborts a sync or hook.
///
/// Validating an offer into a [`ParsedPlugin`] is [`PmInstance`]'s job, not the
/// PM's, because the policy depends on where the offer came from rather than
/// on what it says. That is the whole trust boundary: see [`OfferKind`].
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
    async fn active_plugins(&self, deps: &[PackageId]) -> Vec<PluginOffer>;

    /// The plugin(s) a specific id maps to — zero, one, or many. Used for
    /// `[[plugins]]` chained references and explicitly enabled crates.
    async fn load_plugin(&self, id: &PackageId) -> Vec<PluginOffer>;

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
}

/// Where a registry instance's content comes from — the git-vs-path
/// distinction, made visible on the PM itself rather than inferred elsewhere.
pub enum RegistrySource {
    /// A git repository, fetched into the plugin-source cache.
    Git { url: String },
    /// A local directory.
    Path { dir: PathBuf },
}

/// Which validation policy an instance's offers get.
///
/// Symposium's decision, keyed on the kind of instance the offer came from:
/// never something a package manager states about itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfferKind {
    /// A curated registry entry. It must name itself, `[defaults]` is
    /// rejected, and one that references no dependency anywhere loads
    /// [dormant](crate::plugins::Plugin::requires_use): there is nothing to
    /// infer a gate from, and "always on" would fire it in every workspace.
    Registry,
    /// A package in an ecosystem. Its id supplies the name, the reference that
    /// reached it supplies the gate (so dormancy does not apply), and it picks
    /// up the default `skills/` group.
    Package,
}

/// A package-manager instance: its attribution name (the config registry name,
/// or `cargo` for the transport: the `pm` component of every id it owns), the
/// policy its offers are validated under, a trust marker, and the PM itself.
pub struct PmInstance {
    pub name: String,
    /// Whether this instance's [`active_plugins`](PackageManager::active_plugins)
    /// are trust roots. Registries and the workspace are trusted; the cargo
    /// transport is not, since its `active_plugins` are the plugins *embedded in
    /// dependencies*, which run only with the user's consent.
    pub trusted: bool,
    /// Validation policy for this instance's offers.
    pub kind: OfferKind,
    pub pm: Box<dyn PackageManager + Send + Sync>,
}

impl PmInstance {
    /// This instance's active plugins, validated. An offer that fails
    /// validation is logged and dropped: best-effort, like the PM layer above.
    pub async fn active_plugins(&self, deps: &[PackageId]) -> Vec<ParsedPlugin> {
        self.validate_all(self.pm.active_plugins(deps).await)
    }

    /// The plugin(s) an id maps to, validated.
    pub async fn load_plugin(&self, id: &PackageId) -> Vec<ParsedPlugin> {
        self.validate_all(self.pm.load_plugin(id).await)
    }

    fn validate_all(&self, offers: Vec<PluginOffer>) -> Vec<ParsedPlugin> {
        offers
            .into_iter()
            .filter_map(|offer| {
                let id = offer.id.clone();
                match crate::plugins::validate_offer(offer, self.kind) {
                    Ok(p) => Some(p),
                    Err(e) => {
                        tracing::warn!(
                            report = %crate::report::ReportEvent::Warning {
                                message: format!("skipping {id}: {e:#}"),
                            },
                        );
                        None
                    }
                }
            })
            .collect()
    }
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
    /// contribute a plugin relevant to the id, so this can return several. Each
    /// instance's offers are validated under its own [`OfferKind`].
    pub async fn load_plugin(&self, id: &PackageId) -> Vec<ParsedPlugin> {
        let mut out = Vec::new();
        for inst in &self.instances {
            out.extend(inst.load_plugin(id).await);
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
