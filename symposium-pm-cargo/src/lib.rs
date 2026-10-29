//! The cargo package manager: crates from the active workspace's dependency
//! graph, resolved by [`RustCrateFetch`] (path-dependency override, then the
//! cargo registry cache, then crates.io).
//!
//! The [`workspace`] submodule owns the cargo-workspace resolution — the
//! `cargo metadata` invocation, its cache, and the [`WorkspaceCrate`] /
//! [`WorkspaceDeps`] types — since that is cargo's ecosystem, not a generic
//! concern. A [`CargoPm`] *holds* its [`WorkspaceDeps`] resolver (as an
//! [`Arc`], so several instances share one lazily-run, cached `cargo metadata`)
//! and drives it (`self.workspace.crates()`).
//!
//! Symposium calls it in its own process, or runs it as the
//! `symposium-pm-cargo` binary, which is
//! [`run`](symposium_sdk::pm::server::run) over this same type.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use symposium_sdk::pm::protocol::InitializeParams;
use symposium_sdk::pm::{
    ANY_VERSION, CARGO_PM, FetchedPackage, PackageId, PackageManager, PluginInfo, PluginListing,
    ProjectInfo, UnvalidatedPlugin, UpdateLevel,
};

use crate::sources::RustCrateFetch;

pub mod manifest;
pub mod metadata;
pub mod sources;
pub mod workspace;
pub use workspace::{
    LoadedWorkspace, WorkspaceCrate, WorkspaceDeps, file_mtime, workspace_dir_name,
};

/// What the cargo PM needs to resolve its ecosystem.
///
/// Out of process this is built from [`InitializeParams`]; in process
/// Symposium fills it from its own directories. Either way the PM holds it and
/// takes no ambient context per call.
#[derive(Debug, Clone, Default)]
pub struct CargoPmConfig {
    /// Where to cache the resolved workspace. `None` disables disk caching.
    pub cache_dir: Option<PathBuf>,
    /// The cargo binary to run, when not plain `cargo` (`SYMPOSIUM_CARGO`).
    pub cargo: Option<PathBuf>,
}

impl CargoPmConfig {
    /// The configuration carried in an `initialize` handshake, plus the cargo
    /// override from the process environment Symposium spawned this PM with.
    pub fn from_initialize(params: &InitializeParams) -> Self {
        Self {
            cache_dir: params.cache_dir.clone(),
            cargo: std::env::var_os("SYMPOSIUM_CARGO").map(PathBuf::from),
        }
    }
}

/// How many crates.io hits a search returns — enough to surface the crate a
/// user is looking for without flooding the report.
const SEARCH_PAGE_SIZE: u64 = 10;

/// The cargo transport, bound to one workspace's [`WorkspaceDeps`] resolver.
///
/// Holds the resolver as an [`Arc`] so several `CargoPm`s share one lazily-run,
/// cached `cargo metadata`.
pub struct CargoPm {
    workspace: Arc<WorkspaceDeps>,
}

impl CargoPm {
    /// A transport resolving against `workspace`.
    pub fn new(workspace: Arc<WorkspaceDeps>) -> Self {
        Self { workspace }
    }

    /// A transport resolving the workspace containing `cwd`.
    pub fn for_cwd(cwd: impl Into<PathBuf>, config: CargoPmConfig) -> Self {
        Self::new(Arc::new(WorkspaceDeps::new(cwd, config)))
    }

    /// The transport an `initialize` handshake describes: bound to its
    /// workspace, or to none.
    pub fn from_initialize(params: &InitializeParams) -> Self {
        let config = CargoPmConfig::from_initialize(params);
        match &params.workspace {
            Some(cwd) => Self::for_cwd(cwd.clone(), config),
            None => Self::new(Arc::new(WorkspaceDeps::detached())),
        }
    }

    /// Cargo id for a crate name and optional version requirement.
    pub fn id_for(name: &str, version: Option<&str>) -> PackageId {
        PackageId::new(CARGO_PM, name, version.unwrap_or(ANY_VERSION))
    }

    /// Build the [`UnvalidatedPlugin`] for an already-fetched crate, layering its
    /// manifest sources: `[package.metadata.symposium]` in `Cargo.toml` and a
    /// `Symposium.toml` at the root: via [`manifest::merge`].
    ///
    /// A crate with no manifest sources still yields a plugin: an empty
    /// manifest, which validation turns into a plugin whose only content is the
    /// default `skills/` group.
    fn plugin_from_fetched(&self, fetched: FetchedPackage) -> UnvalidatedPlugin {
        let name = &fetched.id.name;
        let metadata = crate::metadata::symposium_metadata(&fetched.root.join("Cargo.toml"))
            .unwrap_or_else(|e| {
                tracing::warn!(
                    crate_name = %name,
                    error = %e,
                    "failed to read crate Cargo.toml; ignoring [package.metadata.symposium]"
                );
                None
            });

        let manifest_path = fetched.root.join(symposium_sdk::manifest::MANIFEST_FILE);
        let file = if manifest_path.is_file() {
            match std::fs::read_to_string(&manifest_path) {
                Ok(c) => Some(c),
                Err(e) => {
                    tracing::warn!(
                        path = %manifest_path.display(),
                        error = %e,
                        "failed to read crate Symposium.toml"
                    );
                    None
                }
            }
        } else {
            None
        };

        let manifest = manifest::merge(metadata, file.as_deref(), name);
        UnvalidatedPlugin::new(fetched.id, fetched.root, manifest)
    }
}

/// What plugin content a crate source tree at `dir` embeds, as a short
/// human-readable phrase — or `None` when it embeds none. Mirrors what
/// [`CargoPm::load_plugin`] would build a plugin from: a `Symposium.toml`,
/// `[package.metadata.symposium]`, or the default `skills/` directory.
fn embedded_plugin_kind(dir: &std::path::Path) -> Option<&'static str> {
    if dir.join(symposium_sdk::manifest::MANIFEST_FILE).is_file() {
        return Some("plugin manifest (Symposium.toml)");
    }
    if matches!(
        crate::metadata::symposium_metadata(&dir.join("Cargo.toml")),
        Ok(Some(_))
    ) {
        return Some("embedded plugin ([package.metadata.symposium])");
    }
    contains_skill_md(&dir.join(symposium_sdk::manifest::DEFAULT_SKILLS_PATH))
        .then_some("embedded skills (skills/)")
}

/// Is there a `SKILL.md` anywhere under `dir`?
fn contains_skill_md(dir: &std::path::Path) -> bool {
    let mut ancestors = symposium_sdk::dir_walk::Ancestors::rooted_at(dir);
    contains_skill_md_in(dir, &mut ancestors)
}

fn contains_skill_md_in(
    dir: &std::path::Path,
    ancestors: &mut symposium_sdk::dir_walk::Ancestors,
) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        // `is_dir` follows symlinks, so a linked directory is searched — and
        // can name a directory this walk is already inside.
        if path.is_dir() {
            if !ancestors.enter(&path) {
                return false;
            }
            let found = contains_skill_md_in(&path, ancestors);
            ancestors.leave();
            found
        } else {
            path.file_name().is_some_and(|f| f == "SKILL.md")
        }
    })
}

#[async_trait::async_trait]
impl PackageManager for CargoPm {
    fn name(&self) -> &str {
        CARGO_PM
    }

    fn canonical_name(&self, name: &str) -> String {
        sources::normalize_crate_name(name)
    }

    /// The plugins embedded in the workspace's dependencies: every dep in
    /// `deps` whose source tree embeds plugin content, built into a full
    /// `Plugin`. Whether each is *trusted* (may activate without consent)
    /// is the caller's decision — the cargo transport is marked untrusted.
    ///
    /// Each dependency is fetched cache-only ([`UpdateLevel::None`]) to locate
    /// its source, then inspected. A workspace dependency resolves into the
    /// source `cargo metadata` already extracted — no probe, no network — so
    /// registry dependencies are surfaced exactly like path ones. A dependency
    /// whose source can't be served from cache is skipped.
    async fn active_plugins(&self, deps: &[PackageId]) -> Result<PluginListing> {
        let mut out = Vec::new();
        for id in deps.iter().filter(|id| id.pm == CARGO_PM) {
            // Fetch by name only: the concrete version in `id` would make
            // `fetch` treat it as an explicit `--version` and probe, bypassing
            // the workspace-source shortcut.
            let fetched = match self
                .fetch(&Self::id_for(&id.name, None), UpdateLevel::None)
                .await
            {
                Ok(f) => f,
                Err(e) => {
                    tracing::debug!(id = %id, error = %e, "cannot serve dependency source from cache; skipping");
                    continue;
                }
            };
            // Only surface dependencies that actually embed plugin content.
            if embedded_plugin_kind(&fetched.root).is_some() {
                out.push(self.plugin_from_fetched(fetched));
            }
        }
        Ok(PluginListing::complete(out))
    }

    /// Resolve a specific crate id to its plugin (a chained reference or an
    /// enabled crate). Unlike `active_plugins`, this loads the named crate
    /// whatever it embeds — any fetchable crate yields at least the default
    /// `skills/` plugin. Under [`UpdateLevel::None`] it is served from
    /// cargo's cache only.
    async fn load_plugin(
        &self,
        id: &PackageId,
        update: UpdateLevel,
    ) -> Result<Vec<UnvalidatedPlugin>> {
        let fetched = self
            .fetch(id, update)
            .await
            .with_context(|| format!("fetching `{id}` for its plugin"))?;
        Ok(vec![self.plugin_from_fetched(fetched)])
    }

    /// Search crates.io for crates matching `query`.
    ///
    /// Name-based, like `cargo search`: the results are *candidate* crates —
    /// whether one actually carries plugin content is only known once it is
    /// fetched (any fetchable crate yields at least a default `skills/` plugin).
    /// So this lets `cargo agents use <crate>` name a crate the workspace
    /// doesn't depend on; the fetch/load step decides what it contributes.
    async fn search(&self, query: &str) -> Result<Vec<PluginInfo>> {
        let client = crates_io_api::AsyncClient::new(
            "symposium (https://github.com/symposium-dev/symposium)",
            std::time::Duration::from_millis(1000),
        )?;
        let cq = crates_io_api::CratesQuery::builder()
            .search(query)
            .page_size(SEARCH_PAGE_SIZE)
            .build();
        let page = client.crates(cq).await?;
        Ok(page
            .crates
            .into_iter()
            .map(|c| PluginInfo {
                id: PackageId::new(CARGO_PM, c.name, c.max_version),
                description: c.description,
            })
            .collect())
    }

    async fn fetch(&self, id: &PackageId, update: UpdateLevel) -> Result<FetchedPackage> {
        debug_assert_eq!(id.pm, CARGO_PM);
        // `crates()` drives the lazy `cargo metadata` resolution — the cargo PM
        // owns the call, resolving against its own workspace.
        let mut fetch = RustCrateFetch::new(&id.name, self.workspace.crates())
            .offline(matches!(update, UpdateLevel::None));
        if id.version != ANY_VERSION {
            fetch = fetch.version(&id.version);
        }
        let result = fetch.fetch().await?;
        Ok(FetchedPackage {
            id: PackageId::new(CARGO_PM, result.name, result.version),
            root: result.path,
        })
    }

    /// Fails when `cargo metadata` failed, rather than answering that there
    /// are no dependencies.
    async fn list_deps(&self) -> Result<Vec<PackageId>> {
        let Some(ws) = self.workspace.try_load().map_err(anyhow::Error::msg)? else {
            return Ok(Vec::new());
        };
        Ok(ws
            .crates
            .iter()
            .map(|c| PackageId::new(CARGO_PM, c.name.clone(), c.version.to_string()))
            .collect())
    }

    /// The cargo workspace containing the directory: its root, its members,
    /// and the files that change when its dependencies do.
    async fn project_info(&self) -> Result<ProjectInfo> {
        let Some(ws) = self.workspace.try_load().map_err(anyhow::Error::msg)? else {
            return Ok(ProjectInfo::default());
        };
        let mut inputs = vec![
            ws.root.join("Cargo.lock"),
            ws.root.join("battery-pack.toml"),
        ];
        inputs.extend(root_candidate_manifests(self.workspace.cwd(), &ws.root));
        Ok(ProjectInfo {
            root: Some(ws.root.clone()),
            local_packages: ws
                .member_crates
                .iter()
                .filter_map(|member| member.path.clone())
                .collect(),
            inputs,
        })
    }
}

/// The manifests that decide where this workspace is rooted: one per directory
/// from the starting directory up to the root.
///
/// A manifest appearing nearer the starting directory than the root puts a new
/// workspace there, and one gaining a `[workspace]` table makes its own
/// directory the root. Neither shows up in a lockfile, so each path is reported
/// whether or not it exists: an input recorded absent and later found present
/// is a change, which is what makes a caller that cached the old root ask
/// again.
fn root_candidate_manifests(cwd: &Path, root: &Path) -> Vec<PathBuf> {
    let canonical = |dir: &Path| std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let root = canonical(root);
    let mut dir = canonical(cwd);
    let mut manifests = Vec::new();
    loop {
        manifests.push(dir.join("Cargo.toml"));
        if dir == root {
            break;
        }
        // Stop rather than walk past the root, so a starting directory the root
        // does not contain (an unresolvable symlink, say) yields one candidate
        // instead of every manifest up to the filesystem root.
        match dir.parent() {
            Some(parent) if parent.starts_with(&root) => dir = parent.to_path_buf(),
            _ => break,
        }
    }
    manifests
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::WorkspaceCrate;
    use std::path::PathBuf;

    /// The walk resolves both ends, so an expectation has to be canonical too:
    /// a temp dir sits behind a symlink on macOS (`/var` -> `/private/var`) and
    /// canonicalizing adds the `\\?\` prefix on Windows.
    fn canonical(dir: &Path) -> PathBuf {
        std::fs::canonicalize(dir).unwrap()
    }

    #[test]
    fn root_candidates_cover_every_directory_down_to_the_start() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();
        let root = canonical(tmp.path());
        assert_eq!(
            root_candidate_manifests(&nested, tmp.path()),
            vec![
                root.join("a").join("b").join("Cargo.toml"),
                root.join("a").join("Cargo.toml"),
                root.join("Cargo.toml"),
            ]
        );
    }

    #[test]
    fn the_root_itself_is_one_candidate() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(
            root_candidate_manifests(tmp.path(), tmp.path()),
            vec![canonical(tmp.path()).join("Cargo.toml")]
        );
    }

    /// A start the root does not contain stops at once instead of walking to
    /// the filesystem root.
    #[test]
    fn a_start_outside_the_root_yields_one_candidate() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tmp.path().join("outside");
        let root = tmp.path().join("root");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(
            root_candidate_manifests(&outside, &root),
            vec![canonical(&outside).join("Cargo.toml")]
        );
    }

    /// A path dependency: `source_dir` defaults to its local path.
    fn path_dep(name: &str, dir: PathBuf) -> WorkspaceCrate {
        WorkspaceCrate::new(name.to_string(), semver::Version::new(1, 0, 0), Some(dir))
    }

    /// A registry dependency whose extracted source `cargo metadata` located —
    /// no local `path`, but a known `source_dir` (as populated in production).
    fn registry_dep(name: &str, source_dir: PathBuf) -> WorkspaceCrate {
        WorkspaceCrate::new(name.to_string(), semver::Version::new(1, 0, 0), None)
            .with_source_dir(Some(source_dir))
    }

    #[tokio::test]
    async fn offers_dependencies_whose_sources_embed_plugin_content() {
        let tmp = tempfile::tempdir().unwrap();

        let with_skills = tmp.path().join("with-skills");
        std::fs::create_dir_all(with_skills.join("skills/guidance")).unwrap();
        std::fs::write(with_skills.join("skills/guidance/SKILL.md"), "").unwrap();

        // A *registry* dependency (no path) with an extracted source that
        // embeds a manifest — surfaced now that `active_plugins` fetches.
        let registry_embedded = tmp.path().join("registry-embedded");
        std::fs::create_dir_all(&registry_embedded).unwrap();
        std::fs::write(registry_embedded.join("Symposium.toml"), "").unwrap();

        let plain = tmp.path().join("plain");
        std::fs::create_dir_all(plain.join("src")).unwrap();

        let crates = vec![
            path_dep("with-skills", with_skills),
            registry_dep("registry-embedded", registry_embedded),
            path_dep("plain", plain),
        ];
        let deps: Vec<PackageId> = crates
            .iter()
            .map(|c| PackageId::new(CARGO_PM, &c.name, c.version.to_string()))
            .collect();
        let pm = CargoPm::new(WorkspaceDeps::fixture(tmp.path().to_path_buf(), crates));

        let active = pm.active_plugins(&deps).await.unwrap();
        let got: Vec<&str> = active.plugins.iter().map(|o| o.id.name.as_str()).collect();
        // `plain` embeds nothing, so it is not surfaced.
        assert_eq!(got, vec!["with-skills", "registry-embedded"]);
        assert!(active.plugins.iter().all(|o| o.id.pm == CARGO_PM));
        // The plugin points at the crate source; the default `skills/` group is
        // added by validation, not by the PM.
        assert!(active.plugins[0].root.join("skills").is_dir());
    }

    #[tokio::test]
    async fn load_plugin_without_update_reads_only_the_cargo_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let pm = CargoPm::new(crate::workspace::WorkspaceDeps::fixture(
            tmp.path().to_path_buf(),
            vec![],
        ));

        let loaded = pm
            .load_plugin(
                &CargoPm::id_for("zz-symposium-no-such-crate", None),
                UpdateLevel::None,
            )
            .await;
        assert!(loaded.is_err());
    }
}
