//! Opportunistic cleanup while the telemetry storage lock is held.
//!
//! Maintenance scans only the inspectable telemetry directory. Canonical
//! daily files expire after D30, while abandoned replacement temporaries use a
//! reserved prefix and are removed immediately. Every entry is independent:
//! an unreadable or undeletable entry delays only its own cleanup.

use std::{fs, io, path::Path};

use super::{LockedStorage, daily_files::file_day};
use crate::telemetry::{
    RETENTION_DAYS, TEMPORARY_FILE_PREFIX, schema::UtcDay, state::MaintenanceDue,
};

/// Bounded result of one lock-bound maintenance attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MaintenanceOutcome {
    DirectoryUnavailable {
        kind: io::ErrorKind,
    },
    Completed {
        removed_daily_files: u64,
        removed_temporary_files: u64,
        inspection_failures: u64,
        removal_failures: u64,
    },
}

#[derive(Clone, Copy)]
enum Candidate {
    Daily,
    Temporary,
}

impl<Writer> LockedStorage<Writer> {
    /// Run the maintenance attempt released by a published recording receipt.
    ///
    /// No compliant caller can obtain `due` before the state containing its
    /// throttle marker has persisted and publication has been attempted.
    #[must_use]
    pub(super) fn maintain(&self, due: MaintenanceDue) -> MaintenanceOutcome {
        maintain_directory(self.paths.telemetry_dir(), due.day(), |path| {
            fs::remove_file(path)
        })
    }
}

fn maintain_directory(
    directory: &Path,
    accepted_day: UtcDay,
    remove_file: impl FnMut(&Path) -> io::Result<()>,
) -> MaintenanceOutcome {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            return MaintenanceOutcome::DirectoryUnavailable { kind: error.kind() };
        }
    };

    maintain_entries(
        entries,
        accepted_day,
        |entry| entry.file_type(),
        remove_file,
    )
}

fn maintain_entries(
    entries: impl Iterator<Item = io::Result<fs::DirEntry>>,
    accepted_day: UtcDay,
    mut inspect_file_type: impl FnMut(&fs::DirEntry) -> io::Result<fs::FileType>,
    mut remove_file: impl FnMut(&Path) -> io::Result<()>,
) -> MaintenanceOutcome {
    let mut removed_daily_files = 0_u64;
    let mut removed_temporary_files = 0_u64;
    let mut inspection_failures = 0_u64;
    let mut removal_failures = 0_u64;

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                inspection_failures = inspection_failures.saturating_add(1);
                tracing::debug!(
                    kind = ?error.kind(),
                    "telemetry: failed to inspect one maintenance entry"
                );
                continue;
            }
        };
        let file_type = match inspect_file_type(&entry) {
            Ok(file_type) => file_type,
            Err(error) => {
                inspection_failures = inspection_failures.saturating_add(1);
                tracing::debug!(
                    kind = ?error.kind(),
                    "telemetry: failed to inspect one maintenance entry type"
                );
                continue;
            }
        };

        // `DirEntry::file_type` deliberately does not follow symlinks. Cleanup
        // removes only regular files owned by this directory, never a target
        // selected through a link or a recursively nested path.
        if !file_type.is_file() {
            continue;
        }

        let file_name = entry.file_name();
        let candidate = if file_name
            .to_str()
            .is_some_and(|name| name.starts_with(TEMPORARY_FILE_PREFIX))
        {
            Candidate::Temporary
        } else if file_day(&file_name)
            .is_some_and(|day| accepted_day.days_since(day) > RETENTION_DAYS)
        {
            Candidate::Daily
        } else {
            continue;
        };

        match remove_file(&entry.path()) {
            Ok(()) => match candidate {
                Candidate::Daily => {
                    removed_daily_files = removed_daily_files.saturating_add(1);
                }
                Candidate::Temporary => {
                    removed_temporary_files = removed_temporary_files.saturating_add(1);
                }
            },
            Err(error) => {
                removal_failures = removal_failures.saturating_add(1);
                tracing::debug!(
                    kind = ?error.kind(),
                    "telemetry: failed to remove one maintenance entry"
                );
            }
        }
    }

    MaintenanceOutcome::Completed {
        removed_daily_files,
        removed_temporary_files,
        inspection_failures,
        removal_failures,
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, fs};

    use chrono::{NaiveDate, TimeZone as _, Utc};

    use super::*;
    use crate::telemetry::{
        schema::{UtcDay, UtcSecond},
        state::TelemetryStateV1,
        storage::Storage,
    };

    fn day(month: u32, day: u32) -> UtcDay {
        UtcDay::from_date(NaiveDate::from_ymd_opt(2026, month, day).unwrap())
    }

    fn due(month: u32, day_of_month: u32) -> MaintenanceDue {
        let accepted_day = day(month, day_of_month);
        let mut state = TelemetryStateV1::new(accepted_day).unwrap();
        let accepted = state
            .observe_recording(UtcSecond::from_datetime(
                Utc.with_ymd_and_hms(2026, month, day_of_month, 12, 0, 0)
                    .unwrap(),
            ))
            .unwrap();
        state.schedule_maintenance(&accepted).unwrap()
    }

    fn completed(
        removed_daily_files: u64,
        removed_temporary_files: u64,
        removal_failures: u64,
    ) -> MaintenanceOutcome {
        MaintenanceOutcome::Completed {
            removed_daily_files,
            removed_temporary_files,
            inspection_failures: 0,
            removal_failures,
        }
    }

    #[test]
    fn daily_files_are_retained_through_d30_and_removed_on_d31() {
        let root = tempfile::tempdir().unwrap();
        let storage = Storage::try_acquire(root.path()).unwrap();
        let d30_event = storage.paths.event_file(day(8, 11));
        let d30_metric = storage.paths.metrics_file(day(8, 11));
        let d31_event = storage.paths.event_file(day(8, 10));
        let d31_metric = storage.paths.metrics_file(day(8, 10));
        for path in [&d30_event, &d30_metric, &d31_event, &d31_metric] {
            fs::write(path, []).unwrap();
        }

        let outcome = storage.maintain(due(9, 10));

        assert_eq!(outcome, completed(2, 0, 0));
        assert!(d30_event.is_file());
        assert!(d30_metric.is_file());
        assert!(!d31_event.exists());
        assert!(!d31_metric.exists());
    }

    #[test]
    fn current_and_future_daily_files_are_retained() {
        let root = tempfile::tempdir().unwrap();
        let storage = Storage::try_acquire(root.path()).unwrap();
        let current = storage.paths.event_file(day(9, 10));
        let future = storage.paths.metrics_file(day(9, 11));
        fs::write(&current, []).unwrap();
        fs::write(&future, []).unwrap();

        let outcome = storage.maintain(due(9, 10));

        assert_eq!(outcome, completed(0, 0, 0));
        assert!(current.is_file());
        assert!(future.is_file());
    }

    #[test]
    fn abandoned_replacement_temporaries_are_removed_immediately() {
        let root = tempfile::tempdir().unwrap();
        let storage = Storage::try_acquire(root.path()).unwrap();
        let abandoned = storage
            .paths
            .telemetry_dir()
            .join(".telemetry-tmp-abandoned");
        fs::write(&abandoned, []).unwrap();

        let outcome = storage.maintain(due(9, 10));

        assert_eq!(outcome, completed(0, 1, 0));
        assert!(!abandoned.exists());
    }

    #[test]
    fn unknown_names_and_directories_are_ignored() {
        let root = tempfile::tempdir().unwrap();
        let storage = Storage::try_acquire(root.path()).unwrap();
        let unknown = storage.paths.telemetry_dir().join("events-old.jsonl");
        let directory = storage
            .paths
            .telemetry_dir()
            .join("events-2026-08-01.jsonl");
        fs::write(&unknown, []).unwrap();
        fs::create_dir(&directory).unwrap();

        let outcome = storage.maintain(due(9, 10));

        assert_eq!(outcome, completed(0, 0, 0));
        assert!(unknown.is_file());
        assert!(directory.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_ignored_without_following_their_targets() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let storage = Storage::try_acquire(root.path()).unwrap();
        let target = root.path().join("outside.jsonl");
        let link = storage.paths.event_file(day(8, 1));
        fs::write(&target, b"outside").unwrap();
        symlink(&target, &link).unwrap();

        let outcome = storage.maintain(due(9, 10));

        assert_eq!(outcome, completed(0, 0, 0));
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(fs::read(target).unwrap(), b"outside");
    }

    #[test]
    fn one_removal_failure_does_not_stop_later_candidates() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("telemetry");
        fs::create_dir(&directory).unwrap();
        for name in ["events-2026-08-01.jsonl", "metrics-2026-08-01.jsonl"] {
            fs::write(directory.join(name), []).unwrap();
        }
        let attempts = Cell::new(0_u64);

        let outcome = maintain_directory(&directory, day(9, 10), |path| {
            let attempt = attempts.get();
            attempts.set(attempt + 1);
            if attempt == 0 {
                Err(io::Error::from(io::ErrorKind::PermissionDenied))
            } else {
                fs::remove_file(path)
            }
        });

        assert_eq!(outcome, completed(1, 0, 1));
        assert_eq!(attempts.get(), 2);
        assert_eq!(
            fs::read_dir(directory)
                .unwrap()
                .filter_map(Result::ok)
                .count(),
            1
        );
    }

    #[test]
    fn one_inspection_failure_does_not_stop_later_candidates() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("telemetry");
        fs::create_dir(&directory).unwrap();
        for name in ["events-2026-08-01.jsonl", "metrics-2026-08-01.jsonl"] {
            fs::write(directory.join(name), []).unwrap();
        }
        let attempts = Cell::new(0_u64);

        let outcome = maintain_entries(
            fs::read_dir(&directory).unwrap(),
            day(9, 10),
            |entry| {
                let attempt = attempts.get();
                attempts.set(attempt + 1);
                if attempt == 0 {
                    Err(io::Error::from(io::ErrorKind::PermissionDenied))
                } else {
                    entry.file_type()
                }
            },
            |path| fs::remove_file(path),
        );

        assert_eq!(
            outcome,
            MaintenanceOutcome::Completed {
                removed_daily_files: 1,
                removed_temporary_files: 0,
                inspection_failures: 1,
                removal_failures: 0,
            }
        );
        assert_eq!(attempts.get(), 2);
        assert_eq!(fs::read_dir(directory).unwrap().count(), 1);
    }

    #[test]
    fn an_unavailable_directory_is_reported_separately() {
        let root = tempfile::tempdir().unwrap();

        let outcome = maintain_directory(&root.path().join("missing"), day(9, 10), |path| {
            fs::remove_file(path)
        });

        assert_eq!(
            outcome,
            MaintenanceOutcome::DirectoryUnavailable {
                kind: io::ErrorKind::NotFound
            }
        );
    }

    #[test]
    fn cleanup_never_reaches_private_state_outside_the_data_directory() {
        let root = tempfile::tempdir().unwrap();
        let storage = Storage::try_acquire(root.path()).unwrap();
        fs::write(storage.paths.state_file(), b"private state").unwrap();

        let outcome = storage.maintain(due(9, 10));

        assert_eq!(outcome, completed(0, 0, 0));
        assert_eq!(
            fs::read(storage.paths.state_file()).unwrap(),
            b"private state"
        );
    }
}
