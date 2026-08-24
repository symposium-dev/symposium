//! The path package manager: a registry instance fronting one local
//! directory.
//!
//! This is the ordinary plugin-source case — `~/.symposium/plugins/`, a
//! `[[registry]]` entry with a `path`, or the git cache directory a
//! [`GitPm`](crate::pm::GitPm) fetched into (a `GitPm` delegates its reads to
//! an inner `PathPm` over that cache dir, so once the content is on disk a git
//! registry is just a directory).
//!
//! Ids look like `(<registry name>, <entry subpath>, *)`: the `pm` component
//! is the configured registry's name — the same name plugins from it are
//! attributed to — and the name component locates the entry within the
//! source. The instance resolves its own ids against its directory.

use std::path::{Path, PathBuf};

use anyhow::Result;
use symposium_install::UpdateLevel;

use super::{
    ANY_VERSION, FetchedPackage, PackageId, PackageManager, PluginInfo, RegistrySource, layout,
};
use crate::plugins::ParsedPlugin;
use crate::report::ReportEvent;

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
}

#[async_trait::async_trait]
impl PackageManager for PathPm {
    fn name(&self) -> &str {
        &self.name
    }

    /// Every entry in the registry, loaded as a plugin. `deps` is ignored — a
    /// local registry's contents don't vary with the workspace; whether each
    /// plugin applies is decided later by its own predicates. A registry is a
    /// trust root, so these activate without consent.
    async fn active_plugins(&self, _deps: &[PackageId]) -> Vec<ParsedPlugin> {
        let entries = match layout::enumerate(&self.dir) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(registry = %self.name, error = %e, "cannot list registry");
                return Vec::new();
            }
        };
        let mut out = Vec::new();
        for entry in entries {
            match crate::plugins::load_entry(&self.dir, &entry.subpath, &self.name) {
                Some(Ok(p)) => out.push(p),
                Some(Err(e)) => tracing::warn!(
                    report = %ReportEvent::Warning {
                        message: format!(
                            "skipping {}: {e:#}",
                            crate::output::display_path(&self.dir.join(&entry.subpath))
                        ),
                    },
                ),
                None => {}
            }
        }
        out
    }

    /// The entry an id names (`id.name` is the entry's subpath key).
    async fn load_plugin(&self, id: &PackageId) -> Vec<ParsedPlugin> {
        if id.pm != self.name {
            return Vec::new();
        }
        match crate::plugins::load_entry(&self.dir, Path::new(&id.name), &self.name) {
            Some(Ok(p)) => vec![p],
            Some(Err(e)) => {
                tracing::warn!(registry = %self.name, id = %id, error = %e, "failed to load plugin");
                Vec::new()
            }
            None => Vec::new(),
        }
    }

    /// A local directory contributes no workspace dependencies.
    async fn list_deps(&self) -> Result<Vec<PackageId>> {
        Ok(Vec::new())
    }

    /// Substring match over the entries' subpath keys. Manifest names are the
    /// plugin layer's to interpret, so this only sees directory names.
    async fn search(&self, query: &str) -> Result<Vec<PluginInfo>> {
        let entries = layout::enumerate(&self.dir).unwrap_or_default();
        Ok(entries
            .into_iter()
            .map(|entry| layout::subpath_key(&entry.subpath))
            .filter(|key| key.contains(query))
            .map(|key| PluginInfo::from_id(PackageId::new(&self.name, key, ANY_VERSION)))
            .collect())
    }

    /// The entry directory an id names — path entries are their own cache.
    async fn fetch(&self, id: &PackageId, _update: UpdateLevel) -> Result<FetchedPackage> {
        Ok(FetchedPackage {
            id: id.clone(),
            root: self.dir.join(&id.name),
        })
    }

    fn registry_source(&self) -> Option<RegistrySource> {
        Some(RegistrySource::Path {
            dir: self.dir.clone(),
        })
    }

    async fn source_readable(&self) -> bool {
        !self.dir.exists() || std::fs::read_dir(&self.dir).is_ok()
    }
}

#[cfg(test)]
mod tests {
    /// An absent directory is an empty registry, not an unreadable one. Treating
    /// it as a failure would stop a fresh install from ever reaping anything.
    #[tokio::test]
    async fn an_absent_directory_is_readable() {
        let dir = tempfile::tempdir().unwrap();
        let pm = PathPm::new("test", dir.path().join("nope"));
        assert!(pm.source_readable().await);
        let pm = PathPm::new("test", dir.path().to_path_buf());
        assert!(pm.source_readable().await);
    }

    use super::*;

    #[tokio::test]
    async fn loads_one_plugin_per_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("registry");
        std::fs::create_dir_all(source.join("tools")).unwrap();
        std::fs::write(source.join("tools/SYMPOSIUM.toml"), "name = \"tools\"").unwrap();
        std::fs::create_dir_all(source.join("nested/style")).unwrap();
        std::fs::write(
            source.join("nested/style/SKILL.md"),
            "---\nname: style\ndescription: d\ndepends-on: serde\n---\nbody",
        )
        .unwrap();

        let pm = PathPm::new("user-plugins", &source);
        let active = pm.active_plugins(&[]).await;
        let mut names: Vec<&str> = active.iter().map(|p| p.plugin.name.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["style", "tools"]);
        assert!(active.iter().all(|p| p.canonical.pm == "user-plugins"));

        // load_plugin by the entry's subpath key.
        let one = pm
            .load_plugin(&PackageId::new("user-plugins", "tools", ANY_VERSION))
            .await;
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].plugin.name, "tools");

        let hits = pm.search("too").await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id.name, "tools");
    }

    #[tokio::test]
    async fn fetch_returns_the_local_entry_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let pm = PathPm::new("local", tmp.path().join("registry"));
        let id = PackageId::new("local", "tools", ANY_VERSION);
        let fetched = pm.fetch(&id, UpdateLevel::None).await.unwrap();
        assert_eq!(fetched.root, tmp.path().join("registry/tools"));
        assert_eq!(fetched.id, id);
    }
}
