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

/// Why a transcript is not this trial's session, from the client checked before launch.
/// The client launched, so this is an isolation exclusion, not a setup failure.
#[derive(Debug, Error)]
pub(crate) enum ProvenanceError {
    #[error("the transcript is session `{found}`, not `{requested}`, which the runner started")]
    Session { requested: String, found: String },
    #[error("the transcript's client is version `{found}`, not `{checked}`, checked before launch")]
    Version { checked: String, found: String },
    #[error("the transcript ends without saying which session or client it is")]
    Unstated,
}

/// Why the runner cannot take a transcript as how its session ended.
#[derive(Debug, Error)]
pub(crate) enum Unusable {
    #[error(transparent)]
    Transcript(#[from] TranscriptError),
    #[error(transparent)]
    Provenance(#[from] ProvenanceError),
}
