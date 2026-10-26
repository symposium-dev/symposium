//! Everything grading can report.
//!
//! An error here is never a verdict on the code under grading. [`ImportError`] marks the
//! submission invalid when [`ImportError::invalidates_submission`] says so; every other
//! error is an infrastructure failure, which excludes the trial instead of failing it.

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;

use thiserror::Error;

use crate::tree::{EntryKind, WalkError};

#[derive(Debug, Error)]
pub(crate) enum ImportError {
    #[error("the submission's `{path}` is a {kind} under an editable path")]
    UnsupportedEntry { path: String, kind: EntryKind },
    #[error("the submission is a {0}, not a directory")]
    SubmissionRoot(EntryKind),
    #[error("cannot list the submission")]
    Submission(#[source] WalkError),
    #[error("`{tree}/{path}` is a {kind}; only files and directories are copied")]
    UntrustedEntry {
        tree: String,
        path: String,
        kind: EntryKind,
    },
    #[error("cannot list `{tree}`")]
    Trusted {
        tree: String,
        #[source]
        source: WalkError,
    },
    #[error("cannot read `{}`", .path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cannot write `{}`", .path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("`{path}` would resolve outside the grading copy")]
    Escape { path: String },
}

/// Why a command could not be run to an outcome.
#[derive(Debug, Error)]
pub(crate) enum ProcessError {
    #[error("cannot start `{}`", .program.to_string_lossy())]
    Spawn {
        program: OsString,
        #[source]
        source: io::Error,
    },
    #[error("cannot create the log `{}`", .path.display())]
    Log {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cannot wait for the command")]
    Wait(#[source] io::Error),
    #[error("cannot stop the command at its deadline")]
    Kill(#[source] io::Error),
    #[cfg(windows)]
    #[error("cannot contain the command's process tree in a job object")]
    Contain(#[source] io::Error),
    #[error("cannot start a thread to read the command's output")]
    Thread(#[source] io::Error),
    #[error("cannot collect the command's output")]
    Output(#[source] io::Error),
    #[error("the command's output stayed open after it ended")]
    OutputNotClosed,
}

/// Why grading could not reach a verdict.
#[derive(Debug, Error)]
pub(crate) enum GradeError {
    #[error(transparent)]
    Import(#[from] ImportError),
    #[error("cannot run `{step}`")]
    Process {
        step: String,
        #[source]
        source: ProcessError,
    },
    #[error("`{step}` did not finish in time")]
    TimedOut { step: String },
    #[error("`{step}` failed on the trusted starter: {stderr}")]
    Preflight { step: String, stderr: String },
    #[error("cannot read `{}`", .path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("`cargo metadata` printed something other than its documented format")]
    Metadata(#[source] serde_json::Error),
}

impl ImportError {
    /// Whether the submission, not the task or the machine, is at fault.
    pub(crate) fn invalidates_submission(&self) -> bool {
        matches!(
            self,
            Self::UnsupportedEntry { .. } | Self::SubmissionRoot(_) | Self::Submission(_)
        )
    }
}
