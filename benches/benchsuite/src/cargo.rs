//! Cargo process guards used by benchmark preflights.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
#[cfg(not(windows))]
use indoc::indoc;

/// Marker written beside the guard the first time `metadata` is refused.
const MARKER_FILE: &str = "metadata-attempted";

#[cfg(not(windows))]
const METADATA_REJECTING_SCRIPT: &str = indoc! {r#"
    #!/bin/sh
    if [ "$1" = "metadata" ]; then
        : > "${0%/*}/metadata-attempted"
        exit 1
    fi
    exec cargo "$@"
"#};

// A `goto` rather than a parenthesised block: batch parses blocks eagerly.
#[cfg(windows)]
const METADATA_REJECTING_SCRIPT: &str = concat!(
    "@echo off\r\n",
    "if not \"%~1\"==\"metadata\" goto forward\r\n",
    "type nul > \"%~dp0metadata-attempted\"\r\n",
    "exit /b 1\r\n",
    ":forward\r\n",
    "cargo %*\r\n",
);

/// A Cargo executable that forwards every command except `metadata`, which it
/// refuses while recording the attempt.
///
/// Refusing is not itself an assertion: `WorkspaceDeps` turns a failed
/// `metadata` into "no workspace", which callers accept. Preflights have to
/// check [`saw_metadata`](Self::saw_metadata).
#[derive(Debug)]
pub struct MetadataRejectingCargo {
    executable: PathBuf,
    marker: PathBuf,
}

impl MetadataRejectingCargo {
    /// Create the guard under `parent`, which must not already contain one.
    pub fn create_in(parent: &Path) -> Result<Self> {
        let directory = parent.join("metadata-rejecting-cargo");
        fs::create_dir(&directory).with_context(|| {
            format!(
                "creating metadata-rejecting Cargo directory `{}`",
                directory.display()
            )
        })?;
        let executable = write_executable(&directory)?;

        Ok(Self {
            executable,
            marker: directory.join(MARKER_FILE),
        })
    }

    pub fn executable(&self) -> &Path {
        &self.executable
    }

    /// Whether the guard has been asked to run `cargo metadata`.
    pub fn saw_metadata(&self) -> Result<bool> {
        self.marker.try_exists().with_context(|| {
            format!(
                "checking the Cargo guard marker `{}`",
                self.marker.display()
            )
        })
    }
}

#[cfg(not(windows))]
fn write_executable(directory: &Path) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;

    let executable = directory.join("cargo");
    fs::write(&executable, METADATA_REJECTING_SCRIPT)
        .with_context(|| format!("writing Cargo guard `{}`", executable.display()))?;
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))
        .with_context(|| format!("making Cargo guard executable `{}`", executable.display()))?;

    Ok(executable)
}

#[cfg(windows)]
fn write_executable(directory: &Path) -> Result<PathBuf> {
    let executable = directory.join("cargo.cmd");
    fs::write(&executable, METADATA_REJECTING_SCRIPT)
        .with_context(|| format!("writing Cargo guard shim `{}`", executable.display()))?;

    Ok(executable)
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use anyhow::ensure;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn forwards_other_commands_and_rejects_metadata() -> Result<()> {
        let temporary_directory = tempdir()?;
        let cargo = MetadataRejectingCargo::create_in(temporary_directory.path())?;

        let forwarded = Command::new(cargo.executable())
            .arg("--version")
            .output()
            .context("running a forwarded Cargo command")?;
        ensure!(
            forwarded.status.success(),
            "Cargo guard did not forward `--version`: {}",
            String::from_utf8_lossy(&forwarded.stderr)
        );
        ensure!(
            !cargo.saw_metadata()?,
            "forwarding a command must not record a metadata attempt"
        );

        let rejected = Command::new(cargo.executable())
            .arg("metadata")
            .status()
            .context("running rejected Cargo metadata")?;
        ensure!(
            !rejected.success(),
            "Cargo guard unexpectedly allowed `metadata`"
        );
        ensure!(
            cargo.saw_metadata()?,
            "refusing `metadata` must record the attempt"
        );

        Ok(())
    }
}
