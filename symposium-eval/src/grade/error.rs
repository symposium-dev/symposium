//! Everything grading can report.

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
