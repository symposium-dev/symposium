//! Whether Symposium recorded a successful sync for the trial's project (RFD reference,
//! "Synchronization and hooks").
//!
//! Symposium writes `<dir>/state.json` under its cache's `workspaces/` only after a sync
//! succeeds, one per directory a session started in, with `"last-sync"` naming the
//! workspace `root`. This reader is the checker's own, not Symposium's `WorkspaceState`,
//! which reads an unreadable or malformed file as "never synced". The agent can write
//! under the home, so nothing here follows a link, and a file is read only up to a size.
//! A test runs the real hook and reads its record, so a change in the format fails loudly.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Deserializer};

use super::{Verdict, file};

/// Larger than any record Symposium writes, which lists a workspace's members,
/// dependencies and input files.
const MAX_RECORD: u64 = 1024 * 1024;

/// What the trial's home showed about a sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub(crate) enum SyncEvidence {
    /// A record whose root is the trial's project.
    Recorded,
    /// No record for the project, and nothing that could be one.
    NotRecorded,
    /// No record for the project, and a file or link that could not be read as one.
    Unknown,
}

impl SyncEvidence {
    /// The home held no record before launch, so a missing one means no sync succeeded.
    pub(crate) fn verdict(self) -> Verdict {
        match self {
            Self::Recorded => Verdict::Pass,
            Self::NotRecorded => Verdict::Fail,
            Self::Unknown => Verdict::Inconclusive,
        }
    }
}

/// The part of a record this reader relies on. `"last-sync"` must be present, as
/// Symposium always writes it; other fields are left for Symposium to add.
#[derive(Debug, Deserialize)]
struct State {
    #[serde(rename = "last-sync", deserialize_with = "present")]
    last_sync: Option<LastSync>,
}

#[derive(Debug, Deserialize)]
struct LastSync {
    root: PathBuf,
}

/// Reads a field that may be `null` but not missing: a custom reader is what stops
/// serde from reading a missing `Option` field as `None`.
fn present<'de, D: Deserializer<'de>>(fields: D) -> Result<Option<LastSync>, D::Error> {
    Option::deserialize(fields)
}

/// What one directory's record says.
enum Record {
    /// It names the project.
    Project,
    /// There is none, or it names another root.
    Elsewhere,
    Unreadable,
}

/// Decides whether `records`, under `home`, holds a sync of `project`. A record of the
/// project settles it, whatever else is there; otherwise anything unreadable makes it
/// unknown.
pub(crate) fn sync_record(home: &Path, records: &Path, project: &Path) -> SyncEvidence {
    scan(home, records, project).unwrap_or(SyncEvidence::Unknown)
}

fn scan(home: &Path, records: &Path, project: &Path) -> io::Result<SyncEvidence> {
    match way_down(home, records)? {
        Way::Real => {}
        Way::Missing => return Ok(SyncEvidence::NotRecorded),
        Way::NotReal => return Ok(SyncEvidence::Unknown),
    }
    let project = fs::canonicalize(project)?;
    let mut unknown = false;
    for entry in fs::read_dir(records)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            // Symposium makes directories here; a link is someone else's.
            unknown = true;
        } else if kind.is_dir() {
            match record(&entry.path().join("state.json"), &project) {
                Record::Project => return Ok(SyncEvidence::Recorded),
                Record::Elsewhere => {}
                Record::Unreadable => unknown = true,
            }
        }
    }
    Ok(if unknown {
        SyncEvidence::Unknown
    } else {
        SyncEvidence::NotRecorded
    })
}

/// What the directories from the home down to the records are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Way {
    Real,
    /// One is missing, so there are no records.
    Missing,
    /// One is a link or a file.
    NotReal,
}

/// Inspects `home` and every directory below it down to `dir`, following no link.
fn way_down(home: &Path, dir: &Path) -> io::Result<Way> {
    let below = dir
        .strip_prefix(home)
        .expect("the records are under the home");
    let mut walked = home.to_owned();
    let mut steps = vec![walked.clone()];
    for component in below.components() {
        let Component::Normal(name) = component else {
            return Ok(Way::NotReal);
        };
        walked.push(name);
        steps.push(walked.clone());
    }
    for step in steps {
        match fs::symlink_metadata(&step) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => return Ok(Way::NotReal),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Way::Missing),
            Err(error) => return Err(error),
        }
    }
    Ok(Way::Real)
}

fn record(path: &Path, project: &Path) -> Record {
    let text = match file::open(path, MAX_RECORD) {
        Ok(Some(file)) => read_bounded(file),
        Ok(None) => return Record::Elsewhere,
        Err(error) => Err(error),
    };
    let Ok(text) = text else {
        return Record::Unreadable;
    };
    let Ok(state) = serde_json::from_slice::<State>(&text) else {
        return Record::Unreadable;
    };
    match state.last_sync {
        None => Record::Elsewhere,
        // Symposium records an absolute root; a relative one says nothing certain.
        Some(last) if !last.root.is_absolute() => Record::Unreadable,
        // A root that no longer resolves may have been the project, reached by a link.
        Some(last) => match fs::canonicalize(&last.root) {
            Ok(root) if root == project => Record::Project,
            Ok(_) => Record::Elsewhere,
            Err(_) => Record::Unreadable,
        },
    }
}

/// `file`'s bytes, refusing more than [`MAX_RECORD`] even if it grew.
fn read_bounded(file: File) -> io::Result<Vec<u8>> {
    let mut text = Vec::new();
    file.take(MAX_RECORD + 1).read_to_end(&mut text)?;
    if text.len() as u64 > MAX_RECORD {
        return Err(io::Error::other(
            "the record is larger than any sync writes",
        ));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    /// A trial's home and project, as the runner lays them out.
    struct Trial {
        _root: TempDir,
        home: PathBuf,
        project: PathBuf,
    }

    impl Trial {
        fn new() -> Self {
            let root = TempDir::new().unwrap();
            let home = root.path().join("home");
            let project = root.path().join("project");
            fs::create_dir_all(&home).unwrap();
            fs::create_dir_all(&project).unwrap();
            Self {
                _root: root,
                home,
                project,
            }
        }

        fn records(&self) -> PathBuf {
            self.home.join(".symposium/cache/workspaces")
        }

        /// Writes `contents` as the record of the directory `name`.
        fn record(&self, name: &str, contents: &str) -> PathBuf {
            let dir = self.records().join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("state.json"), contents).unwrap();
            dir.join("state.json")
        }

        /// A record whose root is `root`, with the fields Symposium writes beside it.
        fn synced(&self, name: &str, root: &Path) {
            let state = serde_json::json!({
                "last-sync": {
                    "root": root,
                    "members": [root],
                    "deps": [],
                    "inputs": { root.join("Cargo.lock").to_string_lossy(): 1 },
                },
            });
            self.record(name, &state.to_string());
        }

        fn evidence(&self) -> SyncEvidence {
            sync_record(&self.home, &self.records(), &self.project)
        }
    }

    #[test]
    fn a_record_of_the_project_is_recorded() {
        let trial = Trial::new();
        trial.synced("project-0123abcd", &trial.project);

        assert_eq!(trial.evidence(), SyncEvidence::Recorded);
    }

    #[test]
    fn no_records_is_not_recorded() {
        let trial = Trial::new();
        assert_eq!(trial.evidence(), SyncEvidence::NotRecorded);

        fs::create_dir_all(trial.records().join("project-0123abcd")).unwrap();
        assert_eq!(trial.evidence(), SyncEvidence::NotRecorded);
    }

    #[test]
    fn a_record_with_no_sync_is_not_recorded() {
        let trial = Trial::new();
        trial.record("project-0123abcd", r#"{"last-sync":null}"#);

        assert_eq!(trial.evidence(), SyncEvidence::NotRecorded);
    }

    #[test]
    fn a_record_of_another_root_is_not_the_projects() {
        let trial = Trial::new();
        let elsewhere = trial.home.join("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        trial.synced("elsewhere-0123abcd", &elsewhere);

        assert_eq!(trial.evidence(), SyncEvidence::NotRecorded);
    }

    #[test]
    fn a_root_that_no_longer_resolves_is_unknown() {
        let trial = Trial::new();
        trial.synced("absent-0123abcd", &trial.home.join("absent"));

        assert_eq!(trial.evidence(), SyncEvidence::Unknown);
    }

    #[test]
    fn a_relative_root_is_unknown() {
        let trial = Trial::new();
        trial.synced("project-0123abcd", Path::new("project"));

        assert_eq!(trial.evidence(), SyncEvidence::Unknown);
    }

    #[test]
    fn a_record_that_cannot_be_read_is_unknown() {
        for contents in [
            "not json",
            "{}",
            r#"{"last-sync":"yesterday"}"#,
            r#"{"last-sync":{}}"#,
            r#"{"last-sync":{"root":7}}"#,
        ] {
            let trial = Trial::new();
            trial.record("project-0123abcd", contents);

            assert_eq!(trial.evidence(), SyncEvidence::Unknown, "{contents}");
        }
    }

    #[test]
    fn a_record_larger_than_any_sync_writes_is_unknown() {
        let trial = Trial::new();
        let padding = " ".repeat(MAX_RECORD as usize);
        let root = serde_json::to_string(&trial.project).unwrap();
        trial.record(
            "project-0123abcd",
            &format!(r#"{{"last-sync":{{"root":{root}}}}}{padding}"#),
        );

        assert_eq!(trial.evidence(), SyncEvidence::Unknown);
    }

    #[test]
    fn a_record_of_the_project_settles_it_whatever_else_is_there() {
        let trial = Trial::new();
        trial.record("broken-0123abcd", "not json");
        trial.synced("project-0123abcd", &trial.project);
        trial.record("zbroken-0123abcd", "not json");

        assert_eq!(trial.evidence(), SyncEvidence::Recorded);
    }

    #[test]
    fn the_verdict_follows_from_the_evidence() {
        assert_eq!(SyncEvidence::Recorded.verdict(), Verdict::Pass);
        assert_eq!(SyncEvidence::NotRecorded.verdict(), Verdict::Fail);
        assert_eq!(SyncEvidence::Unknown.verdict(), Verdict::Inconclusive);
    }

    /// Links need developer mode or elevation on Windows.
    #[cfg(unix)]
    mod links {
        use std::os::unix::fs::symlink;

        use super::*;

        #[test]
        fn a_linked_record_is_unknown() {
            let trial = Trial::new();
            let forged = trial.home.join("forged.json");
            let root = serde_json::to_string(&trial.project).unwrap();
            fs::write(&forged, format!(r#"{{"last-sync":{{"root":{root}}}}}"#)).unwrap();
            let dir = trial.records().join("project-0123abcd");
            fs::create_dir_all(&dir).unwrap();
            symlink(&forged, dir.join("state.json")).unwrap();

            assert_eq!(trial.evidence(), SyncEvidence::Unknown);
        }

        #[test]
        fn a_linked_directory_on_the_way_is_unknown() {
            for linked in [
                ".symposium",
                ".symposium/cache",
                ".symposium/cache/workspaces",
            ] {
                let trial = Trial::new();
                let elsewhere = trial.home.join("elsewhere");
                fs::create_dir_all(&elsewhere).unwrap();
                let link = trial.home.join(linked);
                fs::create_dir_all(link.parent().unwrap()).unwrap();
                symlink(&elsewhere, &link).unwrap();

                assert_eq!(trial.evidence(), SyncEvidence::Unknown, "{linked}");
            }
        }

        #[test]
        fn a_linked_home_is_unknown() {
            let trial = Trial::new();
            let real_home = trial.home.with_file_name("real-home");
            fs::rename(&trial.home, &real_home).unwrap();
            symlink(&real_home, &trial.home).unwrap();
            trial.synced("project-0123abcd", &trial.project);

            assert_eq!(trial.evidence(), SyncEvidence::Unknown);
        }

        #[test]
        fn a_linked_record_directory_is_unknown() {
            let trial = Trial::new();
            let elsewhere = trial.home.join("elsewhere");
            fs::create_dir_all(&elsewhere).unwrap();
            fs::create_dir_all(trial.records()).unwrap();
            symlink(&elsewhere, trial.records().join("project-0123abcd")).unwrap();

            assert_eq!(trial.evidence(), SyncEvidence::Unknown);
        }

        #[test]
        fn a_project_reached_through_a_link_is_the_same_project() {
            let trial = Trial::new();
            let linked = trial.home.join("linked-project");
            symlink(&trial.project, &linked).unwrap();
            trial.synced("project-0123abcd", &linked);

            assert_eq!(trial.evidence(), SyncEvidence::Recorded);
        }
    }

    /// The real hook's record, read by this reader: the contract between them. Runs
    /// the `cargo-agents` built from this checkout in a trial under real `srt`.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "needs node, srt, and bubblewrap installed, and builds cargo-agents"]
    fn the_session_start_hook_records_a_sync_this_reader_reads() {
        use crate::run::Configuration;
        use crate::run::client::Client;
        use crate::run::stage::{Audience, Tool};
        use crate::run::test_support::{RealSrt, api_key, built_cargo_agents};

        let mut real = RealSrt::new(api_key());
        let tool = Tool::Executable(built_cargo_agents());
        real.stage("cargo-agents", &tool, Audience::Symposium);
        let trial = real.trial(1, Configuration::Symposium);
        real.set_up_symposium(&trial, Client::ClaudeCode);
        let records = trial.sync_records();
        assert!(!records.exists(), "a record before the hook ran");
        let finished = real.run_session_start(&trial);

        assert!(finished.succeeded(), "{}", finished.stderr.tail());
        assert_eq!(
            sync_record(&trial.home(), &records, &trial.project()),
            SyncEvidence::Recorded
        );
    }
}
