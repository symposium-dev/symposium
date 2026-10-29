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

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use symposium_install::UpdateLevel;

use crate::plugins::Plugin;

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

/// The identity types and the operation set are the SDK's, since they cross
/// the PM boundary: a package-manager binary speaks in them too.
pub use symposium_sdk::pm::{
    ANY_VERSION, CARGO_PM, FetchedPackage, PackageId, PackageManager, PluginInfo, PluginListing,
    ProjectInfo, UnvalidatedPlugin,
};

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
        Self {
            name: name.into(),
            trusted,
            kind,
            source: None,
            auto_update: true,
            pm,
            failed: AtomicBool::new(false),
        }
    }

    pub fn with_source(mut self, source: RegistrySource) -> Self {
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

    /// This instance's active plugins, validated. Each entry the PM could not
    /// read is reported, and a plugin that fails validation is reported and
    /// dropped; either leaves the listing with errors.
    pub async fn try_active_plugins(&self, deps: &[PackageId]) -> Result<PluginListing<Plugin>> {
        match self.pm.active_plugins(deps).await {
            Ok(listing) => {
                let total = listing.plugins.len();
                let mut errors = listing.errors;
                for e in &errors {
                    self.skipped(e);
                }
                let plugins = self.validate_all(listing.plugins);
                if plugins.len() < total {
                    errors.push(format!(
                        "{} of `{}`'s plugins were dropped",
                        total - plugins.len(),
                        self.name
                    ));
                }
                Ok(PluginListing { plugins, errors })
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
    pub async fn registry_plugins(&self) -> Option<PluginListing<Plugin>> {
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
    /// A failure is logged, not recorded against the instance: it says that
    /// one package could not be loaded (yanked, misspelled in a `use` entry,
    /// unreachable offline), not that the package manager's answers about the
    /// workspace are missing. Recording it would stop sync from removing any
    /// stale skill of the whole ecosystem for as long as that package fails.
    pub async fn load_plugin(&self, id: &PackageId, update: UpdateLevel) -> Vec<Plugin> {
        match self.pm.load_plugin(id, update).await {
            Ok(plugins) => self.validate_all(plugins),
            Err(e) => {
                tracing::warn!(instance = %self.name, id = %id, error = %format!("{e:#}"), "failed to load package");
                Vec::new()
            }
        }
    }

    /// The package this instance has under `name`, compared under its name
    /// rule: a dependency of that name; otherwise one already on disk (a
    /// cache-only fetch); otherwise, above [`UpdateLevel::None`], one its
    /// search turns up. A registry's packages are the plugins it lists, which
    /// callers hold already, so a trusted instance has none to find.
    ///
    /// Built from the operations every package manager has, so no package
    /// manager implements it and the protocol has no request for it.
    pub async fn find(&self, name: &str, update: UpdateLevel) -> Option<PackageId> {
        if self.trusted {
            return None;
        }
        let wanted = self.pm.canonical_name(name);
        let same =
            |id: &PackageId| id.pm == self.name && self.pm.canonical_name(&id.name) == wanted;
        if let Ok(deps) = self.pm.list_deps().await
            && let Some(dep) = deps.into_iter().find(|dep| same(dep))
        {
            return Some(dep);
        }
        let named = PackageId::any_version(&self.name, name);
        if let Ok(cached) = self.pm.fetch(&named, UpdateLevel::None).await
            && same(&cached.id)
        {
            return Some(cached.id);
        }
        if matches!(update, UpdateLevel::None) {
            return None;
        }
        let found = self.pm.search(name).await.ok()?;
        found.into_iter().map(|info| info.id).find(|id| same(id))
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
    fn validate_all(&self, unvalidated: Vec<UnvalidatedPlugin>) -> Vec<Plugin> {
        unvalidated
            .into_iter()
            .filter(|plugin| self.owns(&plugin.id))
            .filter(|plugin| {
                absolute(&self.name, &plugin.id, &plugin.root)
                    && plugin
                        .label_root
                        .as_ref()
                        .is_none_or(|dir| absolute(&self.name, &plugin.id, dir))
            })
            .filter_map(|plugin| {
                let id = plugin.id.clone();
                match crate::plugins::validate_plugin(plugin, self.kind) {
                    Ok(p) => Some(p),
                    Err(e) => {
                        self.skipped(&format!("`{id}`: {e:#}"));
                        None
                    }
                }
            })
            .collect()
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
}

impl PmRegistry {
    pub fn new(instances: Vec<PmInstance>) -> Self {
        Self { instances }
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

    /// `id` in the form two ids of the same package compare equal in, under
    /// its package manager's [`canonical_name`](PackageManager::canonical_name),
    /// with the version dropped.
    pub fn identity(&self, id: &PackageId) -> PackageId {
        let name = match self.owner(id) {
            Some(inst) => inst.pm.canonical_name(&id.name),
            None => id.name.clone(),
        };
        PackageId::any_version(&id.pm, name)
    }

    /// The identity of the package a user-typed `name` refers to. A plugin in
    /// `plugins` with that name under its registry's rule wins; otherwise the
    /// first package manager whose [`find`](PmInstance::find) has the name at
    /// `update` answers. A name no package manager has is an error.
    pub async fn identify(
        &self,
        plugins: &[Plugin],
        name: &str,
        update: UpdateLevel,
    ) -> Result<PackageId> {
        if let Some(plugin) = plugins.iter().find(|p| {
            self.identity(&PackageId::any_version(&p.canonical.pm, &p.manifest.name))
                == self.identity(&PackageId::any_version(&p.canonical.pm, name))
        }) {
            return Ok(self.identity(&plugin.canonical));
        }
        for inst in &self.instances {
            if let Some(id) = inst.find(name, update).await {
                return Ok(self.identity(&id));
            }
        }
        anyhow::bail!("no package named `{name}`")
    }

    /// Fetch a package via the instance named in its id.
    pub async fn fetch(&self, id: &PackageId, update: UpdateLevel) -> Result<FetchedPackage> {
        let inst = self.owner(id).ok_or_else(|| {
            anyhow::anyhow!("unknown package manager `{}` in package id `{id}`", id.pm)
        })?;
        let fetched = inst.pm.fetch(id, update).await?;
        anyhow::ensure!(
            inst.owns(&fetched.id),
            "`{}` answered a fetch of `{id}` with `{}`",
            inst.name,
            fetched.id
        );
        anyhow::ensure!(
            fetched.root.is_absolute(),
            "`{}` answered a fetch of `{id}` with a relative path, {}",
            inst.name,
            fetched.root.display()
        );
        Ok(fetched)
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
            match inst.pm.list_deps().await {
                Ok(found) => {
                    let total = found.len();
                    let owned: Vec<_> = found.into_iter().filter(|id| inst.owns(id)).collect();
                    if owned.len() != total {
                        inst.failure::<()>(
                            "list_deps",
                            anyhow::anyhow!(
                                "dependency listing contains another package manager's ids"
                            ),
                        )
                        .unwrap_or_default();
                    }
                    deps.extend(owned);
                }
                Err(e) => inst.failure::<()>("list_deps", e).unwrap_or_default(),
            }
        }
        deps
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
        match self.owner(id) {
            Some(inst) => inst.load_plugin(id, update).await,
            None => {
                tracing::debug!(id = %id, "no package manager owns this id");
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
            match inst.pm.search(query).await {
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
            PmRegistry::new(vec![PmInstance::new(
                CARGO_PM,
                false,
                PluginKind::Package,
                Box::new(CargoPm::new(deps)),
            )]),
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
        async fn active_plugins(&self, _deps: &[PackageId]) -> Result<PluginListing> {
            let manifest = symposium_sdk::manifest::RawPluginManifest::parse(
                "name = \"broken\"\n[[skills]]\n",
            )
            .unwrap();
            Ok(PluginListing::complete(vec![UnvalidatedPlugin::new(
                PackageId::any_version("stub", "broken"),
                PathBuf::from("/nonexistent/broken"),
                manifest,
            )]))
        }
        async fn load_plugin(
            &self,
            _id: &PackageId,
            _update: UpdateLevel,
        ) -> Result<Vec<UnvalidatedPlugin>> {
            Ok(Vec::new())
        }
    }

    /// A PM that answers every `load_plugin` with a plugin under `answers_as`,
    /// and lists one dependency under it too.
    struct EchoPm {
        name: &'static str,
        answers_as: &'static str,
        root: &'static str,
    }

    #[async_trait::async_trait]
    impl PackageManager for EchoPm {
        fn name(&self) -> &str {
            self.name
        }
        async fn load_plugin(
            &self,
            id: &PackageId,
            _update: UpdateLevel,
        ) -> Result<Vec<UnvalidatedPlugin>> {
            Ok(vec![UnvalidatedPlugin::new(
                PackageId::any_version(self.answers_as, &id.name),
                PathBuf::from(self.root),
                symposium_sdk::manifest::RawPluginManifest::parse("").unwrap(),
            )])
        }
        async fn list_deps(&self) -> Result<Vec<PackageId>> {
            Ok(vec![PackageId::any_version(self.answers_as, "dep")])
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
            id: &PackageId,
            _update: UpdateLevel,
        ) -> Result<Vec<UnvalidatedPlugin>> {
            anyhow::bail!("cannot load {id}")
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
                .load_plugin(&PackageId::any_version("npm", "widget"), UpdateLevel::None)
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
                root: "/nonexistent",
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
                root: "relative/dir",
            },
        )]);
        assert!(
            pms.load_plugin(&PackageId::any_version("npm", "widget"), UpdateLevel::None)
                .await
                .is_empty()
        );
    }

    #[tokio::test]
    async fn load_plugin_asks_only_the_pm_its_id_names() {
        let pms = PmRegistry::new(vec![echo("cargo", "cargo"), echo("npm", "npm")]);
        let loaded = pms
            .load_plugin(&PackageId::any_version("cargo", "foo"), UpdateLevel::None)
            .await;
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
            pms.load_plugin(&PackageId::any_version("npm", "foo"), UpdateLevel::None)
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

    /// The report events emitted while validating the stub's plugin under `kind`.
    async fn reports_for_invalid_plugin(kind: PluginKind) -> Vec<serde_json::Value> {
        use tracing_subscriber::layer::SubscriberExt;
        symposium_testlib::install_tracing_baseline();
        let (layer, handle) =
            crate::report::ReportLayer::new(crate::report::ReportMode::Json, tracing::Level::DEBUG);
        let _guard = tracing::subscriber::set_default(tracing_subscriber::registry().with(layer));
        let instance = PmInstance::new("stub", false, kind, Box::new(InvalidPluginPm));
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
        let pm = GitPm::new("reg", "https://github.com/example/reg", ctx).unwrap();
        let err = pm.active_plugins(&[]).await.unwrap_err();
        assert!(
            err.to_string().contains("cargo agents plugin sync reg"),
            "{err}"
        );
    }

    #[test]
    fn each_package_manager_decides_which_names_match() {
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
        let registry = PmInstance::new(
            "reg",
            true,
            PluginKind::Registry,
            Box::new(PathPm::new("reg", reg)),
        );
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
                WorkspaceCrate::new("widget-lib".into(), semver::Version::new(1, 0, 0), None),
                WorkspaceCrate::new("serde".into(), semver::Version::new(1, 0, 0), None),
            ],
        );
        let cargo = PmInstance::new(
            CARGO_PM,
            false,
            PluginKind::Package,
            Box::new(CargoPm::new(deps)),
        );
        let (pms, plugins) = registry_beside(cargo, &tmp.path().join("reg")).await;
        let registry_id = |name: &str| {
            let plugin = plugins.iter().find(|p| p.manifest.name == name).unwrap();
            pms.identity(&plugin.canonical)
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
