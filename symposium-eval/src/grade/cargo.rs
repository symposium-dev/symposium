//! The Cargo commands grading runs, and what it reads from them.
//!
//! Every command runs in the grading copy with `--locked --offline`, a private target
//! directory and `CARGO_HOME`, and incremental compilation off. Its environment is built,
//! not inherited: only what the toolchain needs to find itself, the linker, and a
//! temporary directory is passed through (the `RUSTUP_*` pair among them), so no
//! unapproved Cargo or Rust override reaches a grade. `$CARGO` is the Cargo running
//! `cargo xtask`, and `rustc` and `rustdoc` are taken from beside it, which keeps grading
//! on that toolchain.
//!
//! Known gap: Cargo also reads `.cargo/config.toml` in every directory above the
//! grading copy, which can include the developer's home. Only a run directory whose
//! ancestors grading controls closes that, which the `srt` backend provides.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::{fs, io};

use serde::Deserialize;

use super::GradeError;
use super::process::Invocation;
use crate::task::CheckName;

/// Variables passed through from the environment `cargo xtask` runs in, compared without
/// regard to case as Windows does. `rustup` finds its toolchain through `PATH`, the
/// `RUSTUP_*` pair, and the home directory; the Windows system variables are where the
/// MSVC linker and system libraries are found.
const PASSED_THROUGH: [&str; 17] = [
    "PATH",
    "HOME",
    "USERPROFILE",
    "RUSTUP_HOME",
    "RUSTUP_TOOLCHAIN",
    "TMPDIR",
    "TMP",
    "TEMP",
    "SYSTEMROOT",
    "WINDIR",
    "COMSPEC",
    "PATHEXT",
    "PROGRAMDATA",
    "PROGRAMFILES",
    "PROGRAMFILES(X86)",
    "LOCALAPPDATA",
    "APPDATA",
];

/// Builds the invocations for one grading copy.
#[derive(Debug, Clone)]
pub(crate) struct Cargo {
    program: OsString,
    project: PathBuf,
    env: Vec<(OsString, OsString)>,
}

/// The parts of `cargo metadata --format-version 1` grading reads.
#[derive(Debug, Deserialize)]
struct Metadata {
    packages: Vec<Package>,
}

#[derive(Debug, Deserialize)]
struct Package {
    targets: Vec<Target>,
}

#[derive(Debug, Deserialize)]
struct Target {
    name: String,
    kind: Vec<String>,
    src_path: PathBuf,
}

impl Cargo {
    /// `scratch` holds the target directory and `CARGO_HOME`, outside the project.
    pub(crate) fn new(project: PathBuf, scratch: &Path) -> Self {
        Self::with_environment(project, scratch, std::env::vars_os())
    }

    fn with_environment(
        project: PathBuf,
        scratch: &Path,
        inherited: impl IntoIterator<Item = (OsString, OsString)>,
    ) -> Self {
        let (program, mut env) = passed_through(inherited);
        env.push(("CARGO_TARGET_DIR".into(), scratch.join("target").into()));
        env.push(("CARGO_HOME".into(), scratch.join("cargo-home").into()));
        env.push(("CARGO_INCREMENTAL".into(), "0".into()));
        // `cargo fmt` has no `--offline` flag; this covers every subcommand.
        env.push(("CARGO_NET_OFFLINE".into(), "true".into()));
        for (variable, tool) in [("RUSTC", "rustc"), ("RUSTDOC", "rustdoc")] {
            if let Some(path) = beside(&program, tool) {
                env.push((variable.into(), path.into()));
            }
        }
        Self {
            program,
            project,
            env,
        }
    }

    /// Resolves the project against its lockfile, without building anything.
    pub(crate) fn metadata(&self, log_name: &str) -> Invocation {
        self.invocation(
            log_name,
            &["metadata", "--format-version", "1", "--locked", "--offline"],
        )
    }

    pub(crate) fn build(&self) -> Invocation {
        self.invocation("build", &["build", "--locked", "--offline"])
    }

    /// The tests the submission wrote in its library, however many there are.
    pub(crate) fn test_lib(&self) -> Invocation {
        self.invocation("test-lib", &["test", "--lib", "--locked", "--offline"])
    }

    /// Whether `cargo fmt` is installed, checked before anything is imported.
    pub(crate) fn fmt_version(&self) -> Invocation {
        self.invocation("fmt-version", &["fmt", "--version"])
    }

    /// rustfmt colors its diff even into a pipe, so color is turned off for the log.
    pub(crate) fn fmt_check(&self) -> Invocation {
        self.invocation("fmt", &["fmt", "--check", "--", "--color", "never"])
    }

    /// Lists the tests in the hidden check `check`, or only its ignored ones. Listing
    /// compiles the check against the submission without running any test.
    pub(crate) fn list_test(&self, check: &CheckName, ignored: bool) -> Invocation {
        let mut args = vec![
            "test",
            "--test",
            check.as_str(),
            "--locked",
            "--offline",
            "--",
            "--list",
            "--format",
            "terse",
        ];
        let log_name = if ignored {
            args.push("--ignored");
            format!("hidden-{check}-ignored")
        } else {
            format!("hidden-{check}-list")
        };
        self.invocation(log_name, &args)
    }

    pub(crate) fn run_test(&self, check: &CheckName) -> Invocation {
        self.invocation(
            format!("hidden-{check}"),
            &["test", "--test", check.as_str(), "--locked", "--offline"],
        )
    }

    pub(super) fn invocation(&self, log_name: impl Into<String>, args: &[&str]) -> Invocation {
        Invocation {
            program: self.program.clone(),
            args: args.iter().map(Into::into).collect(),
            dir: self.project.clone(),
            env: self.env.clone(),
            log_name: log_name.into(),
        }
    }
}

/// `$CARGO`, or `cargo` on `PATH`, and the inherited variables on the allowlist.
fn passed_through(
    inherited: impl IntoIterator<Item = (OsString, OsString)>,
) -> (OsString, Vec<(OsString, OsString)>) {
    let mut program = OsString::from("cargo");
    let mut env = Vec::new();
    for (name, value) in inherited {
        if name == "CARGO" {
            program = value;
        } else if name.to_str().is_some_and(|name| {
            PASSED_THROUGH
                .iter()
                .any(|passed| name.eq_ignore_ascii_case(passed))
        }) {
            env.push((name, value));
        }
    }
    (program, env)
}

/// `tool` in the directory holding `program`, when `program` is a path and it is there.
fn beside(program: &OsString, tool: &str) -> Option<PathBuf> {
    let tool = Path::new(program)
        .parent()?
        .join(tool)
        .with_extension(std::env::consts::EXE_EXTENSION);
    tool.is_file().then_some(tool)
}

/// What `cargo metadata` reports about the targets inside the project, with paths
/// relative to it and `/`-separated. Targets of a fixture lie outside the project and are
/// left out: nothing there is editable.
#[derive(Debug, Default)]
pub(crate) struct StarterTargets {
    /// Build scripts, which may sit at custom paths.
    pub(crate) build_scripts: BTreeSet<String>,
    /// Test targets by name, with the source of each target that has that name.
    pub(crate) tests: BTreeMap<String, Vec<String>>,
}

/// Reads [`StarterTargets`] from the full `cargo metadata` output in `log`.
///
/// # Errors
///
/// When the log cannot be read or is not the metadata Cargo documents.
pub(crate) fn starter_targets(log: &Path, project: &Path) -> Result<StarterTargets, GradeError> {
    let metadata = read_metadata(log)?;
    let canonical_project = project.canonicalize().map_err(|source| GradeError::Read {
        path: project.to_owned(),
        source,
    })?;
    let mut targets = StarterTargets::default();
    for target in metadata
        .packages
        .iter()
        .flat_map(|package| &package.targets)
    {
        let is_build_script = target.kind.iter().any(|kind| kind == "custom-build");
        let is_test = target.kind.iter().any(|kind| kind == "test");
        if !is_build_script && !is_test {
            continue;
        }
        let Some(relative) = project_path(&target.src_path, project, &canonical_project)? else {
            continue;
        };
        if is_build_script {
            targets.build_scripts.insert(relative);
        } else {
            targets
                .tests
                .entry(target.name.clone())
                .or_default()
                .push(relative);
        }
    }
    Ok(targets)
}

/// The full `cargo metadata` output Cargo wrote to `log`.
fn read_metadata(log: &Path) -> Result<Metadata, GradeError> {
    let text = fs::read(log).map_err(|source| GradeError::Read {
        path: log.to_owned(),
        source,
    })?;
    serde_json::from_slice(&text).map_err(GradeError::Metadata)
}

/// `source` relative to the project and `/`-separated, or `None` when it lies outside.
fn project_path(
    source: &Path,
    project: &Path,
    canonical_project: &Path,
) -> Result<Option<String>, GradeError> {
    let relative = match source.canonicalize() {
        Ok(resolved) => resolved.strip_prefix(canonical_project).map(Path::to_owned),
        // A target the manifest declares before its file exists is still reported, at the
        // path Cargo joined onto the project.
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            source.strip_prefix(project).map(Path::to_owned)
        }
        Err(error) => {
            return Err(GradeError::Read {
                path: source.to_owned(),
                source: error,
            });
        }
    };
    Ok(relative.ok().map(|relative| {
        relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    }))
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn the_environment_is_built_not_inherited() {
        let dir = TempDir::new().unwrap();
        let tools = dir.path().join("toolchain/bin");
        fs::create_dir_all(&tools).unwrap();
        let exe = |name: &str| {
            tools
                .join(name)
                .with_extension(std::env::consts::EXE_EXTENSION)
        };
        for tool in ["cargo", "rustc", "rustdoc"] {
            fs::write(exe(tool), "").unwrap();
        }
        let inherited = [
            ("CARGO", exe("cargo").into_os_string()),
            ("Path", "/usr/bin".into()),
            ("HOME", "/home/someone".into()),
            ("RUSTC", "/hostile/rustc".into()),
            ("RUSTFLAGS", "-Cpanic=abort".into()),
            ("CARGO_BUILD_TARGET", "wasm32-unknown-unknown".into()),
            ("CARGO_BUILD_RUSTC_WRAPPER", "sccache".into()),
            (
                "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS",
                "-Cdebug-assertions=off".into(),
            ),
            ("CARGO_PROFILE_DEV_OPT_LEVEL", "3".into()),
            ("CARGO_TARGET_DIR", "/developer/target".into()),
        ]
        .map(|(name, value): (&str, OsString)| (OsString::from(name), value));

        let cargo = Cargo::with_environment(dir.path().join("project"), dir.path(), inherited);
        let invocation = cargo.metadata("metadata");

        assert_eq!(invocation.program, exe("cargo").into_os_string());
        let env: Vec<(String, String)> = invocation
            .env
            .iter()
            .map(|(name, value)| {
                (
                    name.to_string_lossy().into_owned(),
                    value.to_string_lossy().into_owned(),
                )
            })
            .collect();
        let path = |path: PathBuf| path.to_string_lossy().into_owned();
        assert_eq!(
            env,
            [
                ("Path".to_owned(), "/usr/bin".to_owned()),
                ("HOME".to_owned(), "/home/someone".to_owned()),
                (
                    "CARGO_TARGET_DIR".to_owned(),
                    path(dir.path().join("target"))
                ),
                ("CARGO_HOME".to_owned(), path(dir.path().join("cargo-home"))),
                ("CARGO_INCREMENTAL".to_owned(), "0".to_owned()),
                ("CARGO_NET_OFFLINE".to_owned(), "true".to_owned()),
                ("RUSTC".to_owned(), path(exe("rustc"))),
                ("RUSTDOC".to_owned(), path(exe("rustdoc"))),
            ]
        );
    }

    #[test]
    fn build_scripts_and_tests_inside_the_project_are_found_relative_to_it() {
        let dir = TempDir::new().unwrap();
        let project = dir.path().join("project");
        let fixture = dir.path().join("fixtures/store");
        fs::create_dir_all(project.join("tools")).unwrap();
        fs::create_dir_all(project.join("tests")).unwrap();
        fs::create_dir_all(&fixture).unwrap();
        for file in ["tools/generate.rs", "tests/works.rs", "main.rs"] {
            fs::write(project.join(file), "").unwrap();
        }
        fs::write(fixture.join("build.rs"), "").unwrap();
        let metadata = serde_json::json!({
            "packages": [
                { "targets": [
                    { "name": "demo", "kind": ["lib"], "src_path": project.join("main.rs") },
                    { "name": "build-script-build", "kind": ["custom-build"], "src_path": project.join("tools/generate.rs") },
                    { "name": "works", "kind": ["test"], "src_path": project.join("tests/works.rs") },
                ] },
                { "targets": [
                    { "name": "build-script-build", "kind": ["custom-build"], "src_path": fixture.join("build.rs") },
                ] },
            ],
        });
        let log = dir.path().join("metadata.stdout.log");
        fs::write(&log, metadata.to_string()).unwrap();

        let targets = starter_targets(&log, &project).unwrap();

        assert_eq!(
            targets.build_scripts,
            BTreeSet::from(["tools/generate.rs".to_owned()])
        );
        assert_eq!(
            targets.tests,
            BTreeMap::from([("works".to_owned(), vec!["tests/works.rs".to_owned()])])
        );
    }
}
