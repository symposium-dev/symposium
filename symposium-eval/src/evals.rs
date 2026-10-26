//! The `evals/` directory layout from the RFD reference, in one place.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::task::{CheckName, FixtureName, NameError, PluginName, TaskName};

#[derive(Debug, Clone)]
pub(crate) struct Evals {
    root: PathBuf,
}

/// One entry of `tasks/`, kept even when it is not a usable task so `--check-tasks` can
/// report it instead of skipping it.
#[derive(Debug)]
pub(crate) struct TaskEntry {
    /// The entry's file name, lossily decoded for display only.
    pub(crate) entry: String,
    pub(crate) task: Result<TaskName, EntryError>,
}

#[derive(Debug, Error)]
pub(crate) enum EntryError {
    #[error("the name is not valid UTF-8")]
    NonUtf8,
    #[error("only task directories belong in `tasks/`")]
    NotADirectory,
    #[error("it is a symlink; task directories must be real directories")]
    Symlink,
    #[error(transparent)]
    Name(#[from] NameError),
}

impl Evals {
    pub(crate) fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The checkout's own `evals/`, next to this crate.
    ///
    /// # Panics
    ///
    /// Never in practice: the crate directory always has a parent.
    pub(crate) fn in_repository() -> Self {
        let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let repository = crate_dir
            .parent()
            .expect("symposium-eval lives inside the repository");
        Self::new(repository.join("evals"))
    }

    pub(crate) fn tasks_dir(&self) -> PathBuf {
        self.root.join("tasks")
    }

    pub(crate) fn task_dir(&self, name: &TaskName) -> PathBuf {
        self.tasks_dir().join(name.as_str())
    }

    pub(crate) fn starter_dir(&self, name: &TaskName) -> PathBuf {
        self.task_dir(name).join("starter")
    }

    pub(crate) fn check_file(&self, name: &TaskName, check: &CheckName) -> PathBuf {
        self.task_dir(name)
            .join("checks")
            .join(format!("{check}.rs"))
    }

    pub(crate) fn solution_dir(&self, name: &TaskName) -> PathBuf {
        self.task_dir(name).join("solution")
    }

    pub(crate) fn plugin_dir(&self, name: &PluginName) -> PathBuf {
        self.root.join("plugins").join(name.as_str())
    }

    pub(crate) fn fixture_dir(&self, name: &FixtureName) -> PathBuf {
        self.root.join("fixtures").join(name.as_str())
    }

    /// Every entry of `tasks/`, sorted by name. A missing `tasks/` holds no tasks.
    ///
    /// # Errors
    ///
    /// When `tasks/` exists but cannot be listed.
    pub(crate) fn task_entries(&self) -> io::Result<Vec<TaskEntry>> {
        let entries = match fs::read_dir(self.tasks_dir()) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut tasks = Vec::new();
        for entry in entries {
            let entry = entry?;
            // `DirEntry::file_type` does not follow links, so a symlink is reported, not lost.
            let file_type = entry.file_type()?;
            let file_name = entry.file_name();
            tasks.push(TaskEntry {
                entry: file_name.to_string_lossy().into_owned(),
                task: entry_task(file_type, &file_name),
            });
        }
        tasks.sort_by(|left, right| left.entry.cmp(&right.entry));
        Ok(tasks)
    }
}

/// The task an entry of `tasks/` names, if it is a real directory with a usable name.
fn entry_task(file_type: fs::FileType, file_name: &OsStr) -> Result<TaskName, EntryError> {
    if file_type.is_symlink() {
        Err(EntryError::Symlink)
    } else if !file_type.is_dir() {
        Err(EntryError::NotADirectory)
    } else if let Some(name) = file_name.to_str() {
        TaskName::new(name).map_err(EntryError::from)
    } else {
        Err(EntryError::NonUtf8)
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn summary(entries: &[TaskEntry]) -> Vec<(String, String)> {
        entries
            .iter()
            .map(|entry| {
                let outcome = match &entry.task {
                    Ok(name) => format!("task {name}"),
                    Err(error) => error.to_string(),
                };
                (entry.entry.clone(), outcome)
            })
            .collect()
    }

    #[test]
    fn task_entries_are_sorted_and_report_stray_files() {
        let root = TempDir::new().unwrap();
        fs::create_dir_all(root.path().join("tasks/zeta")).unwrap();
        fs::create_dir_all(root.path().join("tasks/alpha")).unwrap();
        fs::write(root.path().join("tasks/notes.md"), "").unwrap();

        let entries = Evals::new(root.path()).task_entries().unwrap();

        assert_eq!(
            summary(&entries),
            [
                ("alpha".to_owned(), "task alpha".to_owned()),
                (
                    "notes.md".to_owned(),
                    "only task directories belong in `tasks/`".to_owned()
                ),
                ("zeta".to_owned(), "task zeta".to_owned()),
            ]
        );
    }

    // Creating a symlink on Windows needs developer mode or elevation.
    #[cfg(unix)]
    #[test]
    fn symlinked_task_directory_is_reported() {
        let root = TempDir::new().unwrap();
        fs::create_dir_all(root.path().join("tasks/real")).unwrap();
        std::os::unix::fs::symlink(
            root.path().join("tasks/real"),
            root.path().join("tasks/link"),
        )
        .unwrap();

        let entries = Evals::new(root.path()).task_entries().unwrap();

        assert_eq!(
            summary(&entries),
            [
                (
                    "link".to_owned(),
                    "it is a symlink; task directories must be real directories".to_owned()
                ),
                ("real".to_owned(), "task real".to_owned()),
            ]
        );
    }

    #[test]
    fn missing_tasks_directory_holds_no_tasks() {
        let root = TempDir::new().unwrap();

        let entries = Evals::new(root.path()).task_entries().unwrap();

        assert!(entries.is_empty());
    }
}
