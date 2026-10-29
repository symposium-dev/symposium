//! Shared test utilities: temporary fixture directories and a fake cargo
//! transport.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::UpdateLevel;
use crate::pm::{
    CARGO_PM, FetchedPackage, PackageId, PackageManager, PluginInfo, PluginKind, PmInstance,
    UnvalidatedPlugin,
};

/// A file entry for [`instantiate_fixture`].
pub struct File<'a>(pub &'a str, pub &'a str);

/// Create a temporary directory populated with the given files.
///
/// Each `File(path, content)` creates the file at `path` relative to the
/// temp directory root, creating parent directories as needed.
///
/// Returns the [`tempfile::TempDir`] handle (drop it to clean up).
///
/// ```ignore
/// let tmp = instantiate_fixture(&[
///     File("foo/Symposium.toml", r#"name = "foo"\ndepends-on = ["*"]"#),
///     File("bar/SKILL.md", "---\nname: bar\n---\nBody."),
/// ]);
/// let root = tmp.path();
/// ```
pub fn instantiate_fixture(files: &[File]) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    write_fixture_files(tmp.path(), files);
    tmp
}

/// Write files into an existing directory.
pub fn write_fixture_files(root: &Path, files: &[File]) {
    for File(path, content) in files {
        let full = root.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&full, content).unwrap();
    }
}

/// A stand-in for the cargo transport, comparing names the way cargo does. It
/// has its `cached` crates at every [`UpdateLevel`] and its `published` ones
/// only above `None`, and counts in `network` every lookup or load that would
/// reach crates.io or spawn cargo.
pub struct FakeCargo {
    cached: &'static [&'static str],
    published: &'static [&'static str],
    network: Arc<AtomicUsize>,
}

impl FakeCargo {
    pub fn instance(
        cached: &'static [&'static str],
        published: &'static [&'static str],
    ) -> (PmInstance, Arc<AtomicUsize>) {
        let network = Arc::new(AtomicUsize::new(0));
        let pm = FakeCargo {
            cached,
            published,
            network: Arc::clone(&network),
        };
        let instance = PmInstance::new(CARGO_PM, false, PluginKind::Package, Box::new(pm));
        (instance, network)
    }

    fn has(&self, crates: &[&'static str], name: &str) -> Option<PackageId> {
        crates
            .iter()
            .find(|known| self.canonical_name(known) == self.canonical_name(name))
            .map(|known| PackageId::any_version(CARGO_PM, *known))
    }
}

#[async_trait::async_trait]
impl PackageManager for FakeCargo {
    fn name(&self) -> &str {
        CARGO_PM
    }

    fn canonical_name(&self, name: &str) -> String {
        crate::crate_sources::normalize_crate_name(name)
    }

    async fn load_plugin(
        &self,
        _id: &PackageId,
        update: UpdateLevel,
    ) -> anyhow::Result<Vec<UnvalidatedPlugin>> {
        if !matches!(update, UpdateLevel::None) {
            self.network.fetch_add(1, Ordering::SeqCst);
        }
        Ok(Vec::new())
    }

    async fn search(&self, query: &str) -> anyhow::Result<Vec<PluginInfo>> {
        self.network.fetch_add(1, Ordering::SeqCst);
        Ok(self
            .has(self.published, query)
            .into_iter()
            .map(|id| PluginInfo {
                id,
                description: None,
            })
            .collect())
    }

    /// Cache-only at [`UpdateLevel::None`], as the cargo transport is.
    async fn fetch(&self, id: &PackageId, update: UpdateLevel) -> anyhow::Result<FetchedPackage> {
        let found = match update {
            UpdateLevel::None => self.has(self.cached, &id.name),
            _ => {
                self.network.fetch_add(1, Ordering::SeqCst);
                self.has(self.cached, &id.name)
                    .or_else(|| self.has(self.published, &id.name))
            }
        };
        match found {
            Some(id) => Ok(FetchedPackage {
                id,
                root: std::env::temp_dir(),
            }),
            None => anyhow::bail!("cannot fetch {id}"),
        }
    }
}
