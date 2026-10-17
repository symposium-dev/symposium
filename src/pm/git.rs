//! The git package manager: a registry instance backed by a git repository.
//!
//! A [`GitPm`] fronts one `[[registry]]` `git` entry. Once its repository is
//! fetched into the plugin-source cache it is just a directory, so the *reads*
//! ([`active_plugins`](PackageManager::active_plugins) / `load_plugin` /
//! `search` / `fetch`) delegate to an inner [`PathPm`] over that cache dir. The
//! git-specific part is [`refresh`](PackageManager::refresh): pulling the latest
//! content — which is exactly the operation a [`PathPm`] has no work to do for.

use anyhow::Result;
use symposium_install::UpdateLevel;
use symposium_install::git::GitCacheManager;

use super::{
    ConfiguredRegistrySource, FetchedPackage, PackageId, PackageManager, PackageProvenance, PathPm,
    PluginInfo, SourcedPackage, UnvalidatedPlugin,
};
use crate::config::REGISTRY_CACHE_SUBDIR;

/// A git-backed registry: a repository cached on disk, read as a directory.
pub struct GitPm {
    name: String,
    git_url: String,
    /// Whether this registry participates in unforced auto-refresh.
    auto_update: bool,
    /// Install context, used to build a [`GitCacheManager`] for refreshing.
    ctx: symposium_install::InstallContext,
    /// Reads delegate here — the cache directory the repository unpacks into.
    inner: PathPm,
}

impl GitPm {
    /// A git registry named `name` for `git_url`, cached under
    /// [`REGISTRY_CACHE_SUBDIR`]. `None` if the URL can't be resolved to a
    /// cache path (a malformed URL) — the caller skips it with a warning.
    pub fn new(
        name: impl Into<String>,
        git_url: impl Into<String>,
        auto_update: bool,
        ctx: symposium_install::InstallContext,
    ) -> Option<Self> {
        let name = name.into();
        let git_url = git_url.into();
        let content_dir =
            GitCacheManager::new(&ctx, REGISTRY_CACHE_SUBDIR).cache_path_for_url(&git_url)?;
        let inner = PathPm::new(name.clone(), content_dir);
        Some(Self {
            name,
            git_url,
            auto_update,
            ctx,
            inner,
        })
    }

    fn restamp(&self, package: &mut SourcedPackage) {
        package.provenance = PackageProvenance::Git {
            url: self.git_url.clone(),
        };
    }
}

#[async_trait::async_trait]
impl PackageManager for GitPm {
    fn name(&self) -> &str {
        &self.name
    }

    async fn active_plugins(&self, deps: &[SourcedPackage]) -> Vec<UnvalidatedPlugin> {
        let mut plugins = self.inner.active_plugins(deps).await;
        for plugin in &mut plugins {
            self.restamp(&mut plugin.package);
        }
        plugins
    }

    async fn load_plugin(&self, id: &PackageId) -> Vec<UnvalidatedPlugin> {
        let mut plugins = self.inner.load_plugin(id).await;
        for plugin in &mut plugins {
            self.restamp(&mut plugin.package);
        }
        plugins
    }

    async fn list_deps(&self) -> Result<Vec<SourcedPackage>> {
        let mut packages = self.inner.list_deps().await?;
        for package in &mut packages {
            self.restamp(package);
        }
        Ok(packages)
    }

    async fn search(&self, query: &str) -> Result<Vec<PluginInfo>> {
        let mut infos = self.inner.search(query).await?;
        for info in &mut infos {
            self.restamp(&mut info.package);
        }
        Ok(infos)
    }

    async fn fetch(&self, id: &PackageId, update: UpdateLevel) -> Result<FetchedPackage> {
        let mut fetched = self.inner.fetch(id, update).await?;
        self.restamp(&mut fetched.package);
        Ok(fetched)
    }

    /// Pull the repository. Skipped (returns `false`) when auto-update is off
    /// and the caller did not `force`; otherwise fetches at `update` and
    /// returns `true`.
    async fn refresh(&self, update: UpdateLevel, force: bool) -> Result<bool> {
        if !force && !self.auto_update {
            tracing::debug!(registry = %self.name, "skipping refresh (auto-update disabled)");
            return Ok(false);
        }
        GitCacheManager::new(&self.ctx, REGISTRY_CACHE_SUBDIR)
            .fetch_url(&self.git_url, update)
            .await?;
        Ok(true)
    }

    fn registry_source(&self) -> Option<ConfiguredRegistrySource> {
        Some(ConfiguredRegistrySource::Git {
            url: self.git_url.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_pm_with_entry() -> (tempfile::TempDir, GitPm, PackageId, PackageProvenance) {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = symposium_install::InstallContext::new(tmp.path().to_path_buf());
        let url = "https://github.com/symposium-dev/recommendations";
        let content_dir = GitCacheManager::new(&ctx, REGISTRY_CACHE_SUBDIR)
            .cache_path_for_url(url)
            .unwrap();
        std::fs::create_dir_all(content_dir.join("tools")).unwrap();
        std::fs::write(content_dir.join("tools/Symposium.toml"), "name = \"tools\"").unwrap();
        let pm = GitPm::new("symposium-recommendations", url, true, ctx).unwrap();
        let id = PackageId::any_version("symposium-recommendations", "tools");
        let provenance = PackageProvenance::Git {
            url: url.to_owned(),
        };
        (tmp, pm, id, provenance)
    }

    #[test]
    fn new_resolves_a_cache_dir_and_reports_git_source() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = symposium_install::InstallContext::new(tmp.path().to_path_buf());
        let pm = GitPm::new(
            "symposium-recommendations",
            "https://github.com/symposium-dev/recommendations",
            true,
            ctx,
        )
        .expect("a well-formed URL resolves to a cache path");
        assert_eq!(pm.name(), "symposium-recommendations");
        match pm.registry_source() {
            Some(ConfiguredRegistrySource::Git { url }) => {
                assert_eq!(url, "https://github.com/symposium-dev/recommendations");
            }
            _ => panic!("expected a git registry source"),
        }
    }

    #[tokio::test]
    async fn delegated_results_are_restamped_as_git() {
        let (_tmp, pm, id, provenance) = git_pm_with_entry();

        let active = pm.active_plugins(&[]).await;
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].package.provenance, provenance);

        let loaded = pm.load_plugin(&id).await;
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].package.provenance, provenance);

        assert!(pm.list_deps().await.unwrap().is_empty());

        let searched = pm.search("tool").await.unwrap();
        assert_eq!(searched.len(), 1);
        assert_eq!(searched[0].package.provenance, provenance);

        let fetched = pm.fetch(&id, UpdateLevel::None).await.unwrap();
        assert_eq!(fetched.package.provenance, provenance);
    }
}
