//! Package managers: the seam from the [registry-centric plugin
//! distribution RFD](../../md/rfds/registry-centric-plugins/README.md).
//!
//! A [`PackageId`] names a package as a `(pm, name, version)` tuple, and a
//! [`PackageManager`] resolves ids of its ecosystem to content on disk. The
//! trait is the SDK's, so a package manager written against it runs either
//! way: called directly in this process, or as a binary behind [`RemotePm`].
//!
//! A `PackageManager` value is an *instance*, not just an ecosystem. A
//! **transport** ([`CargoPm`]) can `fetch` any id of its ecosystem, because the
//! id carries the source; a **registry instance** ([`PathPm`]) fronts one
//! configured source and enumerates the packages it contains. [`PmRegistry`]
//! holds them as one flat set: an id is dispatched to the instance whose
//! [`PackageManager::name`] matches its [`PackageId::pm`] (`fetch`,
//! `load_plugin`), while `list_deps` and `search` union every instance.
//!
//! A registry instance's [`PackageManager::name`] is the *configured registry
//! name* (`user-plugins`, `symposium-recommendations`, …), which is also the
//! `pm` component of every id it mints and the name its plugins are attributed
//! to. Registry instances resolve their own ids, so those ids are never routed
//! through the ecosystem transports.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use symposium_install::UpdateLevel;

use crate::plugins::Plugin;

mod admission;
mod git;
pub mod layout;
mod path;
pub mod remote;

pub use admission::ResolvedContent;
use admission::ResolvedDependency;
pub(crate) use admission::{PackageNamingView, ResolvedPackage};
pub use git::GitPm;
pub use path::PathPm;
pub use remote::{RemotePm, RemotePmCommand};
/// The cargo package manager is its own crate, because it has to be one to run
/// as its own binary. Re-exported here so the rest of Symposium keeps naming one `pm`
/// module regardless of where a PM's code lives.
pub(crate) use symposium_pm_cargo::embedded_plugin_kind;
pub use symposium_pm_cargo::{
    CargoPm, LoadedWorkspace, WorkspaceCrate, WorkspaceDeps, file_mtime, workspace_dir_name,
};

/// The identity types and the operation set are the SDK's, since they cross
/// the PM boundary: a package-manager binary speaks in them too.
pub use symposium_sdk::pm::{
    ANY_VERSION, CARGO_PM, FetchedPackage, PackageId, PackageKey, PackageLoadTarget,
    PackageManager, PackageResolution, PackageSourceClaim, PluginInfo, PluginListing, ProjectInfo,
    UnvalidatedPlugin,
};

/// The Git source core recognizes as Symposium's recommendations registry.
pub(crate) const BUILTIN_RECOMMENDATIONS_URL: &str =
    "https://github.com/symposium-dev/recommendations";

/// Where a registry instance's content comes from — the git-vs-path
/// distinction, made visible on the PM itself rather than inferred elsewhere.
pub enum RegistrySource {
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

/// One package-plugin load selected by core.
///
/// Dependencies retain their admitted identity. A coordinate is used only
/// when a reference has not selected a package in the dependency snapshot.
#[derive(Clone)]
pub(crate) enum PackageRequest {
    Resolved(ResolvedPackage),
    Coordinate(PackageId),
}

pub(crate) struct ValidatedPluginListing {
    pub(crate) plugins: Vec<Plugin>,
    complete: bool,
}

impl ValidatedPluginListing {
    pub(crate) fn is_complete(&self) -> bool {
        self.complete
    }
}

impl PackageRequest {
    pub(crate) fn coordinate(id: PackageId) -> Self {
        Self::Coordinate(id)
    }

    pub(crate) fn id(&self) -> &PackageId {
        match self {
            Self::Resolved(package) => package.id(),
            Self::Coordinate(id) => id,
        }
    }
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
    /// How a registry's content is sourced; `None` for a transport.
    pub source: Option<RegistrySource>,
    /// Whether Symposium refreshes this registry on its own, rather than only
    /// when the user names it (`plugin sync <name>`).
    pub auto_update: bool,
    pub pm: Box<dyn PackageManager>,
    admission: admission::PackageAdmission,
    /// Whether an operation on this instance has failed. Its silence is not
    /// an empty answer, so nothing it would have offered is removed on its
    /// strength.
    failed: AtomicBool,
}

impl PmInstance {
    pub fn new(
        name: impl Into<String>,
        trusted: bool,
        kind: PluginKind,
        pm: Box<dyn PackageManager>,
    ) -> Self {
        Self::with_authority(name, trusted, kind, pm, admission::PackageAuthority::Remote)
    }

    fn with_authority(
        name: impl Into<String>,
        trusted: bool,
        kind: PluginKind,
        pm: Box<dyn PackageManager>,
        authority: admission::PackageAuthority,
    ) -> Self {
        let name = name.into();
        Self {
            admission: admission::PackageAdmission::new(name.clone(), authority),
            name,
            trusted,
            kind,
            source: None,
            auto_update: true,
            pm,
            failed: AtomicBool::new(false),
        }
    }

    pub fn with_source(mut self, source: RegistrySource) -> Self {
        let authority = match &source {
            RegistrySource::Git { url } if url == BUILTIN_RECOMMENDATIONS_URL => {
                admission::PackageAuthority::Recommendations
            }
            RegistrySource::Git { .. } | RegistrySource::Path { .. } => {
                admission::PackageAuthority::Remote
            }
        };
        self.admission = admission::PackageAdmission::new(self.name.clone(), authority);
        self.source = Some(source);
        self
    }

    /// Whether an operation on this instance has failed.
    pub fn has_failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }

    /// Record a failed operation and log it.
    fn failure<T>(&self, operation: &str, e: anyhow::Error) -> Result<T> {
        self.failed.store(true, Ordering::Relaxed);
        tracing::warn!(instance = %self.name, operation, error = %format!("{e:#}"), "package manager failed");
        Err(e)
    }

    /// Package-specific errors may be ignored by lookups and plugin loading,
    /// but losing the transport invalidates this instance's answers for cleanup.
    fn record_transport_failure<T>(&self, operation: &str, result: Result<T>) -> Result<T> {
        match result {
            Err(e) if e.is::<remote::Unavailable>() => self.failure(operation, e),
            result => result,
        }
    }

    fn mark_incomplete(&self, operation: &str) {
        self.failed.store(true, Ordering::Relaxed);
        tracing::warn!(
            instance = %self.name,
            operation,
            "package manager returned an incomplete plugin listing"
        );
    }

    async fn admit_dependencies(&self) -> Result<Vec<ResolvedDependency>> {
        let found = match self.pm.list_deps().await {
            Ok(found) => found,
            Err(error) => {
                self.admission.invalidate_dependency_snapshot();
                return self.failure("list_deps", error);
            }
        };
        match self.admission.admit_dependencies(found) {
            Ok(dependencies) => Ok(dependencies),
            Err(error) => {
                self.admission.invalidate_dependency_snapshot();
                self.failure("list_deps", error.into())
            }
        }
    }

    /// Recover supplier-session keys when the durable workspace cache reused
    /// only coordinates from a previous invocation.
    async fn ensure_dependencies_admitted(&self, ids: &[PackageId]) -> Result<()> {
        let needs_snapshot =
            ids.iter().any(|id| id.pm == self.name) && !self.admission.has_dependency_snapshot();
        if needs_snapshot {
            self.admit_dependencies().await?;
        }
        Ok(())
    }

    fn load_target(&self, request: &PackageRequest) -> Result<PackageLoadTarget> {
        match request {
            PackageRequest::Resolved(package) => self
                .admission
                .project(package.handle())
                .map(|resolution| PackageLoadTarget::resolved(resolution.key().clone()))
                .map_err(Into::into),
            PackageRequest::Coordinate(id) => Ok(PackageLoadTarget::coordinate(id.clone())),
        }
    }

    async fn resolved_dependencies(&self, ids: &[PackageId]) -> Result<Vec<ResolvedDependency>> {
        self.ensure_dependencies_admitted(ids).await?;
        Ok(self.admission.resolved_dependencies(ids))
    }

    /// This instance's active plugins, validated. Each entry the PM could not
    /// read is reported, and a plugin that fails validation is reported and
    /// dropped; either leaves the listing with errors.
    async fn try_active_plugins(&self, deps: &[PackageId]) -> Result<ValidatedPluginListing> {
        self.ensure_dependencies_admitted(deps).await?;
        let deps = self.admission.project_dependencies(deps);
        match self.pm.active_plugins(&deps).await {
            Ok(listing) => {
                let listing = self.validate_listing(listing);
                if !listing.is_complete() {
                    self.mark_incomplete("active_plugins");
                }
                Ok(listing)
            }
            Err(e) => self.failure("active_plugins", e),
        }
    }

    /// [`try_active_plugins`](Self::try_active_plugins), with a failure
    /// contributing nothing.
    pub async fn active_plugins(&self, deps: &[PackageId]) -> Vec<Plugin> {
        self.try_active_plugins(deps)
            .await
            .map(|listing| listing.plugins)
            .unwrap_or_default()
    }

    /// A registry's plugins, or `None` with a report when it cannot be read:
    /// sync removes what a registry stops offering, so an unreadable one must
    /// not read as an empty one, nor an incomplete one as all it offers.
    pub(crate) async fn registry_plugins(&self) -> Option<ValidatedPluginListing> {
        match self.try_active_plugins(&[]).await {
            Ok(listing) => Some(listing),
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
    ///
    /// A package-specific error is logged without blocking stale-artifact
    /// cleanup for the ecosystem. A transport failure is recorded against the
    /// instance: missing answers from a dead PM cannot authorize cleanup.
    async fn load_plugin(&self, target: &PackageLoadTarget, update: UpdateLevel) -> Vec<Plugin> {
        match self
            .record_transport_failure("load_plugin", self.pm.load_plugin(target, update).await)
        {
            Ok(mut listing) => {
                let mut exact_mismatch = false;
                if let PackageLoadTarget::Resolved { key } = target {
                    let before = listing.plugins.len();
                    listing.plugins.retain(|plugin| plugin.package.key() == key);
                    if listing.plugins.len() != before {
                        exact_mismatch = true;
                        self.skipped("exact package load answered with another package key");
                    }
                }
                let mut listing = self.validate_listing(listing);
                listing.complete &= !exact_mismatch;
                if !listing.is_complete() {
                    self.mark_incomplete("load_plugin");
                }
                listing.plugins
            }
            Err(e) => {
                tracing::warn!(instance = %self.name, error = %format!("{e:#}"), "failed to load package");
                Vec::new()
            }
        }
    }

    /// The package this instance has under `name`, compared under its
    /// [`canonical_name`](Self::canonical_name): a dependency of that name;
    /// otherwise one already on disk (a cache-only fetch); otherwise, above
    /// [`UpdateLevel::None`], one its search turns up. A registry's packages are the plugins it lists, which
    /// callers hold already, so a trusted instance has none to find.
    ///
    /// Built from the operations every package manager has, so no package
    /// manager implements it and the protocol has no request for it.
    pub async fn find(&self, name: &str, update: UpdateLevel) -> Option<PackageId> {
        if self.trusted {
            return None;
        }
        let wanted = self.canonical_name(name).await.ok()?;
        if let Ok(deps) = self.admit_dependencies().await {
            let deps: Vec<_> = deps
                .into_iter()
                .filter(|dep| dep.id().pm == self.name)
                .collect();
            if let Some(dep) = deps
                .iter()
                .find(|dep| dep.id().name == wanted || dep.id().name == name)
            {
                return Some(dep.id().clone());
            }
            let spellings: Vec<_> = deps.iter().map(|dep| dep.id().name.clone()).collect();
            let canonical = self.canonical_names(&spellings).await.ok()?;
            if let Some((dep, _)) = deps
                .into_iter()
                .zip(canonical)
                .find(|(_, known)| *known == wanted)
            {
                return Some(dep.id().clone());
            }
        }
        let named = PackageId::any_version(&self.name, name);
        if let Ok(cached) =
            self.record_transport_failure("fetch", self.pm.fetch(&named, UpdateLevel::None).await)
            && self.names(cached.id(), &wanted).await
        {
            return self
                .admission
                .admit_content(cached.package, &cached.root)
                .ok()
                .map(|content| content.id().clone());
        }
        if matches!(update, UpdateLevel::None) {
            return None;
        }
        for info in self
            .record_transport_failure("search", self.pm.search(name).await)
            .ok()?
        {
            if self.names(&info.id, &wanted).await {
                return Some(info.id);
            }
        }
        None
    }

    /// Whether `id` is this instance's package whose canonical name is
    /// `canonical`.
    async fn names(&self, id: &PackageId, canonical: &str) -> bool {
        id.pm == self.name
            && self
                .canonical_name(&id.name)
                .await
                .is_ok_and(|name| name == canonical)
    }

    /// Normalize a name, recording failures so incomplete answers cannot
    /// authorize cleanup or bypass a disable entry.
    pub async fn canonical_name(&self, name: &str) -> Result<String> {
        match self.pm.canonical_name(name).await {
            Ok(canonical) => Ok(canonical),
            Err(e) => self.failure("canonical_name", e),
        }
    }

    /// Normalize related names together, recording incomplete answers as failures.
    pub async fn canonical_names(&self, names: &[String]) -> Result<Vec<String>> {
        if names.is_empty() {
            return Ok(Vec::new());
        }
        match self.pm.canonical_names(names).await {
            Ok(canonical) if canonical.len() == names.len() => Ok(canonical),
            Ok(_) => self.failure(
                "canonical_names",
                anyhow::anyhow!("wrong number of canonical names"),
            ),
            Err(error) => self.failure("canonical_names", error),
        }
    }

    /// Whether `id` belongs to this instance. Every id a PM answers with must:
    /// ids route by their `pm` component, and consent and `disable` are keyed
    /// on them, so a PM answering for another could take the consent given to
    /// that PM's packages, or escape a `disable` entry naming its own.
    fn owns(&self, id: &PackageId) -> bool {
        if id.pm != self.name {
            tracing::warn!(instance = %self.name, id = %id, "package manager answered with another package manager's id");
            return false;
        }
        true
    }

    /// A registry entry that fails validation is the user's to fix, so it is
    /// reported. A package's is its author's, and the hook path must not print
    /// it next to the agent's JSON, so it is only logged.
    fn validate_listing(&self, listing: PluginListing) -> ValidatedPluginListing {
        let total = listing.plugins.len();
        let mut complete = listing.errors.is_empty();
        if !complete {
            self.skipped("package manager reported unreadable plugin entries");
        }

        // Conflicting answers for one supplier key reject every entry carrying
        // that key. Choosing the first would make listing order authoritative.
        let mut resolutions = HashMap::<PackageKey, PackageResolution>::new();
        let mut conflicting_keys = HashSet::<PackageKey>::new();
        for plugin in &listing.plugins {
            match resolutions.get(plugin.package.key()) {
                Some(previous) if previous != &plugin.package => {
                    conflicting_keys.insert(plugin.package.key().clone());
                }
                Some(_) => {}
                None => {
                    resolutions.insert(plugin.package.key().clone(), plugin.package.clone());
                }
            }
        }

        let mut candidates = Vec::with_capacity(total);
        for mut plugin in listing.plugins {
            if conflicting_keys.contains(plugin.package.key()) {
                self.skipped("package listing contains a conflicting package key");
                continue;
            }
            let id = plugin.id().clone();
            if !absolute(&self.name, &id, &plugin.root)
                || plugin
                    .label_root
                    .as_ref()
                    .is_some_and(|dir| !absolute(&self.name, &id, dir))
            {
                continue;
            }
            let content = match self
                .admission
                .admit_content(plugin.package.clone(), &plugin.root)
            {
                Ok(content) => content,
                Err(error) => {
                    self.skipped(&error.to_string());
                    continue;
                }
            };
            plugin.root = content.root().to_owned();
            let handle = content.package().handle();
            match crate::plugins::validate_plugin(plugin, Some(content), self.kind) {
                Ok(plugin) => candidates.push((handle, plugin)),
                Err(error) => self.skipped(&format!("`{id}`: {error:#}")),
            }
        }

        // A package may offer several differently named plugins. Two answers
        // for the same package and validated name are instead one conflicting
        // identity, so reject both independent of their order.
        let mut identities = HashMap::new();
        for (handle, plugin) in &candidates {
            *identities
                .entry((*handle, plugin.manifest.name.clone()))
                .or_insert(0_usize) += 1;
        }
        let plugins: Vec<_> = candidates
            .into_iter()
            .filter_map(|(handle, plugin)| {
                (identities[&(handle, plugin.manifest.name.clone())] == 1).then_some(plugin)
            })
            .collect();
        if plugins.len() < total {
            complete = false;
        }
        ValidatedPluginListing { plugins, complete }
    }

    /// Report or log one entry this instance skipped, as for a plugin that
    /// fails validation.
    fn skipped(&self, error: &str) {
        match self.kind {
            PluginKind::Registry => tracing::warn!(
                report = %crate::report::ReportEvent::Warning {
                    message: format!("skipping {error}"),
                },
            ),
            PluginKind::Package => {
                tracing::warn!(instance = %self.name, error, "skipping a plugin")
            }
        }
    }

    /// This instance's project, checked: every path must be absolute, since a
    /// relative one would resolve against wherever Symposium happens to run.
    async fn project_info(&self) -> Result<ProjectInfo> {
        let project = match self.pm.project_info().await {
            Ok(project) => project,
            Err(e) => return self.failure("project_info", e),
        };
        let relative = project
            .root
            .iter()
            .chain(&project.local_packages)
            .chain(&project.inputs)
            .find(|path| !path.is_absolute());
        match relative {
            Some(path) => self.failure(
                "project_info",
                anyhow::anyhow!("reported a relative path, {}", path.display()),
            ),
            None => Ok(project),
        }
    }
}

/// Construct core's in-process Cargo supplier.
///
/// A configured `[[package-manager]]` named `cargo` still goes through
/// [`PmInstance::new`] and therefore receives remote authority.
pub(crate) fn builtin_cargo_instance(workspace: std::sync::Arc<WorkspaceDeps>) -> PmInstance {
    PmInstance::with_authority(
        CARGO_PM,
        false,
        PluginKind::Package,
        Box::new(CargoPm::new(workspace)),
        admission::PackageAuthority::BuiltinCargo,
    )
}

#[cfg(test)]
pub(crate) fn resolved_content_for_test(root: &std::path::Path) -> ResolvedContent {
    let admission = admission::PackageAdmission::new(
        CARGO_PM.to_string(),
        admission::PackageAuthority::BuiltinCargo,
    );
    let resolution = PackageResolution::new(
        PackageId::new(CARGO_PM, "test-package", "1.0.0"),
        PackageSourceClaim::CratesIo,
        PackageKey::new("test-package-key").unwrap(),
    );
    admission.admit_content(resolution, root).unwrap()
}

/// Whether a directory a package manager gave for `id` is absolute. A relative
/// one would resolve against wherever Symposium happens to run, which differs
/// from one agent's hook to another's, so the plugin is dropped.
fn absolute(instance: &str, id: &PackageId, dir: &std::path::Path) -> bool {
    if !dir.is_absolute() {
        tracing::warn!(instance, id = %id, dir = %dir.display(), "package manager answered with a relative path");
    }
    dir.is_absolute()
}

/// The active set of package-manager instances — one flat collection, the cargo
/// transport alongside one instance per configured registry. Ids are dispatched
/// by their `pm` component ([`PackageId::pm`]) to the instance that owns them.
pub struct PmRegistry {
    instances: Vec<PmInstance>,
    /// The payload digests computed through this set, so one invocation
    /// hashes each loaded payload once however many routes look it up. The
    /// package managers never read it: it belongs to the [audits](crate::audits)
    /// reader, and lives here because this is the per-invocation value every
    /// lookup is handed.
    pub(crate) payload_digests: crate::audits::DigestMemo,
}

impl PmRegistry {
    pub fn new(instances: Vec<PmInstance>) -> Self {
        Self {
            instances,
            payload_digests: Default::default(),
        }
    }

    /// Every instance, in order (cargo transport first, then registries).
    pub fn instances(&self) -> impl Iterator<Item = &PmInstance> {
        self.instances.iter()
    }

    fn owner(&self, id: &PackageId) -> Option<&PmInstance> {
        self.instances.iter().find(|inst| inst.name == id.pm)
    }

    /// The instances an operation has failed on so far.
    pub fn failed(&self) -> Vec<String> {
        self.instances
            .iter()
            .filter(|inst| inst.has_failed())
            .map(|inst| inst.name.clone())
            .collect()
    }

    /// `id` with its version dropped: the form two ids of the same package
    /// compare equal in, since a package manager spells each of its packages
    /// one way.
    pub fn identity(&self, id: &PackageId) -> PackageId {
        PackageId::any_version(&id.pm, &id.name)
    }

    /// `name` under the [`canonical_name`](PackageManager::canonical_name) of
    /// the package manager called `pm`, or as it is for one not configured.
    pub async fn canonical_name(&self, pm: &str, name: &str) -> Result<String> {
        match self.instances.iter().find(|inst| inst.name == pm) {
            Some(inst) => inst.canonical_name(name).await,
            None => Ok(name.to_string()),
        }
    }

    pub async fn canonical_names(&self, pm: &str, names: &[String]) -> Result<Vec<String>> {
        match self.instances.iter().find(|inst| inst.name == pm) {
            Some(inst) => inst.canonical_names(names).await,
            None => Ok(names.to_vec()),
        }
    }

    /// Fetch a package via the instance named in its id.
    pub async fn fetch(&self, id: &PackageId, update: UpdateLevel) -> Result<ResolvedContent> {
        let inst = self.owner(id).ok_or_else(|| {
            anyhow::anyhow!("unknown package manager `{}` in package id `{id}`", id.pm)
        })?;
        let fetched = inst.record_transport_failure("fetch", inst.pm.fetch(id, update).await)?;
        anyhow::ensure!(
            inst.owns(fetched.id()),
            "`{}` answered a fetch of `{id}` with `{}`",
            inst.name,
            fetched.id()
        );
        anyhow::ensure!(
            fetched.root.is_absolute(),
            "`{}` answered a fetch of `{id}` with a relative path, {}",
            inst.name,
            fetched.root.display()
        );
        Ok(inst
            .admission
            .admit_content(fetched.package, &fetched.root)?)
    }

    /// Union of `list_deps` across the instances — the workspace's full
    /// dependency set for discovery and `depends-on` predicate evaluation.
    ///
    /// An instance that fails contributes nothing, and the failure is
    /// recorded. Failing the union instead would leave every `depends-on`
    /// false, so one missing PM binary would turn off every plugin the other
    /// ecosystems activate.
    pub async fn list_deps(&self) -> Vec<PackageId> {
        let mut deps = Vec::new();
        for inst in &self.instances {
            if let Ok(admitted) = inst.admit_dependencies().await {
                deps.extend(
                    admitted
                        .into_iter()
                        .map(|dependency| dependency.id().clone()),
                );
            }
        }
        deps
    }

    /// Exact admitted dependencies matching `ids`, retaining distinct source
    /// identities even when their coordinates compare equal.
    pub(crate) async fn dependency_requests(&self, ids: &[PackageId]) -> Vec<PackageRequest> {
        let mut requests = Vec::new();
        for instance in &self.instances {
            if let Ok(dependencies) = instance.resolved_dependencies(ids).await {
                requests.extend(
                    dependencies
                        .into_iter()
                        .map(ResolvedDependency::into_package)
                        .map(PackageRequest::Resolved),
                );
            }
        }
        requests
    }

    /// Each instance's [`ProjectInfo`], by instance name, and why each one
    /// that failed did, by instance name. A failure is also recorded.
    pub async fn project_info(&self) -> (Vec<(String, ProjectInfo)>, Vec<(String, String)>) {
        let mut found = Vec::new();
        let mut failed = Vec::new();
        for inst in &self.instances {
            match inst.project_info().await {
                Ok(project) => found.push((inst.name.clone(), project)),
                Err(e) => failed.push((inst.name.clone(), format!("{e:#}"))),
            }
        }
        (found, failed)
    }

    /// Load the plugin(s) an id maps to, from the instance its `pm` names.
    ///
    /// Only that instance is asked. Asking every one would spawn every
    /// out-of-process PM to answer about an id none of them owns, and would let
    /// a PM answer for a package of another ecosystem.
    pub async fn load_plugin(&self, id: &PackageId, update: UpdateLevel) -> Vec<Plugin> {
        self.load_plugin_request(&PackageRequest::coordinate(id.clone()), update)
            .await
    }

    pub(crate) async fn load_plugin_request(
        &self,
        request: &PackageRequest,
        update: UpdateLevel,
    ) -> Vec<Plugin> {
        match self.owner(request.id()) {
            Some(inst) => {
                let target = match inst.load_target(request) {
                    Ok(target) => target,
                    Err(error) => {
                        tracing::warn!(instance = %inst.name, error = %error, "refusing package load");
                        return Vec::new();
                    }
                };
                inst.load_plugin(&target, update).await
            }
            None => {
                tracing::debug!(id = %request.id(), "no package manager owns this id");
                Vec::new()
            }
        }
    }

    /// Search every instance for packages matching `query`, tagged with the
    /// instance's display name. A failing instance is skipped with a debug log
    /// rather than failing the union.
    pub async fn search(&self, query: &str) -> Vec<(String, PluginInfo)> {
        let mut out = Vec::new();
        for inst in self.instances() {
            match inst.record_transport_failure("search", inst.pm.search(query).await) {
                Ok(infos) => out.extend(
                    infos
                        .into_iter()
                        .filter(|info| inst.owns(&info.id))
                        .map(|info| (inst.name.clone(), info)),
                ),
                Err(e) => {
                    tracing::debug!(instance = %inst.name, error = %e, "search failed, skipping");
                }
            }
        }
        out
    }
}

/// The workspace: the project the package managers report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceInfo {
    /// The project's root, where project configuration is installed.
    pub root: PathBuf,
    /// Local packages the package managers reported, those inside `root` only.
    /// Each may define a workspace plugin.
    pub members: Vec<PathBuf>,
    /// Files whose change means the package managers' answers may have changed.
    pub inputs: Vec<PathBuf>,
}

impl WorkspaceInfo {
    /// Combine each package manager's [`ProjectInfo`]. `None` when none found a
    /// project; an error when two found different roots, since picking one
    /// would install where the other ecosystem's tools do not look.
    fn combine(found: Vec<(String, ProjectInfo)>) -> Result<Option<Self>, String> {
        let mut root: Option<(String, PathBuf)> = None;
        let mut members = Vec::new();
        let mut inputs = Vec::new();
        for (pm, project) in found {
            inputs.extend(project.inputs);
            let Some(found_root) = project.root else {
                continue;
            };
            match &root {
                None => root = Some((pm, found_root)),
                Some((first, existing)) if canonical(existing) != canonical(&found_root) => {
                    return Err(format!(
                        "package managers disagree about the workspace root: `{first}` finds {}, `{pm}` finds {}",
                        existing.display(),
                        found_root.display()
                    ));
                }
                Some(_) => {}
            }
            members.extend(project.local_packages);
        }
        let Some((_, root)) = root else {
            return Ok(None);
        };
        let canonical_root = canonical(&root);
        members.retain(|member| canonical(member).starts_with(&canonical_root));
        Ok(Some(Self {
            root,
            members,
            inputs,
        }))
    }
}

fn canonical(path: &std::path::Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The per-invocation workspace context: the package managers, plus the two
/// facts about the workspace that everything above `pm/` needs: where it is
/// and what it depends on.
///
/// Both are the package managers' answers. Where the workspace is, is the
/// project they find from the starting directory, since where a project is
/// rooted is each ecosystem's own rule. Both are resolved on first use and
/// cached for the handle's lifetime, so one invocation shares one PM set: with
/// a PM in another process, building the set per call would spawn a process
/// per call. So it is threaded as an [`Arc`](std::sync::Arc).
pub struct Workspace {
    /// Where the package managers start looking. `None` for a
    /// workspace-independent operation.
    start: Option<PathBuf>,
    pms: PmRegistry,
    dep_ids: tokio::sync::OnceCell<Vec<PackageId>>,
    info: tokio::sync::OnceCell<Result<Option<WorkspaceInfo>, String>>,
}

impl Workspace {
    pub fn new(start: Option<PathBuf>, pms: PmRegistry) -> Self {
        Self {
            start,
            pms,
            dep_ids: tokio::sync::OnceCell::new(),
            info: tokio::sync::OnceCell::new(),
        }
    }

    /// A context over a pre-resolved cargo workspace, for tests: no
    /// `cargo metadata`, no registries, exactly `crates` rooted at `root`.
    #[cfg(test)]
    pub(crate) fn fixture(
        root: impl Into<PathBuf>,
        crates: Vec<WorkspaceCrate>,
    ) -> std::sync::Arc<Self> {
        let root = root.into();
        let deps = WorkspaceDeps::fixture(root.clone(), crates);
        std::sync::Arc::new(Self::new(
            Some(root),
            PmRegistry::new(vec![builtin_cargo_instance(deps)]),
        ))
    }

    /// The package managers active for this workspace.
    pub fn pms(&self) -> &PmRegistry {
        &self.pms
    }

    /// Where the package managers start looking, or `None` for a
    /// workspace-independent operation.
    pub fn start(&self) -> Option<&std::path::Path> {
        self.start.as_deref()
    }

    async fn lookup(&self) -> &Result<Option<WorkspaceInfo>, String> {
        self.info
            .get_or_init(|| async {
                if self.start.is_none() {
                    return Ok(None);
                }
                // A package manager that fails is left out and the others
                // decide, rather than failing the workspace: one broken PM must
                // not stop syncing for every ecosystem. The design chapter on
                // modules records the cost: the root it might have disagreed
                // about.
                let (found, failed) = self.pms.project_info().await;
                match WorkspaceInfo::combine(found) {
                    // With no project found, a failure is why: say so, rather
                    // than that there is no project here.
                    Ok(None) if !failed.is_empty() => Err(failed
                        .iter()
                        .map(|(name, e)| format!("`{name}` could not describe the project: {e}"))
                        .collect::<Vec<_>>()
                        .join("; ")),
                    combined => combined,
                }
            })
            .await
    }

    /// The workspace, or `None` when no package manager finds a project, or
    /// when they disagree about where it is (logged).
    pub async fn info(&self) -> Option<&WorkspaceInfo> {
        match self.lookup().await {
            Ok(info) => info.as_ref(),
            Err(e) => {
                tracing::warn!(error = %e, "no workspace");
                None
            }
        }
    }

    /// The workspace, or an error saying why there is none.
    pub async fn require_info(&self) -> Result<&WorkspaceInfo> {
        match self.lookup().await {
            Ok(Some(info)) => Ok(info),
            Ok(None) => {
                anyhow::bail!("not in a workspace: no package manager found a project here")
            }
            Err(e) => anyhow::bail!("{e}"),
        }
    }

    /// The workspace root, or `None` outside a workspace.
    pub async fn root(&self) -> Option<&std::path::Path> {
        self.info().await.map(|info| info.root.as_path())
    }

    /// Use `info` and `dep_ids` instead of asking the package managers: what
    /// they reported at a sync whose inputs have not changed since. Both, so
    /// that an event with nothing changed asks no package manager either
    /// question.
    pub fn reuse(&self, info: WorkspaceInfo, dep_ids: Vec<PackageId>) {
        let _ = self.info.set(Ok(Some(info)));
        let _ = self.dep_ids.set(dep_ids);
    }

    /// The instances an operation has failed on so far.
    pub fn failed(&self) -> Vec<String> {
        self.pms.failed()
    }

    /// The workspace's dependency set as package ids: every PM's `list_deps`
    /// unioned. This is what `depends-on` predicates evaluate against
    /// ([`crate::predicate::PredicateContext`]). A PM that fails contributes
    /// nothing ([`PmRegistry::list_deps`]).
    pub async fn dep_ids(&self) -> &[PackageId] {
        self.dep_ids.get_or_init(|| self.pms.list_deps()).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn package_id_display_is_colon_tuple() {
        let id = PackageId::new("cargo", "serde", "1.0.210");
        assert_eq!(id.to_string(), "cargo:serde:1.0.210");
    }

    #[test]
    fn authority_comes_from_core_construction_not_instance_names() {
        let configured_cargo = PmInstance::new(
            CARGO_PM,
            false,
            PluginKind::Package,
            Box::new(InvalidPluginPm),
        );
        let configured = configured_cargo
            .admission
            .admit_content(resolution(CARGO_PM, "configured"), Path::new("."))
            .unwrap();
        assert!(matches!(
            configured.package().naming_view().authority(),
            admission::PackageNamingAuthority::Remote
        ));

        let builtin = builtin_cargo_instance(std::sync::Arc::new(WorkspaceDeps::detached()));
        let admitted = builtin
            .admission
            .admit_content(resolution(CARGO_PM, "builtin"), Path::new("."))
            .unwrap();
        assert!(matches!(
            admitted.package().naming_view().authority(),
            admission::PackageNamingAuthority::BuiltinCargo
        ));
    }

    #[test]
    fn recommendations_authority_follows_the_core_source_binding() {
        let recommendations = PmInstance::new(
            "renamed-recommendations",
            true,
            PluginKind::Registry,
            Box::new(InvalidPluginPm),
        )
        .with_source(RegistrySource::Git {
            url: BUILTIN_RECOMMENDATIONS_URL.to_string(),
        });
        let admitted = recommendations
            .admission
            .admit_content(
                resolution("renamed-recommendations", "plugin"),
                Path::new("."),
            )
            .unwrap();
        assert!(matches!(
            admitted.package().naming_view().authority(),
            admission::PackageNamingAuthority::Recommendations
        ));

        let same_name = PmInstance::new(
            "symposium-recommendations",
            true,
            PluginKind::Registry,
            Box::new(InvalidPluginPm),
        )
        .with_source(RegistrySource::Path {
            dir: PathBuf::from(MISSING_DIR),
        });
        let admitted = same_name
            .admission
            .admit_content(
                resolution("symposium-recommendations", "plugin"),
                Path::new("."),
            )
            .unwrap();
        assert!(matches!(
            admitted.package().naming_view().authority(),
            admission::PackageNamingAuthority::Remote
        ));
    }

    /// A PM that answers with one plugin whose manifest fails validation.
    /// An absolute directory that does not exist, for a stub's plugin root. A
    /// leading `/` is not absolute on Windows, where a path needs a prefix, and
    /// a plugin whose root is not absolute is dropped.
    #[cfg(windows)]
    const MISSING_DIR: &str = r"C:\nonexistent";
    #[cfg(not(windows))]
    const MISSING_DIR: &str = "/nonexistent";

    fn resolution(pm: &str, name: &str) -> PackageResolution {
        PackageResolution::new(
            PackageId::any_version(pm, name),
            PackageSourceClaim::Unknown,
            PackageKey::new(format!("{pm}:{name}")).unwrap(),
        )
    }

    fn unvalidated_plugin(package: PackageResolution, name: &str) -> UnvalidatedPlugin {
        UnvalidatedPlugin::new(
            package,
            std::env::current_dir().unwrap(),
            symposium_sdk::manifest::RawPluginManifest::parse(&format!("name = \"{name}\""))
                .unwrap(),
        )
    }

    struct InvalidPluginPm;

    #[async_trait::async_trait]
    impl PackageManager for InvalidPluginPm {
        fn name(&self) -> &str {
            "stub"
        }
        async fn active_plugins(&self, _deps: &[PackageResolution]) -> Result<PluginListing> {
            let manifest = symposium_sdk::manifest::RawPluginManifest::parse(
                "name = \"broken\"\n[[skills]]\n",
            )
            .unwrap();
            Ok(PluginListing::complete(vec![UnvalidatedPlugin::new(
                resolution("stub", "broken"),
                std::env::current_dir().unwrap(),
                manifest,
            )]))
        }
        async fn load_plugin(
            &self,
            _target: &PackageLoadTarget,
            _update: UpdateLevel,
        ) -> Result<PluginListing> {
            Ok(PluginListing::complete(Vec::new()))
        }
    }

    /// A PM that answers every `load_plugin` with a plugin under `answers_as`,
    /// and lists one dependency under it too.
    struct EchoPm {
        name: &'static str,
        answers_as: &'static str,
        root: PathBuf,
    }

    struct ConflictingDepsPm;

    struct DuplicateDepsPm;

    struct InvalidatingDepsPm {
        list_calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        active_called: std::sync::Arc<AtomicBool>,
    }

    struct MismatchedExactLoadPm;

    struct MovingFetchRootPm {
        first: PathBuf,
        second: PathBuf,
        calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl PackageManager for ConflictingDepsPm {
        fn name(&self) -> &str {
            "cargo"
        }

        async fn load_plugin(
            &self,
            _target: &PackageLoadTarget,
            _update: UpdateLevel,
        ) -> Result<PluginListing> {
            Ok(PluginListing::complete(Vec::new()))
        }

        async fn list_deps(&self) -> Result<Vec<PackageResolution>> {
            let key = PackageKey::new("conflict").unwrap();
            Ok(vec![
                PackageResolution::new(
                    PackageId::new("cargo", "first", "1.0.0"),
                    PackageSourceClaim::CratesIo,
                    key.clone(),
                ),
                PackageResolution::new(
                    PackageId::new("cargo", "second", "1.0.0"),
                    PackageSourceClaim::CratesIo,
                    key,
                ),
            ])
        }
    }

    #[async_trait::async_trait]
    impl PackageManager for DuplicateDepsPm {
        fn name(&self) -> &str {
            "cargo"
        }

        async fn load_plugin(
            &self,
            _target: &PackageLoadTarget,
            _update: UpdateLevel,
        ) -> Result<PluginListing> {
            Ok(PluginListing::complete(Vec::new()))
        }

        async fn list_deps(&self) -> Result<Vec<PackageResolution>> {
            let package = PackageResolution::new(
                PackageId::new("cargo", "duplicate", "1.0.0"),
                PackageSourceClaim::CratesIo,
                PackageKey::new("duplicate").unwrap(),
            );
            Ok(vec![package.clone(), package])
        }
    }

    #[async_trait::async_trait]
    impl PackageManager for InvalidatingDepsPm {
        fn name(&self) -> &str {
            "cargo"
        }

        async fn active_plugins(&self, _deps: &[PackageResolution]) -> Result<PluginListing> {
            self.active_called.store(true, Ordering::Relaxed);
            Ok(PluginListing::complete(Vec::new()))
        }

        async fn load_plugin(
            &self,
            _target: &PackageLoadTarget,
            _update: UpdateLevel,
        ) -> Result<PluginListing> {
            Ok(PluginListing::complete(Vec::new()))
        }

        async fn list_deps(&self) -> Result<Vec<PackageResolution>> {
            let call = self.list_calls.fetch_add(1, Ordering::Relaxed);
            if call == 0 {
                return Ok(vec![PackageResolution::new(
                    PackageId::new("cargo", "valid", "1.0.0"),
                    PackageSourceClaim::CratesIo,
                    PackageKey::new("valid").unwrap(),
                )]);
            }

            let key = PackageKey::new("conflict").unwrap();
            Ok(vec![
                PackageResolution::new(
                    PackageId::new("cargo", "first", "1.0.0"),
                    PackageSourceClaim::CratesIo,
                    key.clone(),
                ),
                PackageResolution::new(
                    PackageId::new("cargo", "second", "1.0.0"),
                    PackageSourceClaim::CratesIo,
                    key,
                ),
            ])
        }
    }

    #[async_trait::async_trait]
    impl PackageManager for MismatchedExactLoadPm {
        fn name(&self) -> &str {
            "cargo"
        }

        async fn load_plugin(
            &self,
            target: &PackageLoadTarget,
            _update: UpdateLevel,
        ) -> Result<PluginListing> {
            assert!(matches!(target, PackageLoadTarget::Resolved { .. }));
            Ok(PluginListing::complete(vec![unvalidated_plugin(
                PackageResolution::new(
                    PackageId::new("cargo", "other", "1.0.0"),
                    PackageSourceClaim::CratesIo,
                    PackageKey::new("other-key").unwrap(),
                ),
                "other",
            )]))
        }

        async fn list_deps(&self) -> Result<Vec<PackageResolution>> {
            Ok(vec![PackageResolution::new(
                PackageId::new("cargo", "wanted", "1.0.0"),
                PackageSourceClaim::CratesIo,
                PackageKey::new("wanted-key").unwrap(),
            )])
        }
    }

    #[async_trait::async_trait]
    impl PackageManager for MovingFetchRootPm {
        fn name(&self) -> &str {
            "cargo"
        }

        async fn load_plugin(
            &self,
            _target: &PackageLoadTarget,
            _update: UpdateLevel,
        ) -> Result<PluginListing> {
            Ok(PluginListing::complete(Vec::new()))
        }

        async fn fetch(&self, id: &PackageId, _update: UpdateLevel) -> Result<FetchedPackage> {
            let root = if self.calls.fetch_add(1, Ordering::Relaxed) == 0 {
                self.first.clone()
            } else {
                self.second.clone()
            };
            Ok(FetchedPackage {
                package: PackageResolution::new(
                    id.clone(),
                    PackageSourceClaim::CratesIo,
                    PackageKey::new("stable-fetch-key").unwrap(),
                ),
                root,
            })
        }
    }

    struct AmbiguousDepsPm;

    struct RehydratingPm {
        list_calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        active_saw_exact: std::sync::Arc<AtomicBool>,
        load_saw_exact: std::sync::Arc<AtomicBool>,
    }

    #[async_trait::async_trait]
    impl PackageManager for AmbiguousDepsPm {
        fn name(&self) -> &str {
            "cargo"
        }

        async fn active_plugins(&self, deps: &[PackageResolution]) -> Result<PluginListing> {
            assert_eq!(deps.len(), 2);
            assert!(
                deps.iter()
                    .any(|dep| dep.source() == PackageSourceClaim::Workspace)
            );
            assert!(
                deps.iter()
                    .any(|dep| dep.source() == PackageSourceClaim::CratesIo)
            );
            Ok(PluginListing::complete(Vec::new()))
        }

        async fn load_plugin(
            &self,
            target: &PackageLoadTarget,
            _update: UpdateLevel,
        ) -> Result<PluginListing> {
            let PackageLoadTarget::Resolved { key } = target else {
                panic!("an admitted dependency must load by its exact key")
            };
            let (source, name) = match key.as_str() {
                "workspace:shared" => (PackageSourceClaim::Workspace, "workspace"),
                "registry:shared" => (PackageSourceClaim::CratesIo, "registry"),
                _ => panic!("unexpected package key"),
            };
            Ok(PluginListing::complete(vec![unvalidated_plugin(
                PackageResolution::new(
                    PackageId::new("cargo", "shared", "1.0.0"),
                    source,
                    key.clone(),
                ),
                name,
            )]))
        }

        async fn list_deps(&self) -> Result<Vec<PackageResolution>> {
            let id = PackageId::new("cargo", "shared", "1.0.0");
            Ok(vec![
                PackageResolution::new(
                    id.clone(),
                    PackageSourceClaim::Workspace,
                    PackageKey::new("workspace:shared").unwrap(),
                ),
                PackageResolution::new(
                    id,
                    PackageSourceClaim::CratesIo,
                    PackageKey::new("registry:shared").unwrap(),
                ),
            ])
        }
    }

    #[async_trait::async_trait]
    impl PackageManager for RehydratingPm {
        fn name(&self) -> &str {
            "cargo"
        }

        async fn active_plugins(&self, deps: &[PackageResolution]) -> Result<PluginListing> {
            self.active_saw_exact.store(
                deps.len() == 1 && deps[0].key().as_str() == "cargo:cached",
                Ordering::Relaxed,
            );
            Ok(PluginListing::complete(Vec::new()))
        }

        async fn load_plugin(
            &self,
            target: &PackageLoadTarget,
            _update: UpdateLevel,
        ) -> Result<PluginListing> {
            self.load_saw_exact.store(
                matches!(target, PackageLoadTarget::Resolved { key } if key.as_str() == "cargo:cached"),
                Ordering::Relaxed,
            );
            Ok(PluginListing::complete(Vec::new()))
        }

        async fn list_deps(&self) -> Result<Vec<PackageResolution>> {
            self.list_calls.fetch_add(1, Ordering::Relaxed);
            Ok(vec![PackageResolution::new(
                PackageId::new("cargo", "cached", "1.0.0"),
                PackageSourceClaim::CratesIo,
                PackageKey::new("cargo:cached").unwrap(),
            )])
        }
    }

    #[async_trait::async_trait]
    impl PackageManager for EchoPm {
        fn name(&self) -> &str {
            self.name
        }
        async fn load_plugin(
            &self,
            target: &PackageLoadTarget,
            _update: UpdateLevel,
        ) -> Result<PluginListing> {
            let name = match target {
                PackageLoadTarget::Coordinate { id } => id.name.as_str(),
                PackageLoadTarget::Resolved { .. } => "dep",
            };
            Ok(PluginListing::complete(vec![UnvalidatedPlugin::new(
                resolution(self.answers_as, name),
                self.root.clone(),
                symposium_sdk::manifest::RawPluginManifest::parse("").unwrap(),
            )]))
        }
        async fn list_deps(&self) -> Result<Vec<PackageResolution>> {
            Ok(vec![resolution(self.answers_as, "dep")])
        }
    }

    /// A PM that reports a fixed project, and fails to load any plugin.
    struct ProjectPm {
        name: &'static str,
        project: ProjectInfo,
    }

    #[async_trait::async_trait]
    impl PackageManager for ProjectPm {
        fn name(&self) -> &str {
            self.name
        }
        async fn load_plugin(
            &self,
            _target: &PackageLoadTarget,
            _update: UpdateLevel,
        ) -> Result<PluginListing> {
            anyhow::bail!("cannot load package")
        }
        async fn project_info(&self) -> Result<ProjectInfo> {
            Ok(self.project.clone())
        }
    }

    fn package_manager(name: &'static str, pm: impl PackageManager + 'static) -> PmInstance {
        PmInstance::new(name, false, PluginKind::Package, Box::new(pm))
    }

    fn workspace_of(start: &std::path::Path, pms: Vec<(&'static str, ProjectInfo)>) -> Workspace {
        Workspace::new(
            Some(start.to_path_buf()),
            PmRegistry::new(
                pms.into_iter()
                    .map(|(name, project)| package_manager(name, ProjectPm { name, project }))
                    .collect(),
            ),
        )
    }

    /// The workspace is the project its package managers report, which may be
    /// rooted above the starting directory. A PM that finds no project
    /// there leaves the root to the others; a local package outside the root
    /// is dropped, since its plugin would be trusted.
    #[tokio::test]
    async fn the_workspace_is_the_project_the_package_managers_report() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        let member = root.join("crates/app");
        let outside = tmp.path().join("elsewhere");
        std::fs::create_dir_all(&member).unwrap();
        std::fs::create_dir_all(&outside).unwrap();

        let ws = workspace_of(
            &member,
            vec![
                ("npm", ProjectInfo::default()),
                (
                    "cargo",
                    ProjectInfo {
                        root: Some(root.clone()),
                        local_packages: vec![member.clone(), outside],
                        inputs: vec![root.join("Cargo.lock")],
                    },
                ),
            ],
        );
        let info = ws.require_info().await.unwrap();
        assert_eq!(info.root, root);
        assert_eq!(info.members, vec![member]);
        assert_eq!(info.inputs, vec![root.join("Cargo.lock")]);
    }

    /// Two package managers rooting the project in different places is an
    /// error naming both, not a choice between them.
    #[tokio::test]
    async fn package_managers_that_disagree_about_the_root_are_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let web = tmp.path().join("web");
        std::fs::create_dir_all(&web).unwrap();
        let rooted_at = |dir: &std::path::Path| ProjectInfo {
            root: Some(dir.to_path_buf()),
            ..Default::default()
        };

        let ws = workspace_of(
            &web,
            vec![("cargo", rooted_at(tmp.path())), ("npm", rooted_at(&web))],
        );
        assert!(ws.info().await.is_none());
        let err = format!("{:#}", ws.require_info().await.unwrap_err());
        assert!(err.contains("`cargo`") && err.contains("`npm`"), "{err}");
    }

    /// Outside any project there is no workspace, and saying so is not an
    /// error from any package manager.
    #[tokio::test]
    async fn no_project_means_no_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = workspace_of(tmp.path(), vec![("cargo", ProjectInfo::default())]);
        assert!(ws.info().await.is_none());
        assert!(ws.failed().is_empty());
    }

    /// A relative path would resolve against wherever Symposium runs, so a
    /// package manager reporting one has failed, rather than been believed.
    #[tokio::test]
    async fn a_relative_path_in_a_project_is_a_failure() {
        let ws = workspace_of(
            std::path::Path::new("/"),
            vec![(
                "npm",
                ProjectInfo {
                    root: Some(PathBuf::from("web")),
                    ..Default::default()
                },
            )],
        );
        assert!(ws.info().await.is_none());
        assert_eq!(ws.failed(), vec!["npm"]);
    }

    /// One package failing to load says nothing about the rest of its
    /// ecosystem, so it is not recorded against the package manager.
    #[tokio::test]
    async fn a_failed_load_is_not_a_failed_package_manager() {
        let ws = workspace_of(
            std::path::Path::new("/"),
            vec![("npm", ProjectInfo::default())],
        );
        assert!(
            ws.pms()
                .load_plugin_request(
                    &PackageRequest::coordinate(PackageId::any_version("npm", "widget")),
                    UpdateLevel::None,
                )
                .await
                .is_empty()
        );
        assert!(ws.failed().is_empty());
    }

    fn echo(name: &'static str, answers_as: &'static str) -> PmInstance {
        package_manager(
            name,
            EchoPm {
                name,
                answers_as,
                root: std::env::current_dir().unwrap(),
            },
        )
    }

    /// A plugin root must be absolute: a relative one would resolve against
    /// wherever Symposium happens to run.
    #[tokio::test]
    async fn a_plugin_with_a_relative_root_is_dropped() {
        let pms = PmRegistry::new(vec![package_manager(
            "npm",
            EchoPm {
                name: "npm",
                answers_as: "npm",
                root: PathBuf::from("relative/dir"),
            },
        )]);
        assert!(
            pms.load_plugin_request(
                &PackageRequest::coordinate(PackageId::any_version("npm", "widget")),
                UpdateLevel::None,
            )
            .await
            .is_empty()
        );
    }

    #[tokio::test]
    async fn load_plugin_asks_only_the_pm_its_id_names() {
        let pms = PmRegistry::new(vec![echo("cargo", "cargo"), echo("npm", "npm")]);
        let loaded = pms
            .load_plugin_request(
                &PackageRequest::coordinate(PackageId::any_version("cargo", "foo")),
                UpdateLevel::None,
            )
            .await;
        assert!(loaded[0].resolved_package().is_some());
        assert_eq!(
            loaded[0].resolved_content().unwrap().root(),
            dunce::canonicalize(std::env::current_dir().unwrap()).unwrap()
        );
        let ids: Vec<_> = loaded.iter().map(|p| p.canonical.to_string()).collect();
        assert_eq!(ids, vec!["cargo:foo:*"]);
    }

    /// Consent and `disable` are keyed on the id, so a PM must not be able to
    /// answer as another: not with a plugin, and not with a dependency, which
    /// would satisfy another ecosystem's `depends-on` and enablement.
    #[tokio::test]
    async fn an_answer_under_another_pms_id_is_dropped() {
        let pms = PmRegistry::new(vec![echo("npm", "cargo")]);
        assert!(
            pms.load_plugin_request(
                &PackageRequest::coordinate(PackageId::any_version("npm", "foo")),
                UpdateLevel::None,
            )
            .await
            .is_empty()
        );
        assert!(pms.list_deps().await.is_empty());
        assert_eq!(
            pms.failed(),
            vec!["npm"],
            "an incomplete listing must prevent cleanup"
        );

        let pms = PmRegistry::new(vec![echo("npm", "cargo"), echo("cargo", "cargo")]);
        assert_eq!(
            pms.list_deps().await,
            vec![PackageId::any_version("cargo", "dep")]
        );
        assert_eq!(pms.failed(), vec!["npm"]);
    }

    #[tokio::test]
    async fn conflicting_dependency_keys_reject_the_complete_supplier_listing() {
        let pms = PmRegistry::new(vec![PmInstance::new(
            "cargo",
            false,
            PluginKind::Package,
            Box::new(ConflictingDepsPm),
        )]);

        assert!(pms.list_deps().await.is_empty());
        assert_eq!(pms.failed(), vec!["cargo"]);
    }

    #[tokio::test]
    async fn duplicate_dependency_keys_reject_the_complete_supplier_listing() {
        let pms = PmRegistry::new(vec![PmInstance::new(
            "cargo",
            false,
            PluginKind::Package,
            Box::new(DuplicateDepsPm),
        )]);

        assert!(pms.list_deps().await.is_empty());
        assert_eq!(pms.failed(), vec!["cargo"]);
    }

    #[tokio::test]
    async fn a_rejected_dependency_refresh_invalidates_the_previous_snapshot() {
        let list_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let active_called = std::sync::Arc::new(AtomicBool::new(false));
        let pms = PmRegistry::new(vec![PmInstance::new(
            "cargo",
            false,
            PluginKind::Package,
            Box::new(InvalidatingDepsPm {
                list_calls: list_calls.clone(),
                active_called: active_called.clone(),
            }),
        )]);

        let valid = pms.list_deps().await;
        assert_eq!(valid, vec![PackageId::new("cargo", "valid", "1.0.0")]);
        assert!(pms.list_deps().await.is_empty());

        assert!(
            pms.instances()
                .next()
                .unwrap()
                .active_plugins(&valid)
                .await
                .is_empty()
        );
        assert!(!active_called.load(Ordering::Relaxed));
        assert_eq!(list_calls.load(Ordering::Relaxed), 3);
    }

    #[tokio::test]
    async fn an_exact_load_rejects_a_plugin_from_another_package_key() {
        let pms = PmRegistry::new(vec![PmInstance::new(
            "cargo",
            false,
            PluginKind::Package,
            Box::new(MismatchedExactLoadPm),
        )]);
        let id = pms.list_deps().await.pop().unwrap();
        let request = pms
            .dependency_requests(std::slice::from_ref(&id))
            .await
            .pop()
            .unwrap();

        assert!(
            pms.load_plugin_request(&request, UpdateLevel::None)
                .await
                .is_empty()
        );
        assert_eq!(pms.failed(), vec!["cargo"]);
    }

    #[tokio::test]
    async fn fetch_cannot_move_an_admitted_key_to_another_content_root() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let pms = PmRegistry::new(vec![PmInstance::new(
            "cargo",
            false,
            PluginKind::Package,
            Box::new(MovingFetchRootPm {
                first: first.path().to_path_buf(),
                second: second.path().to_path_buf(),
                calls: AtomicUsize::new(0),
            }),
        )]);
        let id = PackageId::new("cargo", "stable", "1.0.0");

        pms.fetch(&id, UpdateLevel::None).await.unwrap();
        let error = pms.fetch(&id, UpdateLevel::None).await.err().unwrap();
        assert_eq!(
            error.to_string(),
            "package manager reused one package key for another content root"
        );
        assert!(
            !error
                .to_string()
                .contains(&first.path().display().to_string())
        );
        assert!(
            !error
                .to_string()
                .contains(&second.path().display().to_string())
        );
    }

    #[tokio::test]
    async fn equal_dependency_coordinates_retain_each_resolved_identity() {
        let pms = PmRegistry::new(vec![PmInstance::new(
            "cargo",
            false,
            PluginKind::Package,
            Box::new(AmbiguousDepsPm),
        )]);
        let deps = pms.list_deps().await;
        assert_eq!(deps.len(), 2);
        assert!(
            pms.instances()
                .next()
                .unwrap()
                .active_plugins(&deps)
                .await
                .is_empty()
        );

        let requests = pms.dependency_requests(&deps).await;
        assert_eq!(requests.len(), 2);
        assert!(
            requests
                .iter()
                .all(|request| matches!(request, PackageRequest::Resolved(_)))
        );
        let mut loaded = Vec::new();
        for request in &requests {
            let plugins = pms.load_plugin_request(request, UpdateLevel::None).await;
            assert_eq!(plugins.len(), 1);
            loaded.push(plugins[0].resolved_package().unwrap().handle());
        }
        assert!(loaded[0] != loaded[1]);
    }

    #[tokio::test]
    async fn cached_dependency_ids_rehydrate_exact_session_keys_once() {
        let list_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let active_saw_exact = std::sync::Arc::new(AtomicBool::new(false));
        let load_saw_exact = std::sync::Arc::new(AtomicBool::new(false));
        let instance = PmInstance::new(
            "cargo",
            false,
            PluginKind::Package,
            Box::new(RehydratingPm {
                list_calls: list_calls.clone(),
                active_saw_exact: active_saw_exact.clone(),
                load_saw_exact: load_saw_exact.clone(),
            }),
        );
        let cached_id = PackageId::new("cargo", "cached", "1.0.0");

        assert!(
            instance
                .try_active_plugins(std::slice::from_ref(&cached_id))
                .await
                .unwrap()
                .plugins
                .is_empty()
        );
        assert!(active_saw_exact.load(Ordering::Relaxed));

        let pms = PmRegistry::new(vec![instance]);
        let request = pms
            .dependency_requests(std::slice::from_ref(&cached_id))
            .await
            .pop()
            .unwrap();
        assert!(
            pms.load_plugin_request(&request, UpdateLevel::None)
                .await
                .is_empty()
        );
        assert!(load_saw_exact.load(Ordering::Relaxed));
        assert_eq!(list_calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn duplicate_plugin_identities_are_rejected_without_dropping_siblings() {
        let instance = PmInstance::new(
            "stub",
            false,
            PluginKind::Package,
            Box::new(InvalidPluginPm),
        );
        let duplicate = resolution("stub", "package");
        let listing = instance.validate_listing(PluginListing::complete(vec![
            unvalidated_plugin(duplicate.clone(), "duplicate"),
            unvalidated_plugin(duplicate, "duplicate"),
            unvalidated_plugin(resolution("stub", "sibling"), "sibling"),
        ]));

        assert_eq!(listing.plugins.len(), 1);
        assert_eq!(listing.plugins[0].manifest.name, "sibling");
        assert!(!listing.is_complete());
    }

    #[test]
    fn conflicting_plugin_keys_are_rejected_without_dropping_siblings() {
        let instance = PmInstance::new(
            "stub",
            false,
            PluginKind::Package,
            Box::new(InvalidPluginPm),
        );
        let key = PackageKey::new("conflicting").unwrap();
        let listing = instance.validate_listing(PluginListing::complete(vec![
            unvalidated_plugin(
                PackageResolution::new(
                    PackageId::any_version("stub", "first"),
                    PackageSourceClaim::Unknown,
                    key.clone(),
                ),
                "first",
            ),
            unvalidated_plugin(
                PackageResolution::new(
                    PackageId::any_version("stub", "second"),
                    PackageSourceClaim::Unknown,
                    key,
                ),
                "second",
            ),
            unvalidated_plugin(resolution("stub", "sibling"), "sibling"),
        ]));

        assert_eq!(listing.plugins.len(), 1);
        assert_eq!(listing.plugins[0].manifest.name, "sibling");
        assert!(!listing.is_complete());
    }

    /// The report events emitted while validating the stub's plugin under `kind`.
    async fn reports_for_invalid_plugin(kind: PluginKind) -> Vec<serde_json::Value> {
        use tracing_subscriber::layer::SubscriberExt;
        symposium_testlib::install_tracing_baseline();
        let (layer, handle) =
            crate::report::ReportLayer::new(crate::report::ReportMode::Json, tracing::Level::DEBUG);
        let _guard = tracing::subscriber::set_default(tracing_subscriber::registry().with(layer));
        let instance = PmInstance::new("stub", false, kind, Box::new(InvalidPluginPm));
        assert!(instance.active_plugins(&[]).await.is_empty());
        assert!(
            instance.has_failed(),
            "an incomplete listing must not authorize cleanup"
        );
        handle.drain()
    }

    #[tokio::test]
    async fn invalid_registry_plugin_is_reported_with_its_path() {
        let reports = reports_for_invalid_plugin(PluginKind::Registry).await;
        assert_eq!(reports.len(), 1, "{reports:?}");
        let message = reports[0]["message"].as_str().unwrap().replace('\\', "/");
        let root = std::env::current_dir()
            .unwrap()
            .display()
            .to_string()
            .replace('\\', "/");
        assert!(message.contains(&root), "{message}");
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
        let pm = GitPm::new("reg", "https://github.com/example/reg", ctx).unwrap();
        let err = pm.active_plugins(&[]).await.err().unwrap();
        assert!(
            err.to_string().contains("cargo agents plugin sync reg"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn each_package_manager_decides_which_names_match() {
        let tmp = tempfile::tempdir().unwrap();
        let pms = PmRegistry::new(vec![
            PmInstance::new(
                CARGO_PM,
                false,
                PluginKind::Package,
                Box::new(CargoPm::new(std::sync::Arc::new(WorkspaceDeps::detached()))),
            ),
            PmInstance::new(
                "reg",
                true,
                PluginKind::Registry,
                Box::new(PathPm::new("reg", tmp.path())),
            ),
        ]);
        assert_eq!(
            pms.canonical_name(CARGO_PM, "widget_lib").await.unwrap(),
            pms.canonical_name(CARGO_PM, "widget-lib").await.unwrap()
        );
        assert_ne!(
            pms.canonical_name("reg", "my_plugin").await.unwrap(),
            pms.canonical_name("reg", "my-plugin").await.unwrap()
        );
    }

    #[tokio::test]
    async fn registry_rejects_unknown_pm() {
        let tmp = tempfile::tempdir().unwrap();
        let _ = tmp;
        let id = PackageId::any_version("npm", "leftpad");
        let err = PmRegistry::new(vec![])
            .fetch(&id, UpdateLevel::None)
            .await
            .err()
            .unwrap();
        assert!(err.to_string().contains("unknown package manager `npm`"));
    }
}
