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

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::GradeError;
use super::process::Invocation;

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
    pub(crate) fn metadata(&self) -> Invocation {
        self.invocation(
            "metadata",
            &["metadata", "--format-version", "1", "--locked", "--offline"],
        )
    }

    pub(crate) fn build(&self) -> Invocation {
        self.invocation("build", &["build", "--locked", "--offline"])
    }

    pub(super) fn invocation(&self, log_name: &'static str, args: &[&str]) -> Invocation {
        Invocation {
            program: self.program.clone(),
            args: args.iter().map(Into::into).collect(),
            dir: self.project.clone(),
            env: self.env.clone(),
            log_name,
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

/// The project-relative, `/`-separated paths of every build script inside `project`,
/// read from the full `cargo metadata` output in `log`.
///
/// # Errors
///
/// When the log cannot be read or is not the metadata Cargo documents.
pub(crate) fn build_scripts(log: &Path, project: &Path) -> Result<BTreeSet<String>, GradeError> {
    let text = fs::read(log).map_err(|source| GradeError::Read {
        path: log.to_owned(),
        source,
    })?;
    let metadata: Metadata = serde_json::from_slice(&text).map_err(GradeError::Metadata)?;
    let project = project.canonicalize().map_err(|source| GradeError::Read {
        path: project.to_owned(),
        source,
    })?;
    let mut scripts = BTreeSet::new();
    for target in metadata
        .packages
        .iter()
        .flat_map(|package| &package.targets)
    {
        if !target.kind.iter().any(|kind| kind == "custom-build") {
            continue;
        }
        let script = target
            .src_path
            .canonicalize()
            .map_err(|source| GradeError::Read {
                path: target.src_path.clone(),
                source,
            })?;
        // A fixture's build script lies outside the project, where nothing is editable.
        if let Ok(relative) = script.strip_prefix(&project) {
            let components: Vec<_> = relative
                .components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect();
            scripts.insert(components.join("/"));
        }
    }
    Ok(scripts)
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
        let invocation = cargo.metadata();

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
                ("RUSTC".to_owned(), path(exe("rustc"))),
                ("RUSTDOC".to_owned(), path(exe("rustdoc"))),
            ]
        );
    }

    #[test]
    fn build_scripts_inside_the_project_are_found_relative_to_it() {
        let dir = TempDir::new().unwrap();
        let project = dir.path().join("project");
        let fixture = dir.path().join("fixtures/store");
        fs::create_dir_all(project.join("tools")).unwrap();
        fs::create_dir_all(&fixture).unwrap();
        fs::write(project.join("tools/generate.rs"), "").unwrap();
        fs::write(fixture.join("build.rs"), "").unwrap();
        fs::write(project.join("main.rs"), "").unwrap();
        let metadata = serde_json::json!({
            "packages": [
                { "targets": [
                    { "kind": ["lib"], "src_path": project.join("main.rs") },
                    { "kind": ["custom-build"], "src_path": project.join("tools/generate.rs") },
                ] },
                { "targets": [
                    { "kind": ["custom-build"], "src_path": fixture.join("build.rs") },
                ] },
            ],
        });
        let log = dir.path().join("metadata.stdout.log");
        fs::write(&log, metadata.to_string()).unwrap();

        let scripts = build_scripts(&log, &project).unwrap();

        assert_eq!(scripts, BTreeSet::from(["tools/generate.rs".to_owned()]));
    }
}
