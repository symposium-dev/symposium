//! The `cargo-agents` under evaluation: built from the checkout, or the binary given with
//! `--symposium-bin`. A result names the staged copy, which is what trials run, by its
//! version and hash, and by the commit it was built from only when that commit describes
//! it: the checkout's `HEAD` with nothing changed or added. A given binary's commit is
//! unknown.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};

use super::host;
use crate::result::SymposiumBuild;
use crate::run::stage::StagedTool;

/// The binary to stage, and the commit it was built from, when known.
#[derive(Debug)]
pub(crate) struct UnderEvaluation {
    pub(crate) binary: PathBuf,
    pub(crate) revision: Option<String>,
}

/// `given`, or a release build from `checkout`.
pub(crate) fn under_evaluation(
    checkout: &Path,
    given: Option<&Path>,
) -> anyhow::Result<UnderEvaluation> {
    Ok(match given {
        Some(given) => UnderEvaluation {
            binary: host::resolved(given)?,
            revision: None,
        },
        None => UnderEvaluation {
            binary: build(checkout)?,
            revision: clean_revision(checkout),
        },
    })
}

/// What a result records of `staged`. `scratch` is the home its `--version` runs with,
/// so it reads none of the developer's configuration.
pub(crate) fn recorded(
    staged: &StagedTool,
    revision: Option<String>,
    scratch: &Path,
) -> anyhow::Result<SymposiumBuild> {
    Ok(SymposiumBuild {
        version: version(staged.command(), scratch)?,
        sha256: staged
            .digest()
            .context("cannot hash the staged cargo-agents")?,
        revision,
    })
}

/// Builds `cargo-agents` from `checkout`, and returns where Cargo put it.
fn build(checkout: &Path) -> anyhow::Result<PathBuf> {
    let output = Command::new(env!("CARGO"))
        .args(["build", "--release", "--locked", "--quiet"])
        .args(["--package", "symposium", "--bin", "cargo-agents"])
        .arg("--message-format=json-render-diagnostics")
        .current_dir(checkout)
        .output()
        .context("cannot run cargo")?;
    if !output.status.success() {
        bail!(
            "cannot build cargo-agents:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|message| message["target"]["name"] == "cargo-agents")
        .and_then(|message| message["executable"].as_str().map(PathBuf::from))
        .context("cargo did not report the cargo-agents executable")
}

/// `HEAD`, when nothing in `checkout` is changed or untracked; otherwise the build is
/// not that commit's, so none is named.
fn clean_revision(checkout: &Path) -> Option<String> {
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(checkout)
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8(output.stdout).ok())
            .flatten()
    };
    let status = git(&["status", "--porcelain"])?;
    if !status.trim().is_empty() {
        return None;
    }
    git(&["rev-parse", "HEAD"]).map(|head| head.trim().to_owned())
}

/// `binary --version`, run outside any sandbox with `scratch` as its home and nothing
/// else in its environment.
fn version(binary: &Path, scratch: &Path) -> anyhow::Result<String> {
    let output = Command::new(binary)
        .arg("--version")
        .env_clear()
        .env("HOME", scratch)
        .env("PATH", "/usr/bin:/bin")
        .current_dir(scratch)
        .output()
        .with_context(|| format!("cannot run `{} --version`", binary.display()))?;
    if !output.status.success() {
        bail!("`{} --version` failed", binary.display());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    struct Repository {
        root: TempDir,
    }

    impl Repository {
        fn committed() -> Self {
            let repository = Self {
                root: TempDir::new().unwrap(),
            };
            repository.git(&["init", "--quiet"]);
            repository.write("lib.rs", "");
            repository.git(&["add", "."]);
            repository.git(&["commit", "--quiet", "-m", "start"]);
            repository
        }

        fn path(&self) -> &Path {
            self.root.path()
        }

        fn write(&self, path: &str, contents: &str) {
            fs::write(self.path().join(path), contents).unwrap();
        }

        fn head(&self) -> String {
            self.git_output(&["rev-parse", "HEAD"])
        }

        fn git(&self, args: &[&str]) {
            let status = self.git_command(args).status().unwrap();
            assert!(status.success(), "git {args:?}");
        }

        fn git_output(&self, args: &[&str]) -> String {
            let output = self.git_command(args).output().unwrap();
            assert!(output.status.success(), "git {args:?}");
            String::from_utf8(output.stdout).unwrap().trim().to_owned()
        }

        fn git_command(&self, args: &[&str]) -> Command {
            let mut command = Command::new("git");
            command
                .args(["-c", "user.name=test", "-c", "user.email=test@example.com"])
                .args(args)
                .current_dir(self.path());
            command
        }
    }

    #[test]
    fn a_clean_checkout_names_its_commit() {
        let repository = Repository::committed();

        let revision = clean_revision(repository.path());

        assert_eq!(revision.as_deref(), Some(repository.head().as_str()));
    }

    #[test]
    fn a_changed_checkout_names_no_commit() {
        let repository = Repository::committed();
        repository.write("lib.rs", "changed");

        let revision = clean_revision(repository.path());

        assert_eq!(revision, None);
    }

    #[test]
    fn an_untracked_file_names_no_commit() {
        let repository = Repository::committed();
        repository.write("new.rs", "");

        let revision = clean_revision(repository.path());

        assert_eq!(revision, None);
    }
}
