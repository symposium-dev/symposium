//! Everything that can stop a session from ending well, by whose doing.

use std::io;
use std::path::PathBuf;
use std::process::ExitStatus;

use thiserror::Error;

/// Why a session failed, by the client's own doing.
#[derive(Debug, Error)]
pub(crate) enum ClientError {
    #[error("the client exited with {status}: {stderr}")]
    Exited { status: ExitStatus, stderr: String },
    #[error("the client reported an error: {0}")]
    Reported(String),
    #[error("the client's transcript has no completion event")]
    NoCompletion,
}

/// Why a transcript cannot say how its session ended. Neither is the client's failure.
#[derive(Debug, Error)]
pub(crate) enum TranscriptError {
    /// The runner could not read what the backend wrote.
    #[error("cannot read the transcript `{}`", .path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// An event the adapter needs is not in a shape it knows, so the client's format may
    /// have changed.
    #[error("line {line} of the transcript: {reason}")]
    Malformed { line: usize, reason: String },
}
