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

use std::sync::Arc;

use anyhow::Result;
use symposium_install::UpdateLevel;

use crate::crate_sources::RustCrateFetch;

pub mod workspace;
pub(crate) use workspace::CRATES_IO_REGISTRY_URL;
pub(crate) use workspace::classify_package_source;
pub use workspace::{
    LoadedWorkspace, WorkspaceCrate, WorkspaceDeps, file_mtime, workspace_dir_name,
};

use super::{
    ANY_VERSION, CARGO_PM, FetchedPackage, PackageId, PackageManager, PackageProvenance,
    PluginInfo, SourcedPackage, UnvalidatedPlugin,
};

/// How many crates.io hits a search returns — enough to surface the crate a
/// user is looking for without flooding the report.
const SEARCH_PAGE_SIZE: u64 = 10;

/// The cargo transport, bound to one workspace's [`WorkspaceDeps`] resolver.
///
/// Holds the resolver as an [`Arc`] so the transport in a [`PmRegistry`] and any
/// ad-hoc [`CargoPm`] built for crate loading share one lazily-run, cached
/// `cargo metadata` — the in-process stand-in for a per-workspace PM process.
pub struct CargoPm {
    workspace: Arc<WorkspaceDeps>,
}

impl CargoPm {
    /// A transport resolving against `workspace`.
    pub fn new(workspace: Arc<WorkspaceDeps>) -> Self {
        Self { workspace }
    }

    /// Cargo id for a crate name and optional version requirement.
    pub fn id_for(name: &str, version: Option<&str>) -> PackageId {
        PackageId::new(CARGO_PM, name, version.unwrap_or(ANY_VERSION))
    }

    /// A workspace member whose name and version satisfy `id`.
    fn matching_member<'a>(&'a self, id: &PackageId) -> Option<&'a WorkspaceCrate> {
        let requirement = semver::VersionReq::parse(&id.version).ok()?;
        let normalized = crate::crate_sources::normalize_crate_name(&id.name);
        self.workspace.member_crates().iter().find(|package| {
            crate::crate_sources::normalize_crate_name(&package.name) == normalized
                && requirement.matches(&package.version)
        })
    }

    /// A dependency record that names this exact sourced package.
    ///
    /// Looking it up as a complete sourced package matters when a workspace
    /// member and a dependency have the same Cargo name: a name-only fetch
    /// deliberately prefers the member, while dependency discovery must keep
    /// inspecting the dependency that `cargo metadata` reported.
    fn matching_dependency<'a>(&'a self, package: &SourcedPackage) -> Option<&'a WorkspaceCrate> {
        let normalized = crate::crate_sources::normalize_crate_name(&package.id.name);
        self.workspace.crates().iter().find(|candidate| {
            crate::crate_sources::normalize_crate_name(&candidate.name) == normalized
                && candidate.version.to_string() == package.id.version
                && candidate.provenance == package.provenance
        })
    }

    /// Build the [`UnvalidatedPlugin`] for an already-fetched crate, layering its
    /// manifest sources: `[package.metadata.symposium]` in `Cargo.toml` and a
    /// `Symposium.toml` at the root: via
    /// [`merge_crate_manifest`](crate::plugins::merge_crate_manifest).
    ///
    /// A crate with no manifest sources still yields a plugin: an empty
    /// manifest, which validation turns into a plugin whose only content is the
    /// default `skills/` group.
    fn plugin_from_fetched(&self, fetched: FetchedPackage) -> UnvalidatedPlugin {
        let name = &fetched.package.id.name;
        let metadata = crate::crate_metadata::symposium_metadata(&fetched.root.join("Cargo.toml"))
            .unwrap_or_else(|e| {
                tracing::warn!(
                    crate_name = %name,
                    error = %e,
                    "failed to read crate Cargo.toml; ignoring [package.metadata.symposium]"
                );
                None
            });

        let manifest_path = fetched.root.join(crate::pm::layout::MANIFEST_FILE);
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

        let manifest = crate::plugins::merge_crate_manifest(metadata, file.as_deref(), name);
        UnvalidatedPlugin::new(fetched.package, fetched.root, manifest)
    }
}

/// What plugin content a crate source tree at `dir` embeds, as a short
/// human-readable phrase — or `None` when it embeds none. Mirrors what
/// [`CargoPm::load_plugin`] would build a plugin from: a `Symposium.toml`,
/// `[package.metadata.symposium]`, or the default `skills/` directory.
fn embedded_plugin_kind(dir: &std::path::Path) -> Option<&'static str> {
    if dir.join(crate::pm::layout::MANIFEST_FILE).is_file() {
        return Some("plugin manifest (Symposium.toml)");
    }
    if matches!(
        crate::crate_metadata::symposium_metadata(&dir.join("Cargo.toml")),
        Ok(Some(_))
    ) {
        return Some("embedded plugin ([package.metadata.symposium])");
    }
    contains_skill_md(&dir.join(crate::plugins::CRATE_DEFAULT_SKILLS_PATH))
        .then_some("embedded skills (skills/)")
}

/// Is there a `SKILL.md` anywhere under `dir`?
fn contains_skill_md(dir: &std::path::Path) -> bool {
    let mut ancestors = crate::dir_walk::Ancestors::rooted_at(dir);
    contains_skill_md_in(dir, &mut ancestors)
}

fn contains_skill_md_in(dir: &std::path::Path, ancestors: &mut crate::dir_walk::Ancestors) -> bool {
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
    async fn active_plugins(&self, deps: &[SourcedPackage]) -> Vec<UnvalidatedPlugin> {
        let mut out = Vec::new();
        for package in deps.iter().filter(|package| package.id.pm == CARGO_PM) {
            let id = &package.id;
            // `deps` normally came from `list_deps`, so preserve that exact
            // record instead of resolving its name again. The fallback keeps
            // the trait method useful to callers that supply another Cargo id.
            let fetched = if let Some(candidate) = self.matching_dependency(package) {
                let Some(root) = candidate.source_dir.clone() else {
                    tracing::debug!(id = %id, "dependency has no source directory; skipping");
                    continue;
                };
                FetchedPackage {
                    package: SourcedPackage::new(
                        PackageId::new(
                            CARGO_PM,
                            candidate.name.clone(),
                            candidate.version.to_string(),
                        ),
                        candidate.provenance.clone(),
                    ),
                    root,
                }
            } else {
                match self
                    .fetch(&Self::id_for(&id.name, None), UpdateLevel::None)
                    .await
                {
                    Ok(f) => f,
                    Err(e) => {
                        tracing::debug!(id = %id, error = %e, "cannot serve dependency source from cache; skipping");
                        continue;
                    }
                }
            };
            // Only surface dependencies that actually embed plugin content.
            if embedded_plugin_kind(&fetched.root).is_some() {
                out.push(self.plugin_from_fetched(fetched));
            }
        }
        out
    }

    /// Resolve a specific crate id to its plugin (a chained reference or an
    /// enabled crate). Unlike `active_plugins`, this loads the named crate
    /// whatever it embeds — any fetchable crate yields at least the default
    /// `skills/` plugin. Fetched cache-only.
    async fn load_plugin(&self, id: &PackageId) -> Vec<UnvalidatedPlugin> {
        let fetched = match self.fetch(id, UpdateLevel::None).await {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!(id = %id, error = %e, "failed to fetch crate for plugin");
                return Vec::new();
            }
        };
        vec![self.plugin_from_fetched(fetched)]
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
                package: SourcedPackage::new(
                    PackageId::new(CARGO_PM, c.name, c.max_version),
                    PackageProvenance::Registry {
                        url: CRATES_IO_REGISTRY_URL.to_owned(),
                    },
                ),
                description: c.description,
            })
            .collect())
    }

    async fn fetch(&self, id: &PackageId, _update: UpdateLevel) -> Result<FetchedPackage> {
        debug_assert_eq!(id.pm, CARGO_PM);

        // Members are not dependencies, so resolve a matching member before
        // the ordinary workspace-source lookup. A version that the member does
        // not satisfy continues through normal package resolution.
        if let Some(member) = self.matching_member(id) {
            let root = member.source_dir.as_ref().ok_or_else(|| {
                anyhow::anyhow!("workspace member `{}` has no source directory", member.name)
            })?;
            return Ok(FetchedPackage {
                package: SourcedPackage::new(
                    PackageId::new(CARGO_PM, member.name.clone(), member.version.to_string()),
                    member.provenance.clone(),
                ),
                root: root.clone(),
            });
        }

        // `crates()` drives the lazy `cargo metadata` resolution — the cargo PM
        // owns the call, resolving against its own workspace.
        let workspace = self.workspace.crates();
        let mut fetch = RustCrateFetch::new(&id.name, workspace);
        if id.version != ANY_VERSION {
            fetch = fetch.version(&id.version);
        }
        let result = fetch.fetch().await?;
        Ok(FetchedPackage {
            package: SourcedPackage::new(
                PackageId::new(CARGO_PM, result.name, result.version),
                result.provenance,
            ),
            root: result.path,
        })
    }

    async fn list_deps(&self) -> Result<Vec<SourcedPackage>> {
        Ok(self
            .workspace
            .crates()
            .iter()
            .map(|c| {
                SourcedPackage::new(
                    PackageId::new(CARGO_PM, c.name.clone(), c.version.to_string()),
                    c.provenance.clone(),
                )
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pm::{PackageProvenance, WorkspaceCrate};
    use std::path::PathBuf;

    /// A path dependency: `source_dir` defaults to its local path.
    fn path_dep(name: &str, dir: PathBuf) -> WorkspaceCrate {
        WorkspaceCrate::new(
            name.to_string(),
            semver::Version::new(1, 0, 0),
            PackageProvenance::Path,
            Some(dir),
        )
    }

    /// A registry dependency whose extracted source `cargo metadata` located —
    /// no local `path`, but a known `source_dir` (as populated in production).
    fn registry_dep(name: &str, source_dir: PathBuf) -> WorkspaceCrate {
        WorkspaceCrate::new(
            name.to_string(),
            semver::Version::new(1, 0, 0),
            PackageProvenance::Registry {
                url: workspace::CRATES_IO_REGISTRY_URL.to_owned(),
            },
            None,
        )
        .with_source_dir(Some(source_dir))
    }

    fn git_dep(name: &str, source_dir: PathBuf) -> WorkspaceCrate {
        WorkspaceCrate::new(
            name.to_string(),
            semver::Version::new(1, 0, 0),
            PackageProvenance::Git {
                url: "https://git.example/dependency".to_owned(),
            },
            None,
        )
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

        let git_embedded = tmp.path().join("git-embedded");
        std::fs::create_dir_all(&git_embedded).unwrap();
        std::fs::write(git_embedded.join("Symposium.toml"), "").unwrap();

        let plain = tmp.path().join("plain");
        std::fs::create_dir_all(plain.join("src")).unwrap();

        let crates = vec![
            path_dep("with-skills", with_skills),
            registry_dep("registry-embedded", registry_embedded),
            git_dep("git-embedded", git_embedded),
            path_dep("plain", plain),
        ];
        let pm = CargoPm::new(crate::pm::WorkspaceDeps::fixture(
            tmp.path().to_path_buf(),
            crates,
        ));

        let deps = pm.list_deps().await.unwrap();
        let active = pm.active_plugins(&deps).await;
        let got: Vec<&str> = active
            .iter()
            .map(|unplug| unplug.package.id.name.as_str())
            .collect();
        // `plain` embeds nothing, so it is not surfaced.
        assert_eq!(
            got,
            vec!["with-skills", "registry-embedded", "git-embedded"]
        );
        assert!(active.iter().all(|unplug| unplug.package.id.pm == CARGO_PM));
        assert_eq!(active[0].package.provenance, PackageProvenance::Path);
        assert_eq!(
            active[1].package.provenance,
            PackageProvenance::Registry {
                url: workspace::CRATES_IO_REGISTRY_URL.to_owned(),
            }
        );
        assert_eq!(
            active[2].package.provenance,
            PackageProvenance::Git {
                url: "https://git.example/dependency".to_owned(),
            }
        );

        for expected in &deps {
            let loaded = pm
                .load_plugin(&PackageId::any_version(CARGO_PM, &expected.id.name))
                .await;
            assert_eq!(loaded.len(), 1);
            assert_eq!(loaded[0].package.provenance, expected.provenance);
        }
        // The plugin points at the crate source; the default `skills/` group is
        // added by validation, not by the PM.
        assert!(active[0].root.join("skills").is_dir());
    }

    #[tokio::test]
    async fn explicit_workspace_member_keeps_workspace_provenance() {
        let tmp = tempfile::tempdir().unwrap();
        let member_dir = tmp.path().join("member");
        std::fs::create_dir_all(member_dir.join("skills/guidance")).unwrap();
        std::fs::write(member_dir.join("skills/guidance/SKILL.md"), "").unwrap();

        let member = WorkspaceCrate::new(
            "member".to_owned(),
            semver::Version::new(0, 1, 0),
            PackageProvenance::Workspace,
            Some(member_dir.clone()),
        );
        let pm = CargoPm::new(crate::pm::WorkspaceDeps::fixture_with_members(
            tmp.path(),
            Vec::new(),
            vec![member],
        ));

        assert!(pm.list_deps().await.unwrap().is_empty());
        for requested in [ANY_VERSION, "0.1.0"] {
            let loaded = pm
                .load_plugin(&PackageId::new(CARGO_PM, "member", requested))
                .await;
            assert_eq!(loaded.len(), 1);
            assert_eq!(loaded[0].package.id.version, "0.1.0");
            assert_eq!(loaded[0].package.provenance, PackageProvenance::Workspace);
            assert_eq!(loaded[0].root, member_dir);
        }
        assert!(
            pm.matching_member(&PackageId::new(CARGO_PM, "member", "2.0.0"))
                .is_none()
        );
    }

    #[tokio::test]
    async fn workspace_member_without_a_source_directory_fails_closed() {
        let tmp = tempfile::tempdir().unwrap();
        let member = WorkspaceCrate::new(
            "member".to_owned(),
            semver::Version::new(0, 1, 0),
            PackageProvenance::Workspace,
            None,
        );
        let pm = CargoPm::new(crate::pm::WorkspaceDeps::fixture_with_members(
            tmp.path(),
            Vec::new(),
            vec![member],
        ));

        let error = pm
            .fetch(
                &PackageId::any_version(CARGO_PM, "member"),
                UpdateLevel::None,
            )
            .await
            .expect_err("a known member must not fall through to crates.io");
        assert!(error.to_string().contains("has no source directory"));
    }

    #[tokio::test]
    async fn same_named_dependency_is_not_replaced_by_a_workspace_member() {
        let tmp = tempfile::tempdir().unwrap();
        let dependency_dir = tmp.path().join("dependency");
        let member_dir = tmp.path().join("member");
        std::fs::create_dir_all(&dependency_dir).unwrap();
        std::fs::create_dir_all(&member_dir).unwrap();
        std::fs::write(dependency_dir.join("Symposium.toml"), "").unwrap();
        std::fs::write(member_dir.join("Symposium.toml"), "").unwrap();

        let dependency = registry_dep("shared-name", dependency_dir.clone());
        let member = WorkspaceCrate::new(
            "shared-name".to_owned(),
            semver::Version::new(1, 0, 0),
            PackageProvenance::Workspace,
            Some(member_dir),
        );
        let pm = CargoPm::new(crate::pm::WorkspaceDeps::fixture_with_members(
            tmp.path(),
            vec![dependency],
            vec![member],
        ));

        let deps = pm.list_deps().await.unwrap();
        let active = pm.active_plugins(&deps).await;

        assert_eq!(active.len(), 1);
        assert_eq!(active[0].root, dependency_dir);
        assert_eq!(active[0].package, deps[0]);
    }
}
