use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use symposium_sdk::manifest::RawPluginManifest;

use super::*;
use crate::pm::{
    CARGO_PM, FetchedPackage, PackageManager, PluginInfo, PluginKind, PmInstance, UnvalidatedPlugin,
};

struct PublishedCrate {
    name: &'static str,
    root: PathBuf,
    cached: AtomicBool,
}

#[async_trait::async_trait]
impl PackageManager for PublishedCrate {
    fn name(&self) -> &str {
        CARGO_PM
    }

    fn canonical_name(&self, name: &str) -> String {
        symposium_pm_cargo::sources::normalize_crate_name(name)
    }

    async fn search(&self, query: &str) -> Result<Vec<PluginInfo>> {
        Ok(
            (self.canonical_name(query) == self.canonical_name(self.name))
                .then(|| PluginInfo::from_id(PackageId::new(CARGO_PM, self.name, "1.2.3")))
                .into_iter()
                .collect(),
        )
    }

    async fn fetch(&self, id: &PackageId, update: UpdateLevel) -> Result<FetchedPackage> {
        let known = match update {
            UpdateLevel::None => {
                self.cached.load(Ordering::SeqCst)
                    && self.canonical_name(&id.name) == self.canonical_name(self.name)
            }
            _ => id.name == self.name,
        };
        anyhow::ensure!(known, "no matching package named `{}` found", id.name);
        self.cached.store(true, Ordering::SeqCst);
        Ok(FetchedPackage {
            id: PackageId::new(CARGO_PM, self.name, "1.2.3"),
            root: self.root.clone(),
        })
    }

    async fn load_plugin(
        &self,
        id: &PackageId,
        update: UpdateLevel,
    ) -> Result<Vec<UnvalidatedPlugin>> {
        let fetched = self.fetch(id, update).await?;
        Ok(vec![UnvalidatedPlugin::new(
            fetched.id,
            fetched.root,
            RawPluginManifest::default(),
        )])
    }
}

fn outside_a_workspace(dir: &Path) -> (Symposium, Arc<Workspace>) {
    std::fs::write(
        dir.join("config.toml"),
        "[defaults]\nsymposium-recommendations = false\nuser-plugins = false\nsymposium-audits = true\n",
    )
    .unwrap();
    let cargo = PublishedCrate {
        name: "widget-lib",
        root: dir.to_path_buf(),
        cached: AtomicBool::new(false),
    };
    let workspace = Arc::new(Workspace::new(
        Some(dir.to_path_buf()),
        PmRegistry::new(vec![PmInstance::new(
            CARGO_PM,
            false,
            PluginKind::Package,
            Box::new(cargo),
        )]),
    ));
    (Symposium::from_dir(dir), workspace)
}

#[tokio::test]
async fn a_revoked_crate_outside_the_workspace_is_refused_by_any_spelling() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut sym, workspace) = outside_a_workspace(tmp.path());
    let audits = sym.audits_dir().unwrap().join("audits/cargo");
    std::fs::create_dir_all(&audits).unwrap();
    std::fs::write(
        audits.join("widget-lib.toml"),
        "format = 1\n[[revoked]]\nversions = \"*\"\nreason = \"remote-exec\"\n",
    )
    .unwrap();

    for typed in ["widget_lib", "widget-lib"] {
        let err = use_in(&mut sym, &workspace, typed, true, UpdateLevel::None)
            .await
            .expect_err("a revoked crate cannot be enabled");

        assert!(format!("{err:#}").contains("revoked"), "{typed}: {err:#}");
        assert!(sym.config.plugins.used.is_empty(), "{typed}");
    }
}

#[tokio::test]
async fn use_records_the_name_a_crate_is_published_as() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut sym, workspace) = outside_a_workspace(tmp.path());

    use_in(&mut sym, &workspace, "widget_lib", true, UpdateLevel::None)
        .await
        .unwrap();

    assert_eq!(
        sym.config.plugins.used,
        [UseEntry::Global("widget-lib".to_string())]
    );
    let enabled = crate::discovery::enabled_dependencies(
        &sym,
        workspace.pms(),
        &[],
        &[],
        tmp.path(),
        UpdateLevel::None,
    )
    .await;
    assert_eq!(enabled, [PackageId::any_version(CARGO_PM, "widget-lib")]);
}

struct CachedCrates(PathBuf);

#[async_trait::async_trait]
impl PackageManager for CachedCrates {
    fn name(&self) -> &str {
        CARGO_PM
    }

    async fn fetch(&self, id: &PackageId, _update: UpdateLevel) -> Result<FetchedPackage> {
        let root = self.0.join(&id.name);
        anyhow::ensure!(root.is_dir(), "`{}` is not cached", id.name);
        Ok(FetchedPackage {
            id: PackageId::new(CARGO_PM, &id.name, "1.0.0"),
            root,
        })
    }

    async fn load_plugin(
        &self,
        id: &PackageId,
        update: UpdateLevel,
    ) -> Result<Vec<UnvalidatedPlugin>> {
        let fetched = self.fetch(id, update).await?;
        let manifest = std::fs::read_to_string(fetched.root.join("Symposium.toml"))?;
        Ok(vec![UnvalidatedPlugin::new(
            fetched.id,
            fetched.root,
            RawPluginManifest::parse(&manifest)?,
        )])
    }
}

#[tokio::test]
async fn a_revoked_crate_loses_its_mcp_servers_outside_a_workspace() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("config.toml"),
        "hook-scope = \"global\"\n\n[[agent]]\nname = \"claude\"\n\n\
         [defaults]\nsymposium-recommendations = false\nuser-plugins = false\n\
         symposium-audits = true\n",
    )
    .unwrap();
    let crates = tmp.path().join("crates");
    for name in ["revoked-kit", "other-kit"] {
        std::fs::create_dir_all(crates.join(name)).unwrap();
        std::fs::write(
            crates.join(name).join("Symposium.toml"),
            format!(
                "[[mcp_servers]]\nname = \"{name}-server\"\n\
                 command = \"/usr/bin/true\"\nargs = []\nenv = []\n"
            ),
        )
        .unwrap();
    }
    let workspace = Arc::new(Workspace::new(
        Some(tmp.path().to_path_buf()),
        PmRegistry::new(vec![PmInstance::new(
            CARGO_PM,
            false,
            PluginKind::Package,
            Box::new(CachedCrates(crates)),
        )]),
    ));
    let mut sym = Symposium::from_dir(tmp.path());
    let registered =
        || std::fs::read_to_string(tmp.path().join(".claude.json")).unwrap_or_default();

    use_in(&mut sym, &workspace, "revoked-kit", true, UpdateLevel::None)
        .await
        .unwrap();
    assert!(
        registered().contains("revoked-kit-server"),
        "{}",
        registered()
    );
    let audits = sym.audits_dir().unwrap().join("audits/cargo");
    std::fs::create_dir_all(&audits).unwrap();
    std::fs::write(
        audits.join("revoked-kit.toml"),
        "format = 1\n[[revoked]]\nversions = \"*\"\nreason = \"remote-exec\"\n",
    )
    .unwrap();
    use_in(&mut sym, &workspace, "other-kit", true, UpdateLevel::None)
        .await
        .unwrap();

    let registered = registered();
    assert!(registered.contains("other-kit-server"), "{registered}");
    assert!(!registered.contains("revoked-kit-server"), "{registered}");
}
