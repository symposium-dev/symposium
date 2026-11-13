//! Where a trial's submission is graded, under `srt` (RFD reference, "Grading the code").
//!
//! What the agent wrote runs here, as tests, so grading gets a sandbox of its own: it
//! reads the trusted sources and the toolchain beside the system, writes only `work/`,
//! which the runner empties before each step, and reaches no network, no credentials, no
//! home, and no other part of the batch, its own logs included.
//!
//! The submission's code runs in every step, so nothing a step can write survives into
//! the next. Within one step it runs in the same process as the check, before `main` too
//! when the check is listed, so it could forge that step's output; the sandbox bounds
//! where, not whether, a submission misbehaves.
//!
//! Cargo also reads `.cargo/config.toml` in every directory above the copy, up to the
//! root. Those between the copy and the evaluation root are the runner's; above it, one
//! the sandbox would let Cargo read could change how the submission builds, so grading
//! refuses to run beneath it.

#[cfg(all(test, target_os = "linux"))]
mod real;
#[cfg(test)]
mod tests;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{Machine, Policy, RunError, Trial, create_dirs};
use crate::grade::GradingDirs;

/// The names Cargo reads its configuration from, in each directory it searches.
const CARGO_CONFIGS: [&str; 2] = [".cargo/config", ".cargo/config.toml"];

impl Trial {
    /// Everything grading reads or writes, and what the runner writes for it.
    pub(crate) fn grading(&self) -> PathBuf {
        self.root().join("grading")
    }

    /// The trusted project and fixtures, read-only to the submission.
    pub(crate) fn grading_sources(&self) -> PathBuf {
        self.grading().join("sources")
    }

    /// The commands' writable state, emptied before each step.
    pub(crate) fn grading_work(&self) -> PathBuf {
        self.grading().join("work")
    }

    /// Grading's own `/tmp/claude` and `TMPDIR`, inside [`Self::grading_work`], so it is
    /// emptied with it.
    pub(crate) fn grading_tmp(&self) -> PathBuf {
        self.grading_work().join("tmp")
    }

    /// The output of each command grading runs, which the runner writes from outside.
    pub(crate) fn grading_logs(&self) -> PathBuf {
        self.grading().join("logs")
    }

    /// The `srt` settings grading runs under.
    pub(crate) fn grading_policy_file(&self) -> PathBuf {
        self.root().join("srt-grading.json")
    }

    /// Writes grading's policy, so grading can run, and says where it puts each part.
    ///
    /// # Errors
    ///
    /// As [`Policy::for_grading`], when Cargo would read a configuration above the
    /// sources, or when the policy cannot be written.
    pub(crate) fn prepare_grading(&self, machine: &Machine) -> Result<GradingDirs, RunError> {
        let policy = Policy::for_grading(self, machine)?;
        if let Some(config) = ambient_cargo_config(&self.grading_sources(), &policy)? {
            return Err(RunError::AmbientCargoConfig(config));
        }
        create_dirs([self.grading()])?;
        policy.write(&self.grading_policy_file())?;
        Ok(GradingDirs {
            sources: self.grading_sources(),
            work: self.grading_work(),
            logs: self.grading_logs(),
        })
    }
}

/// The first Cargo configuration in a directory above `sources` that `policy` lets a
/// sandboxed command read. One a denied directory holds, such as a home's, is hidden.
fn ambient_cargo_config(sources: &Path, policy: &Policy) -> Result<Option<PathBuf>, RunError> {
    for dir in sources.ancestors() {
        for name in CARGO_CONFIGS {
            let config = dir.join(name);
            if exists(&config)? && policy.can_read(&config) {
                return Ok(Some(config));
            }
        }
    }
    Ok(None)
}

/// Whether anything is at `path`, a link included.
fn exists(path: &Path) -> Result<bool, RunError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(RunError::CargoConfigCheck {
            path: path.to_owned(),
            source,
        }),
    }
}
