//! Everything preparing or launching a trial can report.

use std::env::JoinPathsError;
use std::io;
use std::path::PathBuf;
use std::process::ExitStatus;

use thiserror::Error;

use crate::digest::DigestError;
use crate::grade::{ImportError, ProcessError};
use crate::task::TaskErrors;

#[derive(Debug, Error)]
pub(crate) enum RunError {
    #[error("`{}` is not an absolute path", .0.display())]
    NotAbsolute(PathBuf),
    #[error("cannot resolve `{}`", .path.display())]
    Canonicalize {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("`{}` is not a Rust sysroot: it has no `bin/cargo` and `lib/rustlib`", .0.display())]
    NotToolchain(PathBuf),
    #[error("`{}` lies inside `{}`", .inner.display(), .outer.display())]
    Nested { inner: PathBuf, outer: PathBuf },
    #[error("cannot create `{}`", .path.display())]
    Create {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cannot copy the task into the trial")]
    Copy(#[source] ImportError),
    #[error("cannot copy the task's inputs into the batch")]
    Snapshot(#[source] ImportError),
    #[error("the inputs are batch `{}`'s, not this batch's", .0.display())]
    ForeignInputs(PathBuf),
    #[error("the task is not valid")]
    Task(#[source] TaskErrors),
    #[error("cannot hash the task")]
    Digest(#[source] DigestError),
    #[error("cannot write the `srt` policy")]
    Policy(#[source] serde_json::Error),
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "--exploratory needs its own design")
    )]
    #[error("cannot write the client's sandbox settings")]
    ClientSettings(#[source] serde_json::Error),
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "--exploratory needs its own design")
    )]
    #[error("`{}` would be read as a pattern by the client's sandbox", .0.display())]
    PatternInPath(PathBuf),
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "--exploratory needs its own design")
    )]
    #[error("`{}` is not valid Unicode, which the client's config needs", .0.display())]
    NotUnicode(PathBuf),
    #[error("`{}` is a baseline trial, which has no Symposium to set up", .0.display())]
    NotSymposium(PathBuf),
    #[error("cannot write the Symposium config")]
    SymposiumConfig(#[source] toml::ser::Error),
    #[error("a tool named `{0}` is already staged")]
    StagedTwice(String),
    #[error("`{}` is not a file inside its package", .0.display())]
    ToolEntry(PathBuf),
    #[error("cannot stage a tool")]
    Stage(#[source] ImportError),
    #[error("cannot link `{}` to a staged tool", .path.display())]
    Link {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cannot build `PATH` for the trial")]
    PathList(#[source] JoinPathsError),
    #[error("`{}` has not been written, so the trial cannot run", .0.display())]
    NoPolicy(PathBuf),
    #[error(
        "Cargo would read `{}` while grading, which could change how the submission builds; choose an evaluation root beneath no readable Cargo configuration",
        .0.display()
    )]
    AmbientCargoConfig(PathBuf),
    #[error("cannot check `{}` for a Cargo configuration", .path.display())]
    CargoConfigCheck {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(
        "`srt` reports version `{found}`, but the runner is pinned to {}",
        super::SRT_VERSION
    )]
    SrtVersion { found: String },
    #[error("`srt` needs Node 22.12 or later, but `node --version` reports `{found}`")]
    NodeTooOld { found: String },
    #[error(
        "the runner needs Claude Code {} or later, but `claude --version` reports `{found}`",
        crate::client::claude::version::MINIMUM
    )]
    ClaudeVersion { found: String },
    #[error("`{tool}` exited with {status}: {stderr}")]
    ToolFailed {
        tool: &'static str,
        status: ExitStatus,
        stderr: String,
    },
    #[error("`{tool}` did not finish in time")]
    ToolTimeout {
        tool: &'static str,
        /// What went wrong after the deadline while stopping or collecting the tool.
        #[source]
        after_deadline: Option<ProcessError>,
    },
    #[error("cannot run `{tool}`")]
    Process {
        tool: &'static str,
        #[source]
        source: ProcessError,
    },
    #[error("cannot inspect `{}` before launch", .path.display())]
    Inspect {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("`{}` {why}", .path.display())]
    NotReady { path: PathBuf, why: Unready },
}

/// What a pre-launch check found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum Unready {
    #[error("holds a skill before the agent starts")]
    SkillInstalled,
    #[error("holds a sync record before the agent starts")]
    SyncRecord,
    #[error("holds a hook trace before the agent starts")]
    HookTrace,
    #[error("holds a hook record before the agent starts")]
    HookRecord,
    #[error("holds managed settings, which override the trial's own")]
    ManagedSettings,
    #[error("puts `cargo-agents` on a baseline trial's `PATH`")]
    CargoAgentsOnPath,
    #[error("is a Symposium config, in a baseline trial")]
    SymposiumConfig,
    #[error("runs `cargo-agents`, in a baseline trial")]
    SymposiumEntry,
}
