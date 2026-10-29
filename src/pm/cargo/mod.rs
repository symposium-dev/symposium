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
pub use workspace::{
    LoadedWorkspace, WorkspaceCrate, WorkspaceDeps, file_mtime, workspace_dir_name,
};

use super::layout::AGENT_PLUGIN_FILE;
use super::{
    ANY_VERSION, CARGO_PM, FetchedPackage, PackageId, PackageManager, PluginInfo, UnvalidatedPlugin,
};
use symposium_sdk::manifest::RawDefaults;

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

    fn plugin_from_fetched(&self, fetched: FetchedPackage) -> Option<UnvalidatedPlugin> {
        let name = &fetched.id.name;
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

        if metadata.is_none() && file.is_none() && fetched.root.join(AGENT_PLUGIN_FILE).is_file() {
            return match crate::plugins::agent_plugin_manifest(&fetched.root) {
                Ok(mut manifest) => {
                    manifest.defaults = Some(RawDefaults { skills: false });
                    Some(UnvalidatedPlugin {
                        agent_plugin: true,
                        ..UnvalidatedPlugin::new(fetched.id, fetched.root, manifest)
                    })
                }
                Err(e) => {
                    tracing::warn!(
                        report = %crate::report::ReportEvent::Warning {
                            message: format!("ignoring crate `{name}`: invalid `{AGENT_PLUGIN_FILE}`: {e:#}"),
                        },
                    );
                    None
                }
            };
        }

        let mut manifest = crate::plugins::merge_crate_manifest(metadata, file.as_deref(), name);
        crate::plugins::fill_from_agent_plugin(&mut manifest, &fetched.root);
        Some(UnvalidatedPlugin::new(fetched.id, fetched.root, manifest))
    }
}

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

    fn canonical_name(&self, name: &str) -> String {
        crate::crate_sources::normalize_crate_name(name)
    }

    /// A workspace dependency of that name, or a crate in cargo's extracted
    /// sources; above [`UpdateLevel::None`], otherwise a crate crates.io has.
    async fn find(&self, name: &str, update: UpdateLevel) -> Option<PackageId> {
        let wanted = self.canonical_name(name);
        let same = |id: &PackageId| self.canonical_name(&id.name) == wanted;
        if let Some(dep) = self.list_deps().await.ok()?.into_iter().find(same) {
            return Some(dep);
        }
        if let Ok(cached) = self
            .fetch(&Self::id_for(name, None), UpdateLevel::None)
            .await
        {
            return Some(cached.id);
        }
        if matches!(update, UpdateLevel::None) {
            return None;
        }
        let found = self.search(name).await.ok()?;
        found.into_iter().map(|info| info.id).find(same)
    }

    /// Each dependency is fetched cache-only ([`UpdateLevel::None`]) to locate
    /// its source, then inspected. A workspace dependency resolves into the
    /// source `cargo metadata` already extracted — no probe, no network — so
    /// registry dependencies are surfaced exactly like path ones. A dependency
    /// whose source can't be served from cache is skipped.
    async fn active_plugins(&self, deps: &[PackageId]) -> Vec<UnvalidatedPlugin> {
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
                out.extend(self.plugin_from_fetched(fetched));
            }
        }
        out
    }

    async fn load_plugin(&self, id: &PackageId, update: UpdateLevel) -> Vec<UnvalidatedPlugin> {
        let fetched = match self.fetch(id, update).await {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!(id = %id, error = %e, "failed to fetch crate for plugin");
                return Vec::new();
            }
        };
        self.plugin_from_fetched(fetched).into_iter().collect()
    }

    /// Search crates.io for crates matching `query`.
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

    async fn list_deps(&self) -> Result<Vec<PackageId>> {
        Ok(self
            .workspace
            .crates()
            .iter()
            .map(|c| PackageId::new(CARGO_PM, c.name.clone(), c.version.to_string()))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pm::WorkspaceCrate;
    use std::path::PathBuf;

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
        let pm = CargoPm::new(crate::pm::WorkspaceDeps::fixture(
            tmp.path().to_path_buf(),
            crates,
        ));

        let active = pm.active_plugins(&deps).await;
        let got: Vec<&str> = active.iter().map(|o| o.id.name.as_str()).collect();
        // `plain` embeds nothing, so it is not surfaced.
        assert_eq!(got, vec!["with-skills", "registry-embedded"]);
        assert!(active.iter().all(|o| o.id.pm == CARGO_PM));
        // The plugin points at the crate source; the default `skills/` group is
        // added by validation, not by the PM.
        assert!(active[0].root.join("skills").is_dir());
    }

    #[tokio::test]
    async fn load_plugin_without_update_reads_only_the_cargo_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let pm = CargoPm::new(crate::pm::WorkspaceDeps::fixture(
            tmp.path().to_path_buf(),
            vec![],
        ));

        let loaded = pm
            .load_plugin(
                &CargoPm::id_for("zz-symposium-no-such-crate", None),
                UpdateLevel::None,
            )
            .await;
        assert!(loaded.is_empty());
    }
}
