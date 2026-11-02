//! The client's hook config and Symposium's, copied after `init` and compared after the
//! trial, since the agent can write them too and Claude Code applies a hook edit
//! mid-session (RFD reference, "What the evidence can and cannot show").

use std::path::{Path, PathBuf};

use super::file;
use crate::run::client::Client;
use crate::run::setup::SYMPOSIUM_CONFIG;

/// Larger than any config `init` writes.
const MAX_CONFIG: u64 = 1024 * 1024;

/// What a watched file held.
#[derive(Debug, PartialEq, Eq)]
enum Contents {
    Absent,
    Bytes(Vec<u8>),
    /// A link, a special file, a file too large, or one that cannot be read.
    Unreadable,
}

/// The watched files of a trial, as they were after `init`.
#[derive(Debug)]
#[must_use]
pub(crate) struct Snapshot {
    home: PathBuf,
    project: PathBuf,
    files: Vec<(Root, &'static str, Contents)>,
}

/// Which directory a watched file is relative to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Root {
    Home,
    Project,
}

/// A watched file that is not as it was after `init`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ConfigChange {
    pub(crate) root: Root,
    pub(crate) file: &'static str,
    pub(crate) change: Change,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Change {
    Created,
    Modified,
    Removed,
    /// Unreadable before or after, so it cannot be shown unchanged.
    Unreadable,
}

impl Snapshot {
    /// Copies `client`'s hook config from `home` and `project`, and Symposium's config.
    pub(crate) fn take(home: &Path, project: &Path, client: Client) -> Self {
        let mut snapshot = Self {
            home: home.to_owned(),
            project: project.to_owned(),
            files: Vec::new(),
        };
        let in_home = client.hook_config().iter().chain([&SYMPOSIUM_CONFIG]);
        let in_project = client.project_config().iter();
        let watched = in_home
            .map(|file| (Root::Home, *file))
            .chain(in_project.map(|file| (Root::Project, *file)));
        for (root, file) in watched {
            let contents = contents(&snapshot.path(root, file));
            snapshot.files.push((root, file, contents));
        }
        snapshot
    }

    fn path(&self, root: Root, file: &str) -> PathBuf {
        match root {
            Root::Home => self.home.join(file),
            Root::Project => self.project.join(file),
        }
    }

    /// The watched files that changed since the snapshot.
    pub(crate) fn changes(&self) -> Vec<ConfigChange> {
        self.files
            .iter()
            .filter_map(|(root, file, before)| {
                let change = match (before, contents(&self.path(*root, file))) {
                    (Contents::Unreadable, _) | (_, Contents::Unreadable) => Change::Unreadable,
                    (before, after) if *before == after => return None,
                    (Contents::Absent, _) => Change::Created,
                    (_, Contents::Absent) => Change::Removed,
                    _ => Change::Modified,
                };
                Some(ConfigChange {
                    root: *root,
                    file,
                    change,
                })
            })
            .collect()
    }
}

fn contents(path: &Path) -> Contents {
    match file::read(path, MAX_CONFIG) {
        Ok(Some(bytes)) => Contents::Bytes(bytes),
        Ok(None) => Contents::Absent,
        Err(_) => Contents::Unreadable,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    const SETTINGS: &str = ".claude/settings.json";

    /// A trial with Claude Code's settings in the home and nothing else watched.
    struct Trial {
        dir: TempDir,
    }

    impl Trial {
        fn new() -> Self {
            let trial = Self {
                dir: TempDir::new().unwrap(),
            };
            trial.write(Root::Home, SETTINGS, "{}");
            fs::create_dir_all(trial.root(Root::Project)).unwrap();
            trial
        }

        fn root(&self, root: Root) -> PathBuf {
            match root {
                Root::Home => self.dir.path().join("home"),
                Root::Project => self.dir.path().join("project"),
            }
        }

        fn write(&self, root: Root, file: &str, contents: &str) {
            let path = self.root(root).join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }

        fn replace_with_a_directory(&self, root: Root, file: &str) {
            let path = self.root(root).join(file);
            fs::remove_file(&path).unwrap();
            fs::create_dir(&path).unwrap();
        }

        fn snapshot(&self) -> Snapshot {
            let (home, project) = (self.root(Root::Home), self.root(Root::Project));
            Snapshot::take(&home, &project, Client::ClaudeCode)
        }
    }

    fn changes_after(edit: impl FnOnce(&Trial)) -> Vec<ConfigChange> {
        let trial = Trial::new();
        let snapshot = trial.snapshot();
        edit(&trial);
        snapshot.changes()
    }

    fn changed(root: Root, file: &'static str, change: Change) -> Vec<ConfigChange> {
        vec![ConfigChange { root, file, change }]
    }

    #[test]
    fn an_untouched_trial_has_no_changes() {
        assert_eq!(changes_after(|_| {}), []);
    }

    #[test]
    fn rewriting_the_same_bytes_is_no_change() {
        let rewritten = changes_after(|trial| trial.write(Root::Home, SETTINGS, "{}"));

        assert_eq!(rewritten, []);
    }

    #[test]
    fn each_kind_of_change_is_reported() {
        let modified = changes_after(|trial| trial.write(Root::Home, SETTINGS, "[]"));
        let removed = changes_after(|trial| {
            fs::remove_file(trial.root(Root::Home).join(SETTINGS)).unwrap();
        });
        let created = changes_after(|trial| trial.write(Root::Home, SYMPOSIUM_CONFIG, ""));

        assert_eq!(modified, changed(Root::Home, SETTINGS, Change::Modified));
        assert_eq!(removed, changed(Root::Home, SETTINGS, Change::Removed));
        assert_eq!(
            created,
            changed(Root::Home, SYMPOSIUM_CONFIG, Change::Created)
        );
    }

    #[test]
    fn a_hook_added_in_the_project_is_reported() {
        for file in [".claude/settings.json", ".claude/settings.local.json"] {
            let added = changes_after(|trial| trial.write(Root::Project, file, "{}"));

            assert_eq!(
                added,
                changed(Root::Project, file, Change::Created),
                "{file}"
            );
        }
    }

    #[test]
    fn a_file_that_cannot_be_read_is_a_change() {
        let replaced = changes_after(|trial| trial.replace_with_a_directory(Root::Home, SETTINGS));

        assert_eq!(replaced, changed(Root::Home, SETTINGS, Change::Unreadable));
    }

    #[test]
    fn a_file_unreadable_at_the_snapshot_is_a_change_even_unchanged() {
        let trial = Trial::new();
        trial.replace_with_a_directory(Root::Home, SETTINGS);

        let snapshot = trial.snapshot();

        assert_eq!(
            snapshot.changes(),
            changed(Root::Home, SETTINGS, Change::Unreadable)
        );
    }

    #[test]
    fn a_file_the_client_rewrites_is_not_watched() {
        let rewritten = changes_after(|trial| trial.write(Root::Home, ".claude.json", "{}"));

        assert_eq!(rewritten, []);
    }
}
