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
    FetchedPackage, PackageId, PackageManager, PathPm, PluginInfo, PluginListing, RegistrySource,
    UnvalidatedPlugin,
};
use crate::config::REGISTRY_CACHE_SUBDIR;

/// A git-backed registry: a repository cached on disk, read as a directory.
pub struct GitPm {
    name: String,
    git_url: String,
    /// Whether this registry participates in unforced auto-refresh.
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
            ctx,
            inner,
        })
    }

    /// Where this registry's content comes from, for `plugin list`.
    pub fn source(&self) -> RegistrySource {
        RegistrySource::Git {
            url: self.git_url.clone(),
        }
    }
}

#[async_trait::async_trait]
impl PackageManager for GitPm {
    fn name(&self) -> &str {
        &self.name
    }

    /// The registry's entries, once it has been fetched; until then it cannot
    /// say what it offers, which is not the same as offering nothing.
    async fn active_plugins(&self, deps: &[PackageId]) -> Result<PluginListing> {
        let name = &self.name;
        if !self.inner.dir.exists() {
            anyhow::bail!(
                "registry `{name}` has not been fetched yet; run `cargo agents plugin sync {name}`"
            );
        }
        self.inner.active_plugins(deps).await
    }

    async fn load_plugin(
        &self,
        id: &PackageId,
        update: UpdateLevel,
    ) -> Result<Vec<UnvalidatedPlugin>> {
        self.inner.load_plugin(id, update).await
    }

    async fn search(&self, query: &str) -> Result<Vec<PluginInfo>> {
        self.inner.search(query).await
    }

    async fn fetch(&self, id: &PackageId, update: UpdateLevel) -> Result<FetchedPackage> {
        self.inner.fetch(id, update).await
    }

    /// Pull the repository at `update`.
    async fn refresh(&self, update: UpdateLevel) -> Result<bool> {
        GitCacheManager::new(&self.ctx, REGISTRY_CACHE_SUBDIR)
            .fetch_url(&self.git_url, update)
            .await?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_resolves_a_cache_dir_and_reports_git_source() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = symposium_install::InstallContext::new(tmp.path().to_path_buf());
        let pm = GitPm::new(
            "symposium-recommendations",
            "https://github.com/symposium-dev/recommendations",
            ctx,
        )
        .expect("a well-formed URL resolves to a cache path");
        assert_eq!(pm.name(), "symposium-recommendations");
        match pm.source() {
            RegistrySource::Git { url } => {
                assert_eq!(url, "https://github.com/symposium-dev/recommendations");
            }
            RegistrySource::Path { .. } => panic!("expected a git registry source"),
        }
    }
}
