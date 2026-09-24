//! Checked-in benchmark fixtures and their validation.

use anyhow::{Context, Result, bail, ensure};
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    sync::LazyLock,
};
use symposium::pm::{LoadedWorkspace, WorkspaceCrate};

static FIXTURES_ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("benchsuite must live inside the benches directory")
        .join("fixtures")
});

#[derive(Debug, Clone, Copy)]
struct WorkspaceShape {
    members: &'static [&'static str],
    path_dependencies: &'static [&'static str],
}

impl WorkspaceShape {
    fn check_members(&self, root: &Path, members: &[PathBuf]) -> Result<()> {
        let names = members
            .iter()
            .map(|member| leaf_name(member))
            .collect::<Result<Vec<_>>>()?;
        check_names("workspace members", self.members, &names)?;

        for (member, name) in members.iter().zip(names) {
            let expected = canonicalize(&root.join(name))?;

            ensure!(
                canonicalize(member)? == expected,
                "workspace member `{name}` resolved outside the fixture: expected `{}`, found `{}`",
                expected.display(),
                member.display()
            );
        }

        Ok(())
    }

    fn check_dependencies(&self, root: &Path, dependencies: &[WorkspaceCrate]) -> Result<()> {
        let names: Vec<_> = dependencies
            .iter()
            .map(|dependency| dependency.name.as_str())
            .collect();
        check_names("direct dependencies", self.path_dependencies, &names)?;

        for dependency in dependencies {
            check_path_dependency(root, dependency)?;
        }

        Ok(())
    }
}

#[derive(Debug)]
struct FixtureSpec {
    directory_name: &'static str,
    required_files: &'static [&'static str],
    workspace_shape: Option<WorkspaceShape>,
}

const REFERENCE_PROJECT_SPEC: FixtureSpec = FixtureSpec {
    directory_name: "reference-project",
    required_files: &["Cargo.toml", "Cargo.lock", ".cargo/config.toml"],
    workspace_shape: Some(WorkspaceShape {
        members: &["cli", "server"],
        path_dependencies: &["domain", "storage", "terminal"],
    }),
};

const LOCAL_REGISTRY_SPEC: FixtureSpec = FixtureSpec {
    directory_name: "local-registry",
    required_files: &[
        "always-active/SYMPOSIUM.toml",
        "predicate-gated/SYMPOSIUM.toml",
        "predicate-gated/unexpected-hook.sh",
        "dormant/SYMPOSIUM.toml",
    ],
    workspace_shape: None,
};

/// A checked-in benchmark workload under `benches/fixtures`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fixture {
    ReferenceProject,
    LocalRegistry,
}

impl Fixture {
    const fn spec(self) -> &'static FixtureSpec {
        match self {
            Self::ReferenceProject => &REFERENCE_PROJECT_SPEC,
            Self::LocalRegistry => &LOCAL_REGISTRY_SPEC,
        }
    }

    pub(crate) const fn directory_name(self) -> &'static str {
        self.spec().directory_name
    }

    fn source_dir(self) -> Result<PathBuf> {
        let path = FIXTURES_ROOT.join(self.directory_name());

        ensure!(
            path.is_dir(),
            "benchmark fixture `{}` is missing: {}",
            self.directory_name(),
            path.display()
        );

        Ok(path)
    }

    /// Copy the fixture into `destination`, which must not already exist.
    fn copy_to(self, destination: impl AsRef<Path>) -> Result<()> {
        copy_directory(&self.source_dir()?, destination.as_ref())
    }
}

/// A fixture copied into a [`crate::Sandbox`], with its layout validated, so a
/// benchmark cannot time a workload that is missing checked-in files.
#[derive(Debug)]
pub struct StagedFixture {
    fixture: Fixture,
    path: PathBuf,
}

impl StagedFixture {
    pub(crate) fn stage(fixture: Fixture, path: PathBuf) -> Result<Self> {
        fixture.copy_to(&path)?;
        let staged = Self { fixture, path };
        staged.check_layout()?;
        Ok(staged)
    }

    pub fn fixture(&self) -> Fixture {
        self.fixture
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn check_layout(&self) -> Result<()> {
        for required in self.fixture.spec().required_files {
            let path = required
                .split('/')
                .fold(self.path.clone(), |path, part| path.join(part));

            ensure!(
                path.is_file(),
                "staged fixture `{}` is missing `{required}`: {}",
                self.fixture.directory_name(),
                path.display()
            );
        }

        Ok(())
    }

    /// Check a resolved Cargo graph against the shape this fixture promises.
    pub fn check_workspace(&self, workspace: &LoadedWorkspace) -> Result<()> {
        let Some(shape) = self.fixture.spec().workspace_shape else {
            bail!(
                "fixture `{}` is not a Cargo project",
                self.fixture.directory_name()
            );
        };

        let root = canonicalize(&self.path)?;
        ensure!(
            canonicalize(&workspace.root)? == root,
            "workspace root mismatch: expected `{}`, found `{}`",
            root.display(),
            workspace.root.display()
        );

        shape.check_members(&root, &workspace.members)?;
        shape.check_dependencies(&root, &workspace.crates)
    }
}

fn check_path_dependency(root: &Path, dependency: &WorkspaceCrate) -> Result<()> {
    // The fixture is hermetic only while every dependency resolves inside it.
    let Some(path) = dependency.path.as_deref() else {
        bail!(
            "dependency `{}` is not a local path dependency",
            dependency.name
        );
    };
    let expected = canonicalize(&root.join(&dependency.name))?;

    ensure!(
        canonicalize(path)? == expected,
        "dependency `{}` resolved outside the fixture: expected `{}`, found `{}`",
        dependency.name,
        expected.display(),
        path.display()
    );

    let Some(source_dir) = dependency.source_dir.as_deref() else {
        bail!("dependency `{}` has no source directory", dependency.name);
    };

    ensure!(
        canonicalize(source_dir)? == expected,
        "dependency `{}` source directory resolved outside the fixture: expected `{}`, found `{}`",
        dependency.name,
        expected.display(),
        source_dir.display()
    );

    Ok(())
}

/// Compare two name lists order-insensitively. Sorted vectors rather than sets,
/// so a duplicated name fails instead of being absorbed.
fn check_names(kind: &str, expected: &[&str], actual: &[&str]) -> Result<()> {
    let mut expected = expected.to_vec();
    let mut actual = actual.to_vec();
    expected.sort_unstable();
    actual.sort_unstable();

    ensure!(
        expected == actual,
        "{kind} mismatch: expected [{}], found [{}]",
        expected.join(", "),
        actual.join(", ")
    );

    Ok(())
}

fn leaf_name(path: &Path) -> Result<&str> {
    let Some(name) = path.file_name().and_then(OsStr::to_str) else {
        bail!("path has no usable directory name: {}", path.display());
    };

    Ok(name)
}

fn canonicalize(path: &Path) -> Result<PathBuf> {
    fs::canonicalize(path).with_context(|| format!("canonicalizing `{}`", path.display()))
}

fn copy_directory(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir(destination).with_context(|| {
        format!(
            "creating fixture destination directory `{}`",
            destination.display()
        )
    })?;

    for entry in source
        .read_dir()
        .with_context(|| format!("reading fixture directory `{}`", source.display()))?
    {
        let entry = entry.with_context(|| format!("reading an entry in `{}`", source.display()))?;
        copy_entry(&entry, destination)?;
    }

    Ok(())
}

fn copy_entry(entry: &fs::DirEntry, destination_directory: &Path) -> Result<()> {
    let source = entry.path();
    let destination = destination_directory.join(entry.file_name());
    let file_type = entry
        .file_type()
        .with_context(|| format!("reading file type for `{}`", source.display()))?;

    if file_type.is_dir() {
        copy_directory(&source, &destination)
    } else if file_type.is_file() {
        fs::copy(&source, &destination).with_context(|| {
            format!(
                "copying fixture file `{}` to `{}`",
                source.display(),
                destination.display()
            )
        })?;
        Ok(())
    } else {
        bail!(
            "fixture contains an unsupported filesystem entry: {}",
            source.display()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Sandbox;
    use symposium::dirs::SymposiumDirs;
    use tempfile::tempdir;

    fn resolve_reference_project() -> Result<(Sandbox, StagedFixture, LoadedWorkspace)> {
        let sandbox = Sandbox::new()?;
        let project = sandbox.stage(Fixture::ReferenceProject)?;
        let dirs = SymposiumDirs::new(
            sandbox.config_dir().to_path_buf(),
            sandbox.cache_dir().to_path_buf(),
            None,
        );
        let workspace = dirs
            .workspace_deps(project.path())
            .load()
            .context("resolving the staged reference project")?
            .as_ref()
            .clone();

        Ok((sandbox, project, workspace))
    }

    #[test]
    fn finds_reference_project_fixture() -> Result<()> {
        let source_dir = Fixture::ReferenceProject.source_dir()?;

        assert!(source_dir.join("Cargo.toml").is_file());

        Ok(())
    }

    #[test]
    fn finds_local_registry_fixture() -> Result<()> {
        let source_dir = Fixture::LocalRegistry.source_dir()?;

        assert!(
            source_dir
                .join("always-active")
                .join("SYMPOSIUM.toml")
                .is_file()
        );

        Ok(())
    }

    #[test]
    fn copies_reference_project_fixture() -> Result<()> {
        let temporary_directory = tempdir()?;
        let destination = temporary_directory.path().join("reference-project");

        Fixture::ReferenceProject.copy_to(&destination)?;

        assert!(destination.join("Cargo.toml").is_file());
        assert!(destination.join("domain/src/lib.rs").is_file());

        Ok(())
    }

    #[test]
    fn refuses_to_merge_into_an_existing_destination() -> Result<()> {
        let temporary_directory = tempdir()?;
        let destination = temporary_directory.path().join("reference-project");
        let sentinel = destination.join("sentinel");

        fs::create_dir(&destination)?;
        fs::write(&sentinel, "leave me untouched")?;

        let error = Fixture::ReferenceProject
            .copy_to(&destination)
            .expect_err("copying into an existing destination must fail");

        assert!(
            error
                .to_string()
                .contains(&destination.display().to_string()),
            "error does not name destination `{}`: {error:#}",
            destination.display()
        );
        assert_eq!(fs::read_to_string(sentinel)?, "leave me untouched");
        assert!(!destination.join("Cargo.toml").try_exists()?);

        Ok(())
    }

    #[test]
    fn staging_checks_the_promised_layout() -> Result<()> {
        let sandbox = Sandbox::new()?;
        let project = sandbox.stage(Fixture::ReferenceProject)?;

        project.check_layout()?;
        fs::remove_file(project.path().join("Cargo.lock"))?;

        let error = project
            .check_layout()
            .expect_err("a missing required file must fail validation");
        assert!(error.to_string().contains("Cargo.lock"));

        Ok(())
    }

    #[test]
    fn accepts_the_resolved_reference_project() -> Result<()> {
        let (_sandbox, project, workspace) = resolve_reference_project()?;

        project.check_workspace(&workspace)
    }

    #[test]
    fn rejects_a_missing_workspace_member() -> Result<()> {
        let (_sandbox, project, mut workspace) = resolve_reference_project()?;

        workspace.members.pop();

        let error = project
            .check_workspace(&workspace)
            .expect_err("a missing workspace member must fail validation");
        assert!(error.to_string().contains("workspace members mismatch"));

        Ok(())
    }

    #[test]
    fn rejects_a_workspace_member_outside_the_fixture() -> Result<()> {
        let (sandbox, project, mut workspace) = resolve_reference_project()?;
        let member_name = workspace.members[0]
            .file_name()
            .context("fixture member must have a directory name")?;
        let outside_member = sandbox.config_dir().join(member_name);

        fs::create_dir(&outside_member)?;
        workspace.members[0] = outside_member;

        let error = project
            .check_workspace(&workspace)
            .expect_err("a workspace member outside the fixture must fail validation");
        assert!(error.to_string().contains("resolved outside the fixture"));

        Ok(())
    }

    #[test]
    fn rejects_a_missing_dependency() -> Result<()> {
        let (_sandbox, project, mut workspace) = resolve_reference_project()?;

        workspace.crates.pop();

        let error = project
            .check_workspace(&workspace)
            .expect_err("a missing direct dependency must fail validation");
        assert!(error.to_string().contains("direct dependencies mismatch"));

        Ok(())
    }

    #[test]
    fn rejects_a_non_path_dependency() -> Result<()> {
        let (_sandbox, project, mut workspace) = resolve_reference_project()?;

        workspace.crates[0].path = None;

        let error = project
            .check_workspace(&workspace)
            .expect_err("a registry dependency must fail validation");
        assert!(error.to_string().contains("not a local path dependency"));

        Ok(())
    }

    #[test]
    fn rejects_a_dependency_source_outside_the_fixture() -> Result<()> {
        let (sandbox, project, mut workspace) = resolve_reference_project()?;
        let outside_source = sandbox.config_dir().join(&workspace.crates[0].name);

        fs::create_dir(&outside_source)?;
        workspace.crates[0].source_dir = Some(outside_source);

        let error = project
            .check_workspace(&workspace)
            .expect_err("a dependency source outside the fixture must fail validation");
        assert!(
            error
                .to_string()
                .contains("source directory resolved outside the fixture")
        );

        Ok(())
    }

    #[test]
    fn rejects_a_workspace_check_on_a_registry_fixture() -> Result<()> {
        let (_sandbox, _project, workspace) = resolve_reference_project()?;
        let registry_sandbox = Sandbox::new()?;
        let registry = registry_sandbox.stage(Fixture::LocalRegistry)?;

        let error = registry
            .check_workspace(&workspace)
            .expect_err("the registry fixture is not a Cargo project");
        assert!(error.to_string().contains("not a Cargo project"));

        Ok(())
    }
}
