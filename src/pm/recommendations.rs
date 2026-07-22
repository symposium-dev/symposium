//! The recommendations package manager: a registry of plugins recommended
//! for *other* package managers' packages.
//!
//! A recommendations source is organized by PM namespace:
//!
//! ```text
//! cargo/serde/       — an entry recommending a plugin for the cargo
//!                      package `serde` (the entry's gate and default name)
//! symposium/…/       — the source's own unconditional entries, walked
//!                      like a flat source
//! ```
//!
//! The *subject* of a `cargo/<name>/` entry belongs to the cargo PM; the
//! entry's *content* is owned by this source. That distinction is what keeps
//! resolution from looping: a `[[plugins]] source.cargo` reference inside an
//! entry names a crate that the cargo PM resolves — never a resolve back into
//! this source.
//!
//! Ids look like `(<registry name>, cargo/serde, *)`: the `pm` component is
//! the configured registry's name (what its plugins are attributed to) and
//! the name component is the entry's namespaced path within the source. The
//! instance is constructed with an already-resolved content directory —
//! fetching the repository is
//! [`plugins::ensure_registries`](crate::plugins::ensure_registries)'s job.

use std::path::{Path, PathBuf};

use anyhow::Result;
use symposium_install::UpdateLevel;

use super::{
    ANY_VERSION, FetchedPackage, PackageId, PackageManager, PluginInfo, PmContext, layout,
};

/// A configured recommendations registry instance, over the source's
/// already-resolved content directory.
pub struct RecommendationsPm {
    name: String,
    dir: PathBuf,
}

impl RecommendationsPm {
    /// An instance named `name` over the recommendations content in `dir`.
    pub fn new(name: impl Into<String>, dir: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            dir: dir.into(),
        }
    }

    /// The entries this source offers: one per `cargo/<name>/` directory
    /// (recommending `<name>`) plus one per entry under the unconditional
    /// `symposium/` namespace. Unknown namespace directories are skipped
    /// (future PMs). A source that was never fetched offers nothing.
    fn offers(&self) -> Vec<PluginInfo> {
        if !self.dir.is_dir() {
            return Vec::new();
        }

        let mut entries = Vec::new();
        if let Ok(dirs) = std::fs::read_dir(self.dir.join("cargo")) {
            for entry in dirs.flatten() {
                let path = entry.path();
                if !path.is_dir() || layout::classify(&path).is_none() {
                    continue;
                }
                let Ok(dep_name) = entry.file_name().into_string() else {
                    continue;
                };
                entries.push(layout::RegistryEntry {
                    subpath: PathBuf::from("cargo").join(&dep_name),
                    recommends: Some(dep_name),
                });
            }
        }
        layout::walk(
            &self.dir.join("symposium"),
            Path::new("symposium"),
            &mut entries,
        );
        entries.sort_by(|a, b| a.subpath.cmp(&b.subpath));

        entries
            .into_iter()
            .map(|entry| PluginInfo {
                id: PackageId::new(&self.name, layout::subpath_key(&entry.subpath), ANY_VERSION),
                description: None,
                subpath: Some(entry.subpath),
                recommends: entry.recommends,
            })
            .collect()
    }
}

#[async_trait::async_trait]
impl PackageManager for RecommendationsPm {
    fn name(&self) -> &str {
        &self.name
    }

    /// `deps` is unused: dependency matching happens through the implied
    /// `depends-on(<name>)` gate each `cargo/<name>/` entry lowers to, so the
    /// offer list stays workspace-independent.
    async fn list_plugins(
        &self,
        _deps: &[PackageId],
        _cx: &PmContext<'_>,
    ) -> Result<Vec<PluginInfo>> {
        Ok(self.offers())
    }

    /// Substring match over the entries' namespaced names.
    async fn search(&self, query: &str, _cx: &PmContext<'_>) -> Result<Vec<PluginInfo>> {
        Ok(self
            .offers()
            .into_iter()
            .filter(|info| info.id.name.contains(query))
            .collect())
    }

    /// The content is already on disk; resolve the id's entry directory
    /// within it.
    async fn fetch(
        &self,
        id: &PackageId,
        _cx: &PmContext<'_>,
        _update: UpdateLevel,
    ) -> Result<FetchedPackage> {
        let dir = self.dir.join(&id.name);
        if !dir.is_dir() {
            anyhow::bail!("registry `{}` has no entry `{}`", self.name, id.name);
        }
        Ok(FetchedPackage {
            id: id.clone(),
            root: dir,
        })
    }

    /// A recommendations source contributes no workspace dependencies.
    async fn list_deps(&self, _cx: &PmContext<'_>) -> Result<Vec<PackageId>> {
        Ok(Vec::new())
    }

    /// The entry's directory within the source root.
    fn cached_root(&self, id: &PackageId, _cx: &PmContext<'_>) -> Option<PathBuf> {
        Some(self.dir.join(&id.name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use symposium_install::InstallContext;

    fn cx(cache: &Path) -> PmContext<'static> {
        PmContext {
            install: InstallContext::new(cache.to_path_buf()),
            workspace_crates: &[],
        }
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }

    #[tokio::test]
    async fn offers_namespaced_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("recs");
        touch(&source.join("cargo/widget-lib/SYMPOSIUM.toml"));
        touch(&source.join("cargo/other-lib/SKILL.md"));
        // Neither manifest nor skill: not an entry.
        std::fs::create_dir_all(source.join("cargo/empty-lib")).unwrap();
        touch(&source.join("symposium/tools/SYMPOSIUM.toml"));
        // Unknown namespaces are skipped.
        touch(&source.join("npm/leftpad/SYMPOSIUM.toml"));

        let pm = RecommendationsPm::new("symposium-recommendations", &source);
        let cx = cx(tmp.path());
        let offers = pm.list_plugins(&[], &cx).await.unwrap();
        let got: Vec<(&str, Option<&str>)> = offers
            .iter()
            .map(|o| (o.id.name.as_str(), o.recommends.as_deref()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("cargo/other-lib", Some("other-lib")),
                ("cargo/widget-lib", Some("widget-lib")),
                ("symposium/tools", None),
            ]
        );
        assert!(
            offers
                .iter()
                .all(|o| o.id.pm == "symposium-recommendations")
        );
        assert_eq!(
            pm.cached_root(&offers[1].id, &cx),
            Some(source.join("cargo/widget-lib"))
        );

        // A never-fetched (missing) source offers nothing.
        let missing = RecommendationsPm::new("symposium-recommendations", tmp.path().join("nope"));
        assert!(missing.list_plugins(&[], &cx).await.unwrap().is_empty());
    }
}
