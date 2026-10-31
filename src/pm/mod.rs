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

use crate::plugins::Plugin;

mod cargo;
mod git;
pub mod layout;
mod path;
pub(crate) use cargo::CRATES_IO_REGISTRY_URL;
pub(crate) use cargo::classify_package_source;
pub use cargo::{
    CargoPm, LoadedWorkspace, WorkspaceCrate, WorkspaceDeps, file_mtime, workspace_dir_name,
};
pub use git::GitPm;
pub use path::PathPm;

/// The identity types are the SDK's, since they cross the PM boundary: a
/// package-manager binary speaks in them too.
pub use symposium_sdk::pm::{
    ANY_VERSION, CARGO_PM, PackageId, PackageProvenance, PluginInfo, SourcedPackage,
    UnvalidatedPlugin,
};

/// A fetched package: its sourced identity plus the directory holding its
/// content.
#[derive(Debug, Clone)]
pub struct FetchedPackage {
    pub package: SourcedPackage,
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
/// answer with [`UnvalidatedPlugin`]s: a sourced id, a content directory, and
/// an *unvalidated* manifest. They are best-effort: failures are logged and
/// dropped, so one bad plugin never aborts a sync or hook. Every
/// result-bearing operation carries a [`SourcedPackage`], so source provenance
/// cannot be separated from package identity while it crosses the PM boundary.
///
/// Validating an [`UnvalidatedPlugin`] into a [`Plugin`] is [`PmInstance`]'s
/// job, not the PM's, because the policy depends on where the plugin came from
/// rather than on what it says. That is the whole trust boundary: see
/// [`PluginKind`].
#[async_trait::async_trait]
pub trait PackageManager {
    /// The PM's registry name — the `pm` component of every id it owns. For
    /// an ecosystem transport this is the ecosystem (`cargo`); for a registry
    /// instance it is the configured registry's name.
    fn name(&self) -> &str;

    fn canonical_name(&self, name: &str) -> String;

    /// The package this PM has under `name`, compared by
    /// [`canonical_name`](Self::canonical_name). Under [`UpdateLevel::None`]
    /// only what is already on disk counts. A registry's packages are the
    /// plugins it lists, which [`PmRegistry::identify`] matches directly, so
    /// only a transport answers here.
    async fn find(&self, _name: &str, _update: UpdateLevel) -> Option<PackageId> {
        None
    }

    /// The plugins this PM activates for the workspace's sourced dependency
    /// set. A registry lists its own entries (deps ignored); the cargo
    /// transport surfaces the plugins its dependencies embed. Whether a
    /// dependency-embedded plugin is *trusted* is the caller's decision — see
    /// [`PmInstance::trusted`].
    async fn active_plugins(&self, deps: &[SourcedPackage]) -> Vec<UnvalidatedPlugin>;

    /// Fails instead of returning an empty list when the source cannot be
    /// read, since sync removes what a registry stops offering.
    async fn registry_plugins(&self) -> Result<RegistryListing<UnvalidatedPlugin>> {
        Ok(RegistryListing {
            plugins: self.active_plugins(&[]).await,
            complete: true,
        })
    }

    /// The plugin(s) a specific id maps to — zero, one, or many. Used for
    /// `[[plugins]]` chained references and explicitly enabled crates.
    async fn load_plugin(&self, id: &PackageId, update: UpdateLevel) -> Vec<UnvalidatedPlugin>;

    /// The sourced packages the current workspace depends on. Empty for PMs
    /// with no workspace notion.
    async fn list_deps(&self) -> Result<Vec<SourcedPackage>>;

    /// Find packages matching a partial query (backs `use` / `search`). PMs
    /// without a searchable registry return an empty list.
    async fn search(&self, query: &str) -> Result<Vec<PluginInfo>>;

    /// Acquire a package's source content, resolving its version when the
    /// ecosystem has one and reporting the source that supplied it.
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
    fn registry_source(&self) -> Option<ConfiguredRegistrySource> {
        None
    }
}

/// Where a registry instance's content comes from — the git-vs-path
/// distinction, made visible on the PM itself rather than inferred elsewhere.
pub enum ConfiguredRegistrySource {
    /// A git repository, fetched into the plugin-source cache.
    Git { url: String },
    /// A local directory.
    Path { dir: PathBuf },
}

/// Which validation policy an instance's plugins get.
///
/// Symposium's decision, keyed on the kind of instance the plugin came from:
/// never something a package manager states about itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginKind {
    /// A curated registry entry. It must name itself, `[defaults]` is
    /// rejected, and one that references no dependency anywhere loads
    /// [dormant](crate::plugins::PluginManifest::requires_use): there is nothing to
    /// infer a gate from, and "always on" would fire it in every workspace.
    Registry,
    /// A package in an ecosystem. Its id supplies the name, the reference that
    /// reached it supplies the gate (so dormancy does not apply), and it picks
    /// up the default `skills/` group.
    Package,
}

/// A package-manager instance: its attribution name (the config registry name,
/// or `cargo` for the transport: the `pm` component of every id it owns), the
/// policy its plugins are validated under, a trust marker, and the PM itself.
pub struct PmInstance {
    pub name: String,
    /// Whether this instance's [`active_plugins`](PackageManager::active_plugins)
    /// are trust roots. Registries and the workspace are trusted; the cargo
    /// transport is not, since its `active_plugins` are the plugins *embedded in
    /// dependencies*, which run only with the user's consent.
    pub trusted: bool,
    /// Validation policy for this instance's plugins.
    pub kind: PluginKind,
    pub pm: Box<dyn PackageManager + Send + Sync>,
}

impl PmInstance {
    /// This instance's active plugins, validated. A plugin that fails
    /// validation is logged and dropped: best-effort, like the PM layer above.
    pub async fn active_plugins(&self, deps: &[SourcedPackage]) -> Vec<Plugin> {
        self.validate_all(self.pm.active_plugins(deps).await)
    }

    pub async fn registry_plugins(&self) -> Option<RegistryListing<Plugin>> {
        match self.pm.registry_plugins().await {
            Ok(listing) => {
                let total = listing.plugins.len();
                let plugins = self.validate_all(listing.plugins);
                Some(RegistryListing {
                    complete: listing.complete && plugins.len() == total,
                    plugins,
                })
            }
            Err(e) => {
                tracing::warn!(
                    report = %crate::report::ReportEvent::Warning {
                        message: format!("{e:#}"),
                    },
                );
                None
            }
        }
    }

    /// The plugin(s) an id maps to, validated.
    pub async fn load_plugin(&self, id: &PackageId, update: UpdateLevel) -> Vec<Plugin> {
        self.validate_all(self.pm.load_plugin(id, update).await)
    }

    /// A registry entry that fails validation is the user's to fix, so it is
    /// reported. A package's is its author's, and the hook path must not print
    /// it next to the agent's JSON, so it is only logged.
    fn validate_all(&self, unvalidated: Vec<UnvalidatedPlugin>) -> Vec<Plugin> {
        unvalidated
            .into_iter()
            .filter_map(|plugin| {
                let id = plugin.package.id.clone();
                match crate::plugins::validate_plugin(plugin, self.kind) {
                    Ok(p) => Some(p),
                    Err(e) => {
                        match self.kind {
                            PluginKind::Registry => tracing::warn!(
                                report = %crate::report::ReportEvent::Warning {
                                    message: format!("skipping {e:#}"),
                                },
                            ),
                            PluginKind::Package => {
                                tracing::warn!(id = %id, error = %e, "failed to validate plugin")
                            }
                        }
                        None
                    }
                }
            })
            .collect()
    }
}

#[derive(Debug)]
pub struct RegistryListing<P> {
    pub plugins: Vec<P>,
    pub complete: bool,
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

    pub fn identity(&self, id: &PackageId) -> PackageId {
        let name = match self.owner(&id.pm, id) {
            Ok(pm) => pm.canonical_name(&id.name),
            Err(_) => id.name.clone(),
        };
        PackageId::any_version(&id.pm, name)
    }

    /// The identity of the package a user-typed `name` refers to. A plugin in
    /// `plugins` with that name under its registry's rule wins; otherwise the
    /// first package manager whose [`find`](PackageManager::find) has the name
    /// at `update` answers. A name no package manager has is an error.
    pub async fn identify(
        &self,
        plugins: &[Plugin],
        name: &str,
        update: UpdateLevel,
    ) -> Result<PackageId> {
        if let Some(plugin) = plugins.iter().find(|p| {
            self.identity(&PackageId::any_version(&p.package.id.pm, &p.manifest.name))
                == self.identity(&PackageId::any_version(&p.package.id.pm, name))
        }) {
            return Ok(self.identity(&plugin.package.id));
        }
        for inst in &self.instances {
            if let Some(id) = inst.pm.find(name, update).await {
                return Ok(self.identity(&id));
            }
        }
        anyhow::bail!("no package named `{name}`")
    }

    /// Fetch a package via the instance named in its id.
    pub async fn fetch(&self, id: &PackageId, update: UpdateLevel) -> Result<FetchedPackage> {
        self.owner(&id.pm, id)?.fetch(id, update).await
    }

    /// Union of `list_deps` across the instances — the workspace's full
    /// dependency set for discovery and `depends-on` predicate evaluation.
    pub async fn list_deps(&self) -> Result<Vec<SourcedPackage>> {
        let mut deps = Vec::new();
        for inst in &self.instances {
            deps.extend(inst.pm.list_deps().await?);
        }
        Ok(deps)
    }

    /// Plugins embedded in the exact dependency packages supplied by the
    /// workspace resolver.
    ///
    /// Only untrusted transports participate: configured registries are trust
    /// roots already present in the plugin registry, while a dependency needs
    /// the user's enablement decision before its plugin becomes active.
    pub async fn plugins_for_dependencies(&self, deps: &[SourcedPackage]) -> Vec<Plugin> {
        let mut plugins = Vec::new();
        for instance in self.instances.iter().filter(|instance| !instance.trusted) {
            plugins.extend(instance.active_plugins(deps).await);
        }
        plugins
    }

    /// Load the plugin(s) an id maps to, asking every instance. Any instance may
    /// contribute a plugin relevant to the id, so this can return several. Each
    /// instance's plugins are validated under its own [`PluginKind`].
    pub async fn load_plugin(&self, id: &PackageId, update: UpdateLevel) -> Vec<Plugin> {
        let mut out = Vec::new();
        for inst in &self.instances {
            out.extend(inst.load_plugin(id, update).await);
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
        // Predicates compare package coordinates only. Keep this projection
        // explicit so source policy never grows an inference from `id.pm`.
        Ok(deps) => deps.into_iter().map(|package| package.id).collect(),
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

    /// A PM that answers with one plugin whose manifest fails validation.
    struct InvalidPluginPm;

    #[async_trait::async_trait]
    impl PackageManager for InvalidPluginPm {
        fn name(&self) -> &str {
            "stub"
        }
        fn canonical_name(&self, name: &str) -> String {
            name.to_string()
        }
        async fn active_plugins(&self, _deps: &[SourcedPackage]) -> Vec<UnvalidatedPlugin> {
            let manifest = symposium_sdk::manifest::RawPluginManifest::parse(
                "name = \"broken\"\n[[skills]]\n",
            )
            .unwrap();
            vec![UnvalidatedPlugin::new(
                SourcedPackage::new(
                    PackageId::any_version("stub", "broken"),
                    PackageProvenance::Unknown,
                ),
                PathBuf::from("/nonexistent/broken"),
                manifest,
            )]
        }
        async fn load_plugin(
            &self,
            _id: &PackageId,
            _update: UpdateLevel,
        ) -> Vec<UnvalidatedPlugin> {
            Vec::new()
        }
        async fn list_deps(&self) -> Result<Vec<SourcedPackage>> {
            Ok(Vec::new())
        }
        async fn search(&self, _query: &str) -> Result<Vec<PluginInfo>> {
            Ok(Vec::new())
        }
        async fn fetch(&self, id: &PackageId, _update: UpdateLevel) -> Result<FetchedPackage> {
            anyhow::bail!("cannot fetch {id}")
        }
    }

    /// The report events emitted while validating the stub's plugin under `kind`.
    async fn reports_for_invalid_plugin(kind: PluginKind) -> Vec<serde_json::Value> {
        use tracing_subscriber::layer::SubscriberExt;
        let (layer, handle) =
            crate::report::ReportLayer::new(crate::report::ReportMode::Json, tracing::Level::DEBUG);
        let _guard = tracing::subscriber::set_default(tracing_subscriber::registry().with(layer));
        let instance = PmInstance {
            name: "stub".into(),
            trusted: false,
            kind,
            pm: Box::new(InvalidPluginPm),
        };
        assert!(instance.active_plugins(&[]).await.is_empty());
        handle.drain()
    }

    #[tokio::test]
    async fn invalid_registry_plugin_is_reported_with_its_path() {
        let reports = reports_for_invalid_plugin(PluginKind::Registry).await;
        assert_eq!(reports.len(), 1, "{reports:?}");
        let message = reports[0]["message"].as_str().unwrap();
        assert!(message.contains("/nonexistent/broken"), "{message}");
    }

    /// A package's manifest is its author's problem, and the hook path prints
    /// reports next to the agent's JSON, so a package failure is only logged.
    #[tokio::test]
    async fn invalid_package_plugin_is_not_reported() {
        let reports = reports_for_invalid_plugin(PluginKind::Package).await;
        assert!(reports.is_empty(), "{reports:?}");
    }

    #[tokio::test]
    async fn git_registry_without_its_copy_is_unreadable() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = symposium_install::InstallContext::new(tmp.path().to_path_buf());
        let pm = GitPm::new("reg", "https://github.com/example/reg", false, ctx).unwrap();
        let err = pm.registry_plugins().await.unwrap_err();
        assert!(
            err.to_string().contains("cargo agents plugin sync reg"),
            "{err}"
        );
    }

    #[test]
    fn each_package_manager_decides_which_names_match() {
        let tmp = tempfile::tempdir().unwrap();
        let pms = PmRegistry::new(vec![
            PmInstance {
                name: CARGO_PM.into(),
                trusted: false,
                kind: PluginKind::Package,
                pm: Box::new(CargoPm::new(Arc::new(WorkspaceDeps::detached()))),
            },
            PmInstance {
                name: "reg".into(),
                trusted: true,
                kind: PluginKind::Registry,
                pm: Box::new(PathPm::new("reg", tmp.path())),
            },
        ]);
        assert_eq!(
            pms.identity(&PackageId::new(CARGO_PM, "widget_lib", "1.0.0")),
            pms.identity(&PackageId::any_version(CARGO_PM, "widget-lib"))
        );
        assert_ne!(
            pms.identity(&PackageId::any_version("reg", "my_plugin")),
            pms.identity(&PackageId::any_version("reg", "my-plugin"))
        );
    }

    /// A registry `reg` listing the plugins `widget-lib` and `my-plugin`,
    /// beside `cargo`, and those registry plugins, validated.
    async fn registry_beside(
        cargo: PmInstance,
        reg: &std::path::Path,
    ) -> (PmRegistry, Vec<Plugin>) {
        for name in ["widget-lib", "my-plugin"] {
            std::fs::create_dir_all(reg.join(name)).unwrap();
            std::fs::write(
                reg.join(name).join("Symposium.toml"),
                format!("name = \"{name}\"\ndepends-on = [\"*\"]\n"),
            )
            .unwrap();
        }
        let registry = PmInstance {
            name: "reg".into(),
            trusted: true,
            kind: PluginKind::Registry,
            pm: Box::new(PathPm::new("reg", reg)),
        };
        let plugins = registry.active_plugins(&[]).await;
        assert_eq!(plugins.len(), 2);
        (PmRegistry::new(vec![cargo, registry]), plugins)
    }

    #[tokio::test]
    async fn identify_asks_each_package_manager_and_the_registry_wins() {
        let tmp = tempfile::tempdir().unwrap();
        let deps = WorkspaceDeps::fixture(
            tmp.path(),
            vec![
                WorkspaceCrate::new(
                    "widget-lib".into(),
                    semver::Version::new(1, 0, 0),
                    PackageProvenance::Unknown,
                    None,
                ),
                WorkspaceCrate::new(
                    "serde".into(),
                    semver::Version::new(1, 0, 0),
                    PackageProvenance::Unknown,
                    None,
                ),
            ],
        );
        let cargo = PmInstance {
            name: CARGO_PM.into(),
            trusted: false,
            kind: PluginKind::Package,
            pm: Box::new(CargoPm::new(deps)),
        };
        let (pms, plugins) = registry_beside(cargo, &tmp.path().join("reg")).await;
        let registry_id = |name: &str| {
            let plugin = plugins.iter().find(|p| p.manifest.name == name).unwrap();
            pms.identity(&plugin.package.id)
        };

        assert_eq!(
            pms.identify(&plugins, "serde", UpdateLevel::None)
                .await
                .unwrap(),
            PackageId::any_version(CARGO_PM, "serde")
        );
        assert_eq!(
            pms.identify(&plugins, "my-plugin", UpdateLevel::None)
                .await
                .unwrap(),
            registry_id("my-plugin")
        );
        assert_eq!(
            pms.identify(&plugins, "widget-lib", UpdateLevel::None)
                .await
                .unwrap(),
            registry_id("widget-lib")
        );
        assert_eq!(
            pms.identify(&plugins, "widget_lib", UpdateLevel::None)
                .await
                .unwrap(),
            PackageId::any_version(CARGO_PM, "widget_lib")
        );
    }

    /// A name no package manager has is an error rather than a crate assumed
    /// to exist, including a registry plugin's name spelled the way only
    /// cargo would accept it.
    #[tokio::test]
    async fn identify_rejects_a_name_nobody_has() {
        let tmp = tempfile::tempdir().unwrap();
        let (cargo, _) = crate::test_utils::FakeCargo::instance(&[], &[]);
        let (pms, plugins) = registry_beside(cargo, tmp.path()).await;
        for name in ["ghost", "my_plugin"] {
            let err = pms
                .identify(&plugins, name, UpdateLevel::Check)
                .await
                .unwrap_err();
            assert_eq!(err.to_string(), format!("no package named `{name}`"));
        }
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
