//! The Cargo commands grading runs.
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

use std::ffi::OsString;
use std::path::{Path, PathBuf};

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

#[cfg(test)]
mod tests {
    use std::fs;

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
}
