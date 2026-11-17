//! Everything `--check-tasks` can report about one task.

use std::error::Error as _;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

use super::{
    CheckError, CheckName, EditablePattern, FixtureName, HookName, PluginName, SkillList, SkillName,
};
use crate::evals::EntryError;
use crate::grade::{GradeError, OutsideEdit, Step};
use crate::skill_md::Unloadable;
use crate::tree::{EntryKind, WalkError};

/// Why a path that must be a real directory is not one. A failure to inspect the path
/// is an I/O error instead, kept apart from what the task author wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirProblem {
    Missing,
    Symlink,
    NotADirectory,
}

#[derive(Debug, Error)]
pub(crate) enum TaskError {
    #[error(transparent)]
    Entry(#[from] EntryError),
    #[error("cannot read `task.toml`")]
    Read(#[source] io::Error),
    #[error("invalid `task.toml`")]
    Parse(#[source] toml::de::Error),
    #[error("cannot inspect `{}`", .path.display())]
    Inspect {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("`prompt` is empty")]
    EmptyPrompt,
    #[error("`starter/` {0}")]
    Starter(DirProblem),
    #[error("cannot list `starter/`")]
    StarterTree(#[source] WalkError),
    #[error("`starter/` has no `Cargo.lock`; grading runs Cargo with `--locked`")]
    MissingLockfile,
    #[error(
        "editable pattern `{pattern}` matches `starter/{path}`, which grading always takes from the starter"
    )]
    ProtectedEditable {
        pattern: EditablePattern,
        path: String,
    },
    #[error("plugin `{name}` in evals/plugins/ {problem}")]
    Plugin {
        name: PluginName,
        problem: DirProblem,
    },
    #[error("fixture `{name}` in evals/fixtures/ {problem}")]
    Fixture {
        name: FixtureName,
        problem: DirProblem,
    },
    #[error(transparent)]
    Check(#[from] CheckError),
    #[error(
        "`starter/tests/{0}.rs` has the name of the hidden check `{0}`, which grading puts there"
    )]
    CheckCollision(CheckName),
    #[error("`solution/` {0}")]
    SolutionDir(DirProblem),
    #[error("cannot grade `solution/`")]
    SolutionGrading(#[source] GradeError),
    #[error("`solution/` {0}, outside `editable`; grading ignores it")]
    SolutionOutsideEditable(OutsideEdit),
    #[error("`solution/` {0}")]
    SolutionStep(Step),
    #[error("invalid `Symposium.toml` in plugin `{plugin}`")]
    PluginManifest {
        plugin: PluginName,
        #[source]
        source: toml::de::Error,
    },
    #[error("expected hook `{0}` is not declared by any of the task's plugins")]
    UndeclaredHook(HookName),
    #[error("expected hook `{hook}` is declared by both `{first}` and `{second}`")]
    AmbiguousHook {
        hook: HookName,
        first: PluginName,
        second: PluginName,
    },
    #[error(
        "expected hook `{hook}` in plugin `{plugin}` is limited to one agent by `agent` or `format`, but a task runs on every client"
    )]
    AgentLimitedHook { hook: HookName, plugin: PluginName },
    #[error("expected skill `{0}` is not provided by any of the task's plugins")]
    UnprovidedSkill(SkillName),
    #[error(
        "expected skill `{skill}` is provided by both `{}` and `{}`",
        .first.display(),
        .second.display()
    )]
    AmbiguousSkill {
        skill: SkillName,
        first: PathBuf,
        second: PathBuf,
    },
    #[error(
        "a `[[skills]]` group in plugin `{0}` needs a `source.path`, the only source an offline trial can use"
    )]
    UnsupportedSkillSource(PluginName),
    #[error(
        "skill source `{path}` in plugin `{plugin}` leaves the plugin, which is all a trial copies"
    )]
    EscapingSkillSource { plugin: PluginName, path: String },
    #[error("skill source `{path}` in plugin `{plugin}` is not a directory in it")]
    MissingSkillSource { plugin: PluginName, path: String },
    #[error("cannot list plugin `{plugin}`")]
    PluginTree {
        plugin: PluginName,
        #[source]
        source: WalkError,
    },
    #[error("plugin `{plugin}` holds a {kind} at `{path}`, which a trial cannot copy")]
    PluginEntry {
        plugin: PluginName,
        path: String,
        kind: EntryKind,
    },
    #[error("`{}` in plugin `{plugin}` would not load: {problem}", .path.display())]
    UnloadableSkill {
        plugin: PluginName,
        path: PathBuf,
        problem: Unloadable,
    },
    #[error("`{list}` lists `{value}` more than once")]
    Duplicate { list: &'static str, value: String },
    #[error("skill `{skill}` is in both `{first}` and `{second}`")]
    Contradiction {
        skill: SkillName,
        first: SkillList,
        second: SkillList,
    },
}

/// Every problem found in one task, in the order the rules ran.
#[derive(Debug)]
pub(crate) struct TaskErrors(pub(super) Vec<TaskError>);

impl DirProblem {
    /// `Ok(None)` when `path` is a real directory. Links are not followed.
    ///
    /// # Errors
    ///
    /// When `path` exists but cannot be inspected.
    pub(crate) fn of(path: &Path) -> io::Result<Option<Self>> {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => Ok(Some(Self::Symlink)),
            Ok(metadata) if metadata.is_dir() => Ok(None),
            Ok(_) => Ok(Some(Self::NotADirectory)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Some(Self::Missing)),
            Err(error) => Err(error),
        }
    }
}

impl fmt::Display for DirProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => f.write_str("does not exist"),
            Self::Symlink => f.write_str("is a symlink; it must be a real directory"),
            Self::NotADirectory => f.write_str("is not a directory"),
        }
    }
}

#[cfg(test)]
impl TaskErrors {
    pub(super) fn as_slice(&self) -> &[TaskError] {
        &self.0
    }
}

impl From<TaskError> for TaskErrors {
    fn from(error: TaskError) -> Self {
        Self(vec![error])
    }
}

impl From<Vec<TaskError>> for TaskErrors {
    fn from(errors: Vec<TaskError>) -> Self {
        Self(errors)
    }
}

/// One error per line, each followed by its causes, so a TOML error keeps its line and
/// column.
impl fmt::Display for TaskErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, error) in self.0.iter().enumerate() {
            if index > 0 {
                writeln!(f)?;
            }
            write!(f, "{error}")?;
            let mut cause = error.source();
            while let Some(inner) = cause {
                write!(f, ": {inner}")?;
                cause = inner.source();
            }
        }
        Ok(())
    }
}

impl std::error::Error for TaskErrors {}
