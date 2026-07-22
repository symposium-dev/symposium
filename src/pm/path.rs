//! The path package manager: a registry instance fronting one local
//! directory.
//!
//! This is the ordinary plugin-source case — `~/.symposium/plugins/`, a
//! `[[registry]]` entry with a `path`, or the git cache directory a
//! `[[registry]]` entry's repository was unpacked into (fetching is
//! [`plugins::ensure_registries`](crate::plugins::ensure_registries)'s job,
//! so once the content is on disk a git registry is just a directory).
//!
//! Ids look like `(<registry name>, <entry subpath>, *)`: the `pm` component
//! is the configured registry's name — the same name plugins from it are
//! attributed to — and the name component locates the entry within the
//! source. The instance resolves its own ids against its directory; nothing
//! routes them through the ecosystem transports.

use std::path::PathBuf;

use anyhow::Result;
use symposium_install::UpdateLevel;

use super::{
    ANY_VERSION, FetchedPackage, PackageId, PackageManager, PluginInfo, PmContext, layout,
};

/// A configured path registry: one local directory whose tree is a
/// collection of plugin entries.
pub struct PathPm {
    name: String,
    dir: PathBuf,
}

impl PathPm {
    /// An instance named `name` fronting the registry in `dir`.
    pub fn new(name: impl Into<String>, dir: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            dir: dir.into(),
        }
    }

    /// The entries this registry offers, as package infos.
    fn offers(&self) -> Result<Vec<PluginInfo>> {
        Ok(layout::enumerate(&self.dir)?
            .into_iter()
            .map(|entry| PluginInfo {
                id: PackageId::new(&self.name, layout::subpath_key(&entry.subpath), ANY_VERSION),
                description: None,
                subpath: Some(entry.subpath),
                recommends: entry.recommends,
            })
            .collect())
    }
}

#[async_trait::async_trait]
impl PackageManager for PathPm {
    fn name(&self) -> &str {
        &self.name
    }

    /// The registry's entries. `deps` is unused — a local registry's
    /// contents don't vary with the workspace.
    async fn list_plugins(
        &self,
        _deps: &[PackageId],
        _cx: &PmContext<'_>,
    ) -> Result<Vec<PluginInfo>> {
        self.offers()
    }

    /// Substring match over the entries' names. Manifest names are the
    /// plugin layer's to interpret, so this only sees directory names.
    async fn search(&self, query: &str, _cx: &PmContext<'_>) -> Result<Vec<PluginInfo>> {
        Ok(self
            .offers()?
            .into_iter()
            .filter(|info| info.id.name.contains(query))
            .collect())
    }

    /// Nothing to acquire — the content already lives in the registry
    /// directory. A missing entry directory is not an error here; discovery
    /// over the returned root decides what an empty entry means.
    async fn fetch(
        &self,
        id: &PackageId,
        cx: &PmContext<'_>,
        _update: UpdateLevel,
    ) -> Result<FetchedPackage> {
        let root = self
            .cached_root(id, cx)
            .ok_or_else(|| anyhow::anyhow!("registry `{}` cannot locate `{id}`", self.name))?;
        Ok(FetchedPackage {
            id: id.clone(),
            root,
        })
    }

    /// A local directory contributes no workspace dependencies.
    async fn list_deps(&self, _cx: &PmContext<'_>) -> Result<Vec<PackageId>> {
        Ok(Vec::new())
    }

    /// The entry's directory within the registry directory — path entries
    /// are their own cache.
    fn cached_root(&self, id: &PackageId, _cx: &PmContext<'_>) -> Option<PathBuf> {
        Some(self.dir.join(&id.name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use symposium_install::InstallContext;

    fn cx(cache: &std::path::Path) -> PmContext<'static> {
        PmContext {
            install: InstallContext::new(cache.to_path_buf()),
            workspace_crates: &[],
        }
    }

    #[tokio::test]
    async fn offers_one_package_per_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let cx = cx(tmp.path());
        let source = tmp.path().join("registry");
        std::fs::create_dir_all(source.join("tools")).unwrap();
        std::fs::write(source.join("tools/SYMPOSIUM.toml"), "name = \"tools\"").unwrap();
        std::fs::create_dir_all(source.join("nested/style")).unwrap();
        std::fs::write(source.join("nested/style/SKILL.md"), "# style").unwrap();

        let pm = PathPm::new("user-plugins", &source);
        let offers = pm.list_plugins(&[], &cx).await.unwrap();
        let names: Vec<&str> = offers.iter().map(|o| o.id.name.as_str()).collect();
        assert_eq!(names, vec!["nested/style", "tools"]);
        assert!(offers.iter().all(|o| o.id.pm == "user-plugins"));
        assert!(offers.iter().all(|o| o.recommends.is_none()));
        assert_eq!(
            offers[0].subpath.as_deref(),
            Some(std::path::Path::new("nested/style"))
        );
        assert_eq!(
            pm.cached_root(&offers[1].id, &cx),
            Some(source.join("tools"))
        );

        let hits = pm.search("too", &cx).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id.name, "tools");
    }

    #[tokio::test]
    async fn fetch_returns_the_local_entry_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let cx = cx(tmp.path());
        let pm = PathPm::new("local", tmp.path().join("registry"));
        let id = PackageId::new("local", "tools", ANY_VERSION);
        let fetched = pm.fetch(&id, &cx, UpdateLevel::None).await.unwrap();
        assert_eq!(fetched.root, tmp.path().join("registry/tools"));
        assert_eq!(fetched.id, id);
    }
}
