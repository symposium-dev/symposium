//! Rust crate source fetching and management.
//!
//! Registry crate fetching delegates to `cargo fetch` via a dummy temporary
//! package (see [`probe`]) rather than hitting `crates.io` HTTP endpoints
//! directly. Local path dependencies short-circuit through the workspace's
//! path overrides and never touch the registry.

use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use symposium_install::UpdateLevel;
use symposium_sdk::workspace::{WorkspaceCrate, WorkspaceDeps};

use crate::config::{CargoDependencySpec, CrateSourceSpec, InstalledSourceConfig, Symposium};

mod list;
mod probe;

pub use list::crate_pairs;

/// Registry namespace that produced a source root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceRegistry {
    Path,
    Git,
    Crate,
}

/// A plugin source declaration before registry resolution.
#[derive(Debug, Clone, PartialEq)]
pub enum RegistrySourceSpec {
    /// Direct path registry source.
    Path(PathBuf),
    /// Direct git registry source.
    Git(String),
    /// Cargo crate-registry source.
    Crate(CrateSourceSpec),
}

/// Concrete source root produced by a registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSourceRoot {
    pub registry: SourceRegistry,
    pub source_id: String,
    pub path: PathBuf,
}

/// Non-exclusive provenance flags for why a source is in the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SourceProvenance {
    Installed,
    Workspace,
    Dependency,
}

/// Human-readable reason a source was reached.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceReason {
    pub provenance: SourceProvenance,
    pub detail: String,
}

/// One deduplicated source node in the resolved graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSourceNode {
    pub root: ResolvedSourceRoot,
    pub provenance: BTreeSet<SourceProvenance>,
    pub reasons: BTreeSet<SourceReason>,
}

/// Resolved plugin source graph before discovery mutates agent directories.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedSourceGraph {
    nodes: Vec<ResolvedSourceNode>,
}

impl ResolvedSourceGraph {
    pub async fn resolve_installed_and_workspace(
        sym: &Symposium,
        workspace: &mut WorkspaceDeps,
    ) -> Result<Self> {
        let resolver = SourceRegistryResolver::new(sym);
        let mut graph = ResolvedSourceGraph::default();

        for spec in installed_source_specs(sym.installed_sources()) {
            match resolver.resolve(&spec).await {
                Ok(root) => {
                    graph.add_root(
                        root,
                        SourceReason {
                            provenance: SourceProvenance::Installed,
                            detail: "installed config".to_string(),
                        },
                    );
                }
                Err(e) => {
                    tracing::warn!(error = %e, "failed to resolve installed source, skipping");
                }
            }
        }

        if let Some(loaded) = workspace.load() {
            let root_path =
                std::fs::canonicalize(&loaded.root).unwrap_or_else(|_| loaded.root.clone());
            graph.add_root(
                ResolvedSourceRoot {
                    registry: SourceRegistry::Path,
                    source_id: format!("workspace:{}", root_path.display()),
                    path: root_path,
                },
                SourceReason {
                    provenance: SourceProvenance::Workspace,
                    detail: "workspace root".to_string(),
                },
            );

            for member in &loaded.members {
                let Some(path) = member.path.as_ref() else {
                    continue;
                };
                let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
                graph.add_root(
                    ResolvedSourceRoot {
                        registry: SourceRegistry::Path,
                        source_id: format!("workspace-member:{}@{}", member.name, member.version),
                        path,
                    },
                    SourceReason {
                        provenance: SourceProvenance::Workspace,
                        detail: format!("workspace member {}", member.name),
                    },
                );
            }
        }

        Ok(graph)
    }

    pub fn nodes(&self) -> &[ResolvedSourceNode] {
        &self.nodes
    }

    pub fn add_resolved_root(&mut self, root: ResolvedSourceRoot, reason: SourceReason) {
        self.add_root(root, reason);
    }

    fn add_root(&mut self, root: ResolvedSourceRoot, reason: SourceReason) {
        if let Some(existing) = self
            .nodes
            .iter_mut()
            .find(|node| node.root.path == root.path)
        {
            existing.provenance.insert(reason.provenance);
            existing.reasons.insert(reason);
            return;
        }

        let mut provenance = BTreeSet::new();
        provenance.insert(reason.provenance);
        let mut reasons = BTreeSet::new();
        reasons.insert(reason);
        self.nodes.push(ResolvedSourceNode {
            root,
            provenance,
            reasons,
        });
    }
}

/// Resolver for installed or transitive plugin source declarations.
pub struct SourceRegistryResolver<'a> {
    sym: &'a Symposium,
    update: UpdateLevel,
}

impl<'a> SourceRegistryResolver<'a> {
    pub fn new(sym: &'a Symposium) -> Self {
        Self {
            sym,
            update: UpdateLevel::None,
        }
    }

    pub fn update(mut self, update: UpdateLevel) -> Self {
        self.update = update;
        self
    }

    pub async fn resolve(&self, spec: &RegistrySourceSpec) -> Result<ResolvedSourceRoot> {
        match spec {
            RegistrySourceSpec::Path(path) => self.resolve_path(path),
            RegistrySourceSpec::Git(url) => self.resolve_git(url).await,
            RegistrySourceSpec::Crate(spec) => self.resolve_crate(spec).await,
        }
    }

    pub async fn resolve_installed_sources(&self) -> Result<Vec<ResolvedSourceRoot>> {
        let mut roots = Vec::new();
        for spec in installed_source_specs(self.sym.installed_sources()) {
            roots.push(self.resolve(&spec).await?);
        }
        Ok(roots)
    }

    fn resolve_path(&self, path: &std::path::Path) -> Result<ResolvedSourceRoot> {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.sym.config_dir().join(path)
        };
        let canonical = std::fs::canonicalize(&path)
            .with_context(|| format!("failed to resolve path source {}", path.display()))?;
        if !canonical.is_dir() {
            bail!("path source {} is not a directory", canonical.display());
        }
        Ok(ResolvedSourceRoot {
            registry: SourceRegistry::Path,
            source_id: format!("path:{}", canonical.display()),
            path: canonical,
        })
    }

    async fn resolve_git(&self, url: &str) -> Result<ResolvedSourceRoot> {
        let cache_mgr = symposium_install::git::GitCacheManager::new(
            &self.sym.install_context(),
            "plugin-sources",
        );
        let path = cache_mgr.fetch_url(url, self.update).await?;
        Ok(ResolvedSourceRoot {
            registry: SourceRegistry::Git,
            source_id: format!("git:{url}"),
            path,
        })
    }

    async fn resolve_crate(&self, spec: &CrateSourceSpec) -> Result<ResolvedSourceRoot> {
        let result =
            probe::fetch_dependency_via_cargo(spec.key.as_deref(), &spec.dependency).await?;
        Ok(ResolvedSourceRoot {
            registry: SourceRegistry::Crate,
            source_id: format!("crate:{}@{}", result.name, result.version),
            path: result.path,
        })
    }
}

/// Normalize installed source config into registry source specs.
pub fn installed_source_specs(installed: &InstalledSourceConfig) -> Vec<RegistrySourceSpec> {
    let mut specs = Vec::new();
    specs.extend(installed.crates.iter().map(|(name, dependency)| {
        RegistrySourceSpec::Crate(CrateSourceSpec {
            key: Some(name.clone()),
            dependency: dependency.clone(),
        })
    }));
    specs.extend(
        installed
            .paths
            .iter()
            .map(|path| RegistrySourceSpec::Path(PathBuf::from(path))),
    );
    specs.extend(
        installed
            .git
            .iter()
            .map(|url| RegistrySourceSpec::Git(url.clone())),
    );
    specs
}

/// Render Cargo dependency-table entries for crate-registry source specs.
pub fn crate_dependency_table_toml<'a>(
    specs: impl IntoIterator<Item = (&'a str, &'a CargoDependencySpec)>,
) -> Result<String> {
    probe::dependency_table_toml(specs)
}

/// Normalize a crate name for hyphen/underscore-insensitive comparison.
///
/// Cargo treats `foo-bar` and `foo_bar` as the same crate name (published
/// name on crates.io vs. Rust module identifier), so any name-equality check
/// against a user-supplied query should go through this normalization.
pub(crate) fn normalize_crate_name(name: &str) -> String {
    name.replace('-', "_")
}

/// Result of fetching a crate's sources.
#[derive(Debug, Clone)]
pub struct FetchResult {
    /// The canonical crate name (e.g. `serde_json` even if queried as `serde-json`).
    pub name: String,
    /// The exact version that was fetched.
    pub version: String,
    /// Path to the crate sources on disk.
    pub path: PathBuf,
}

/// Builder for accessing Rust crate source code.
pub struct RustCrateFetch<'a> {
    crate_name: String,
    version_spec: Option<String>,
    workspace: &'a [WorkspaceCrate],
}

impl<'a> RustCrateFetch<'a> {
    /// Create a new fetch request for the given crate name.
    pub fn new(name: &str, workspace: &'a [WorkspaceCrate]) -> Self {
        Self {
            crate_name: name.to_string(),
            version_spec: None,
            workspace,
        }
    }

    /// Specify a version constraint (e.g. `"^1.0"`, `"=1.2.3"`).
    pub fn version(mut self, version: &str) -> Self {
        self.version_spec = Some(version.to_string());
        self
    }

    /// Fetch the crate sources, returning a path to the source directory.
    ///
    /// Resolution order:
    /// 1. If the crate is a local path dependency in the workspace (and no
    ///    explicit `--version` was requested), return the path directly.
    /// 2. Otherwise, run `cargo fetch` in a temporary dummy package to
    ///    populate cargo's registry cache, then read `cargo metadata` to get
    ///    the extracted source path under `~/.cargo/registry/src/`.
    pub async fn fetch(self) -> Result<FetchResult> {
        // Check path overrides first (local path dependencies).
        if self.version_spec.is_none() {
            let normalized = normalize_crate_name(&self.crate_name);
            if let Some(wc) = self
                .workspace
                .iter()
                .find(|wc| wc.path.is_some() && normalize_crate_name(&wc.name) == normalized)
            {
                let path = wc.path.as_ref().unwrap();
                tracing::debug!(crate_name = %wc.name, path = %path.display(), "resolved from path override");
                return Ok(FetchResult {
                    name: wc.name.clone(),
                    version: wc.version.to_string(),
                    path: path.clone(),
                });
            }
        }

        let (name, version_req) = self.resolve_registry_spec();
        probe::fetch_via_cargo(&name, &version_req).await
    }

    /// Choose the `(name, version_req)` pair to put in the probe package's
    /// dependency entry when going through the registry.
    ///
    /// Precedence:
    /// 1. Explicit `--version` constraint from the caller.
    /// 2. If the crate is a direct dependency of the current workspace, pin
    ///    to that exact resolved version (`=x.y.z`).
    /// 3. Otherwise, `"*"` — cargo picks the latest compatible version.
    fn resolve_registry_spec(&self) -> (String, String) {
        if let Some(spec) = &self.version_spec {
            return (self.crate_name.clone(), spec.clone());
        }

        let normalized = normalize_crate_name(&self.crate_name);
        if let Some(wc) = self
            .workspace
            .iter()
            .find(|wc| normalize_crate_name(&wc.name) == normalized)
        {
            return (wc.name.clone(), format!("={}", wc.version));
        }

        (self.crate_name.clone(), "*".to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn v(s: &str) -> semver::Version {
        semver::Version::parse(s).unwrap()
    }

    fn wc(name: &str, version: &str, path: Option<PathBuf>) -> WorkspaceCrate {
        WorkspaceCrate::new(name.to_string(), v(version), path)
    }

    fn spec_table(fields: &[(&str, toml::Value)]) -> CargoDependencySpec {
        CargoDependencySpec::Table(
            fields
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    fn write_minimal_crate(dir: &std::path::Path, name: &str, version: &str) {
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "").unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            format!(
                r#"[package]
name = "{name}"
version = "{version}"
edition = "2021"
"#
            ),
        )
        .unwrap();
    }

    fn write_virtual_workspace(root: &std::path::Path, members: &[(&str, &str)]) {
        let member_names = members
            .iter()
            .map(|(dir, _)| format!("\"{dir}\""))
            .collect::<Vec<_>>()
            .join(", ");
        std::fs::write(
            root.join("Cargo.toml"),
            format!(
                r#"[workspace]
members = [{member_names}]
resolver = "2"
"#
            ),
        )
        .unwrap();
        for (dir, package) in members {
            write_minimal_crate(&root.join(dir), package, "0.1.0");
        }
    }

    // -- Registry dependency rendering ---------------------------------

    #[test]
    fn dependency_table_toml_renders_registry_git_and_path_specs() {
        let registry = CargoDependencySpec::Version("1".to_string());
        let git = spec_table(&[
            (
                "git",
                toml::Value::String("https://github.com/me/plugin".to_string()),
            ),
            ("branch", toml::Value::String("main".to_string())),
        ]);
        let path = spec_table(&[
            ("path", toml::Value::String("../plugin".to_string())),
            ("package", toml::Value::String("actual-plugin".to_string())),
        ]);

        let rendered =
            crate_dependency_table_toml([("foo", &registry), ("bar", &git), ("baz", &path)])
                .unwrap();

        assert_eq!(
            rendered,
            concat!(
                "bar = { branch = \"main\", git = \"https://github.com/me/plugin\" }\n",
                "baz = { package = \"actual-plugin\", path = \"../plugin\" }\n",
                "foo = { version = \"1\" }"
            )
        );
    }

    // -- Path override behaviour ---------------------------------------

    #[tokio::test]
    async fn fetch_uses_path_override_for_path_dep() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = vec![wc("my-crate", "0.1.0", Some(tmp.path().to_path_buf()))];

        let result = RustCrateFetch::new("my-crate", &workspace)
            .fetch()
            .await
            .unwrap();

        assert_eq!(result.name, "my-crate");
        assert_eq!(result.version, "0.1.0");
        assert_eq!(result.path, tmp.path());
    }

    #[tokio::test]
    async fn fetch_path_override_normalizes_hyphens() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = vec![wc("my_crate", "0.1.0", Some(tmp.path().to_path_buf()))];

        // Query with hyphen, workspace entry uses underscore.
        let result = RustCrateFetch::new("my-crate", &workspace)
            .fetch()
            .await
            .unwrap();

        assert_eq!(result.name, "my_crate");
        assert_eq!(result.path, tmp.path());
    }

    // -- Registry spec resolution (pure, no I/O) -----------------------

    #[test]
    fn registry_spec_prefers_explicit_version() {
        let workspace = vec![wc("foo", "1.0.0", None)];
        let fetch = RustCrateFetch::new("foo", &workspace).version("^2.0");
        let (name, req) = fetch.resolve_registry_spec();
        assert_eq!(name, "foo");
        assert_eq!(req, "^2.0");
    }

    #[test]
    fn registry_spec_pins_workspace_version_exactly() {
        let workspace = vec![wc("foo", "1.2.3", None)];
        let fetch = RustCrateFetch::new("foo", &workspace);
        let (name, req) = fetch.resolve_registry_spec();
        assert_eq!(name, "foo");
        assert_eq!(req, "=1.2.3");
    }

    #[test]
    fn registry_spec_normalizes_hyphens_against_workspace() {
        let workspace = vec![wc("serde_json", "1.0.0", None)];
        let fetch = RustCrateFetch::new("serde-json", &workspace);
        let (name, req) = fetch.resolve_registry_spec();
        // Canonical name from the workspace wins.
        assert_eq!(name, "serde_json");
        assert_eq!(req, "=1.0.0");
    }

    #[test]
    fn registry_spec_falls_back_to_wildcard() {
        let workspace: Vec<WorkspaceCrate> = Vec::new();
        let fetch = RustCrateFetch::new("foo", &workspace);
        let (name, req) = fetch.resolve_registry_spec();
        assert_eq!(name, "foo");
        assert_eq!(req, "*");
    }

    #[test]
    fn registry_spec_is_used_when_version_specified_even_with_path_dep() {
        // Explicit version → path override is skipped → registry spec uses
        // the explicit version.
        let tmp = tempfile::tempdir().unwrap();
        let workspace = vec![wc("serde", "1.0.210", Some(tmp.path().to_path_buf()))];
        let fetch = RustCrateFetch::new("serde", &workspace).version("=99.99.99");
        let (name, req) = fetch.resolve_registry_spec();
        assert_eq!(name, "serde");
        assert_eq!(req, "=99.99.99");
    }

    // -- Source registry resolver --------------------------------------

    #[tokio::test]
    async fn path_registry_resolves_local_directory_without_network() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("plugin-source");
        std::fs::create_dir(&source).unwrap();
        let sym = Symposium::from_dir(tmp.path());

        let resolver = SourceRegistryResolver::new(&sym);
        let resolved = resolver
            .resolve(&RegistrySourceSpec::Path(PathBuf::from("plugin-source")))
            .await
            .unwrap();

        assert_eq!(resolved.registry, SourceRegistry::Path);
        assert_eq!(resolved.path, std::fs::canonicalize(&source).unwrap());
        assert!(resolved.source_id.starts_with("path:"));
    }

    #[tokio::test]
    async fn crate_registry_resolves_unkeyed_local_path_crate_without_network() {
        let tmp = tempfile::tempdir().unwrap();
        let crate_dir = tmp.path().join("my-plugin-crate");
        write_minimal_crate(&crate_dir, "actual-plugin", "0.1.0");

        let sym = Symposium::from_dir(tmp.path());
        let spec = CrateSourceSpec {
            key: None,
            dependency: spec_table(&[(
                "path",
                toml::Value::String(crate_dir.to_string_lossy().to_string()),
            )]),
        };

        let resolver = SourceRegistryResolver::new(&sym);
        let resolved = resolver
            .resolve(&RegistrySourceSpec::Crate(spec))
            .await
            .unwrap();

        assert_eq!(resolved.registry, SourceRegistry::Crate);
        assert_eq!(resolved.source_id, "crate:actual-plugin@0.1.0");
        assert_eq!(resolved.path, crate_dir);
    }

    #[tokio::test]
    async fn crate_registry_resolves_package_renamed_local_path_crate() {
        let tmp = tempfile::tempdir().unwrap();
        let crate_dir = tmp.path().join("actual-plugin");
        write_minimal_crate(&crate_dir, "actual-plugin", "0.2.0");

        let sym = Symposium::from_dir(tmp.path());
        let spec = CrateSourceSpec {
            key: Some("friendly-name".to_string()),
            dependency: spec_table(&[
                (
                    "path",
                    toml::Value::String(crate_dir.to_string_lossy().to_string()),
                ),
                ("package", toml::Value::String("actual-plugin".to_string())),
            ]),
        };

        let resolver = SourceRegistryResolver::new(&sym);
        let resolved = resolver
            .resolve(&RegistrySourceSpec::Crate(spec))
            .await
            .unwrap();

        assert_eq!(resolved.registry, SourceRegistry::Crate);
        assert_eq!(resolved.source_id, "crate:actual-plugin@0.2.0");
        assert_eq!(resolved.path, crate_dir);
    }

    #[tokio::test]
    async fn resolver_resolves_installed_path_and_local_crate_sources() {
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path();
        let direct_path = config_dir.join("direct-plugin-source");
        std::fs::create_dir(&direct_path).unwrap();

        let crate_dir = config_dir.join("crate-plugin-source");
        write_minimal_crate(&crate_dir, "crate-plugin-source", "0.3.0");

        std::fs::write(
            config_dir.join("config.toml"),
            format!(
                r#"[installed]
paths = ["direct-plugin-source"]

[installed.crates]
crate-plugin-source = {{ path = "{}" }}
"#,
                crate_dir.display()
            ),
        )
        .unwrap();

        let sym = Symposium::from_dir(config_dir);
        let resolver = SourceRegistryResolver::new(&sym);
        let roots = resolver.resolve_installed_sources().await.unwrap();

        assert_eq!(roots.len(), 2);
        assert!(roots.iter().any(|root| {
            root.registry == SourceRegistry::Crate
                && root.source_id == "crate:crate-plugin-source@0.3.0"
                && root.path == crate_dir
        }));
        assert!(roots.iter().any(|root| {
            root.registry == SourceRegistry::Path
                && root.path == std::fs::canonicalize(&direct_path).unwrap()
        }));
    }

    #[tokio::test]
    async fn source_graph_includes_workspace_root_and_members() {
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join("symposium-home");
        std::fs::create_dir(&config_dir).unwrap();
        std::fs::write(config_dir.join("config.toml"), "[installed]\n").unwrap();

        let workspace_root = tmp.path().join("workspace");
        std::fs::create_dir(&workspace_root).unwrap();
        write_virtual_workspace(&workspace_root, &[("member-a", "member-a")]);

        let sym = Symposium::from_dir(&config_dir);
        let mut deps = sym.workspace_deps(&workspace_root);
        let graph = ResolvedSourceGraph::resolve_installed_and_workspace(&sym, &mut deps)
            .await
            .unwrap();

        let root = std::fs::canonicalize(&workspace_root).unwrap();
        let member = std::fs::canonicalize(workspace_root.join("member-a")).unwrap();
        assert!(graph.nodes().iter().any(|node| {
            node.root.path == root && node.provenance.contains(&SourceProvenance::Workspace)
        }));
        assert!(graph.nodes().iter().any(|node| {
            node.root.path == member && node.provenance.contains(&SourceProvenance::Workspace)
        }));
    }

    #[tokio::test]
    async fn source_graph_dedupes_installed_workspace_root_and_keeps_reasons() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace_root = tmp.path().join("workspace");
        std::fs::create_dir(&workspace_root).unwrap();
        write_virtual_workspace(&workspace_root, &[("member-a", "member-a")]);

        let config_dir = tmp.path().join("symposium-home");
        std::fs::create_dir(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            format!(
                r#"[installed]
paths = ["{}"]
"#,
                workspace_root.display()
            ),
        )
        .unwrap();

        let sym = Symposium::from_dir(&config_dir);
        let mut deps = sym.workspace_deps(&workspace_root);
        let graph = ResolvedSourceGraph::resolve_installed_and_workspace(&sym, &mut deps)
            .await
            .unwrap();

        let root = std::fs::canonicalize(&workspace_root).unwrap();
        let root_nodes = graph
            .nodes()
            .iter()
            .filter(|node| node.root.path == root)
            .collect::<Vec<_>>();
        assert_eq!(root_nodes.len(), 1);
        let root_node = root_nodes[0];
        assert!(root_node.provenance.contains(&SourceProvenance::Installed));
        assert!(root_node.provenance.contains(&SourceProvenance::Workspace));
        assert_eq!(root_node.reasons.len(), 2);
    }

    #[test]
    fn source_graph_records_installed_workspace_and_dependency_on_one_node() {
        let tmp = tempfile::tempdir().unwrap();
        let source = std::fs::canonicalize(tmp.path()).unwrap();
        let root = ResolvedSourceRoot {
            registry: SourceRegistry::Path,
            source_id: "test-source".to_string(),
            path: source,
        };

        let mut graph = ResolvedSourceGraph::default();
        for provenance in [
            SourceProvenance::Installed,
            SourceProvenance::Workspace,
            SourceProvenance::Dependency,
        ] {
            graph.add_resolved_root(
                root.clone(),
                SourceReason {
                    provenance,
                    detail: format!("{provenance:?} reason"),
                },
            );
        }

        assert_eq!(graph.nodes().len(), 1);
        let node = &graph.nodes()[0];
        assert!(node.provenance.contains(&SourceProvenance::Installed));
        assert!(node.provenance.contains(&SourceProvenance::Workspace));
        assert!(node.provenance.contains(&SourceProvenance::Dependency));
        assert_eq!(node.reasons.len(), 3);
    }
}
