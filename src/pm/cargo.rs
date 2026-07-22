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

/// What plugin content a crate source tree at `dir` embeds, as a short
/// human-readable phrase — or `None` when it embeds none. Mirrors what
/// [`CargoPm::load_plugin`] would build a plugin from: a `SYMPOSIUM.toml`,
/// `[package.metadata.symposium]`, or the default `skills/` directory.
fn embedded_plugin_kind(dir: &std::path::Path) -> Option<&'static str> {
    if dir.join("SYMPOSIUM.toml").is_file() {
        return Some("plugin manifest (SYMPOSIUM.toml)");
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
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        if path.is_dir() {
            contains_skill_md(&path)
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

    /// Offer every workspace dependency whose source tree on disk embeds
    /// plugin content. Each offer `recommends` the dependency itself — a
    /// dependency-embedded plugin is a plugin *for* the crate carrying it —
    /// which is what [`discovery`](crate::discovery) matches against the
    /// workspace.
    ///
    /// Read-only by construction: only dependencies whose sources are already
    /// on disk (path dependencies) can be inspected without a fetch, so a
    /// registry dependency is never offered here. Enabling one still works —
    /// [`discovery::enabled_dependencies`](crate::discovery::enabled_dependencies)
    /// consults the config, not this list — it just isn't *discoverable*
    /// until its source has been fetched.
    ///
    /// Offers are consent-gated by the caller: the PM offers, the
    /// `[plugins]` config enables.
    async fn list_plugins(
        &self,
        _deps: &[PackageId],
        cx: &PmContext<'_>,
    ) -> Result<Vec<PluginInfo>> {
        Ok(cx
            .workspace_crates
            .iter()
            .filter_map(|wc| {
                let kind = embedded_plugin_kind(wc.path.as_deref()?)?;
                Some(PluginInfo {
                    id: PackageId::new(CARGO_PM, &wc.name, wc.version.to_string()),
                    description: Some(kind.to_string()),
                    subpath: None,
                    recommends: Some(wc.name.clone()),
                })
            })
            .collect())
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

#[cfg(test)]
mod tests {
    use super::*;
    use symposium_install::InstallContext;
    use symposium_sdk::workspace::WorkspaceCrate;

    fn dep(name: &str, path: Option<PathBuf>) -> WorkspaceCrate {
        WorkspaceCrate::new(name.to_string(), semver::Version::new(1, 0, 0), path)
    }

    #[tokio::test]
    async fn offers_dependencies_whose_sources_embed_plugin_content() {
        let tmp = tempfile::tempdir().unwrap();

        let with_skills = tmp.path().join("with-skills");
        std::fs::create_dir_all(with_skills.join("skills/guidance")).unwrap();
        std::fs::write(with_skills.join("skills/guidance/SKILL.md"), "").unwrap();

        let with_manifest = tmp.path().join("with-manifest");
        std::fs::create_dir_all(&with_manifest).unwrap();
        std::fs::write(with_manifest.join("SYMPOSIUM.toml"), "").unwrap();

        let plain = tmp.path().join("plain");
        std::fs::create_dir_all(plain.join("src")).unwrap();

        let crates = vec![
            dep("with-skills", Some(with_skills)),
            dep("with-manifest", Some(with_manifest)),
            dep("plain", Some(plain)),
            // A registry dependency: no source on disk to inspect.
            dep("serde", None),
        ];
        let cx = PmContext {
            install: InstallContext::new(tmp.path().to_path_buf()),
            workspace_crates: &crates,
        };

        let offers = CargoPm.list_plugins(&[], &cx).await.unwrap();
        let got: Vec<(&str, Option<&str>)> = offers
            .iter()
            .map(|o| (o.id.name.as_str(), o.recommends.as_deref()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("with-skills", Some("with-skills")),
                ("with-manifest", Some("with-manifest")),
            ]
        );
        assert!(offers.iter().all(|o| o.id.pm == CARGO_PM));
        assert!(
            offers[0]
                .description
                .as_deref()
                .unwrap()
                .contains("skills/")
        );
    }
}
