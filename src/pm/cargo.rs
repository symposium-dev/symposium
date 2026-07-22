//! The cargo package manager: crates from the active workspace's dependency
//! graph, resolved by [`RustCrateFetch`] (path-dependency override, then the
//! cargo registry cache, then crates.io).

use std::path::PathBuf;

use anyhow::Result;
use symposium_install::UpdateLevel;

use crate::crate_sources::RustCrateFetch;
use crate::plugins::ParsedPlugin;

use super::{
    ANY_VERSION, CARGO_PM, FetchedPackage, PackageId, PackageManager, PluginInfo, PmContext,
};

pub struct CargoPm;

impl CargoPm {
    /// Cargo id for a crate name and optional version requirement.
    pub fn id_for(name: &str, version: Option<&str>) -> PackageId {
        PackageId::new(CARGO_PM, name, version.unwrap_or(ANY_VERSION))
    }

    /// Resolve a crate to its plugin definition.
    ///
    /// Fetches the crate and builds a first-class [`ParsedPlugin`] from its
    /// manifest sources — `[package.metadata.symposium]` in `Cargo.toml` and a
    /// `SYMPOSIUM.toml` at the source root — layered over the crate defaults
    /// (see [`load_crate_manifest`](crate::plugins::load_crate_manifest)). The
    /// plugin is stamped with the resolved crate id as its
    /// [`canonical`](ParsedPlugin::canonical) identity (which keys chained-plugin
    /// cycle detection). A crate with no manifest sources still yields a plugin
    /// whose only content is the default `skills/` group.
    ///
    /// Returns `None` only when the crate can't be fetched or the merged
    /// manifest fails validation (both logged); the caller then contributes no
    /// skills for this reference.
    pub async fn load_plugin(&self, name: &str, cx: &PmContext<'_>) -> Option<ParsedPlugin> {
        let id = Self::id_for(name, None);
        let fetched = match self.fetch(&id, cx, UpdateLevel::None).await {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!(crate_name = %name, error = %e, "failed to fetch crate for plugin");
                return None;
            }
        };

        let metadata = crate::crate_metadata::symposium_metadata(&fetched.root.join("Cargo.toml"))
            .unwrap_or_else(|e| {
                tracing::warn!(
                    crate_name = %name,
                    error = %e,
                    "failed to read crate Cargo.toml; ignoring [package.metadata.symposium]"
                );
                None
            });

        let manifest_path = fetched.root.join("SYMPOSIUM.toml");
        let file = if manifest_path.is_file() {
            match std::fs::read_to_string(&manifest_path) {
                Ok(c) => Some(c),
                Err(e) => {
                    tracing::warn!(
                        path = %manifest_path.display(),
                        error = %e,
                        "failed to read crate SYMPOSIUM.toml"
                    );
                    None
                }
            }
        } else {
            None
        };

        let plugin = match crate::plugins::load_crate_manifest(
            metadata,
            file.as_deref(),
            &fetched.id.name,
        ) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(
                    crate_name = %name,
                    error = %e,
                    "failed to build crate plugin manifest"
                );
                return None;
            }
        };

        Some(ParsedPlugin {
            path: manifest_path,
            source_dir: fetched.root,
            plugin,
            workspace_member: false,
            canonical: fetched.id,
        })
    }
}

#[async_trait::async_trait]
impl PackageManager for CargoPm {
    fn name(&self) -> &'static str {
        CARGO_PM
    }

    /// Cargo surfaces dependency-matched plugins through `[[plugins]]` chained
    /// references today, not through this list; dependency discovery will fill
    /// it in.
    async fn list_plugins(
        &self,
        _deps: &[PackageId],
        _cx: &PmContext<'_>,
    ) -> Result<Vec<PluginInfo>> {
        Ok(Vec::new())
    }

    /// Searching crates.io lands with the `search` command.
    async fn search(&self, _query: &str, _cx: &PmContext<'_>) -> Result<Vec<PluginInfo>> {
        Ok(Vec::new())
    }

    async fn fetch(
        &self,
        id: &PackageId,
        cx: &PmContext<'_>,
        _update: UpdateLevel,
    ) -> Result<FetchedPackage> {
        debug_assert_eq!(id.pm, CARGO_PM);
        let mut fetch = RustCrateFetch::new(&id.name, cx.workspace_crates);
        if id.version != ANY_VERSION {
            fetch = fetch.version(&id.version);
        }
        let result = fetch.fetch().await?;
        Ok(FetchedPackage {
            id: PackageId::new(CARGO_PM, result.name, result.version),
            root: result.path,
        })
    }

    async fn list_deps(&self, cx: &PmContext<'_>) -> Result<Vec<PackageId>> {
        Ok(cx
            .workspace_crates
            .iter()
            .map(|c| PackageId::new(CARGO_PM, c.name.clone(), c.version.to_string()))
            .collect())
    }

    /// A crate's cache location depends on how it resolved (path override,
    /// registry cache, download), so it can't be answered from the id alone.
    fn cached_root(&self, _id: &PackageId, _cx: &PmContext<'_>) -> Option<PathBuf> {
        None
    }
}
