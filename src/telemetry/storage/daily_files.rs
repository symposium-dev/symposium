//! Discovery of canonical telemetry day files.

use std::{
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
};

use super::paths::{DAILY_FILE_SUFFIX, EVENT_FILE_PREFIX, METRIC_FILE_PREFIX};
use crate::telemetry::schema::UtcDay;

/// Kind of one canonical daily telemetry file.
///
/// Declaration order is the raw-inspection order within one day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::telemetry) enum DailyFileKind {
    Events,
    Metrics,
}

impl DailyFileKind {
    const ALL: [Self; 2] = [Self::Events, Self::Metrics];

    pub(super) const fn prefix(self) -> &'static str {
        match self {
            Self::Events => EVENT_FILE_PREFIX,
            Self::Metrics => METRIC_FILE_PREFIX,
        }
    }
}

/// One regular file in the canonical telemetry archive namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DailyFile {
    pub(super) day: UtcDay,
    pub(super) kind: DailyFileKind,
    pub(super) path: PathBuf,
}

/// Best-effort discovery result used by the diagnostic status reader.
pub(super) struct CanonicalFileInspection {
    pub(super) files: Vec<DailyFile>,
    pub(super) directory_failure: Option<io::ErrorKind>,
    pub(super) entry_inspection_failures: u64,
    pub(super) first_entry_inspection_failure: Option<io::ErrorKind>,
}

/// Discover canonical daily files without letting one unreadable entry hide
/// the rest of the archive from `telemetry status`.
///
/// A missing directory is an empty archive. Failure to open an existing
/// directory is retained as a content-free category, while individual entry
/// and file-type failures are counted and scanning continues.
pub(super) fn inspect_canonical_files(directory: &Path) -> CanonicalFileInspection {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return CanonicalFileInspection {
                files: Vec::new(),
                directory_failure: None,
                entry_inspection_failures: 0,
                first_entry_inspection_failure: None,
            };
        }
        Err(error) => {
            return CanonicalFileInspection {
                files: Vec::new(),
                directory_failure: Some(error.kind()),
                entry_inspection_failures: 0,
                first_entry_inspection_failure: None,
            };
        }
    };

    let mut files = Vec::new();
    let mut entry_inspection_failures = 0_u64;
    let mut first_entry_inspection_failure = None;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                record_entry_inspection_failure(
                    &mut entry_inspection_failures,
                    &mut first_entry_inspection_failure,
                    error.kind(),
                );
                continue;
            }
        };
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                record_entry_inspection_failure(
                    &mut entry_inspection_failures,
                    &mut first_entry_inspection_failure,
                    error.kind(),
                );
                continue;
            }
        };
        if !file_type.is_file() {
            continue;
        }
        let Some((day, kind)) = file_identity(&entry.file_name()) else {
            continue;
        };
        files.push(DailyFile {
            day,
            kind,
            path: entry.path(),
        });
    }
    files.sort_by_key(|file| (file.day, file.kind));

    CanonicalFileInspection {
        files,
        directory_failure: None,
        entry_inspection_failures,
        first_entry_inspection_failure,
    }
}

fn record_entry_inspection_failure(
    count: &mut u64,
    first: &mut Option<io::ErrorKind>,
    kind: io::ErrorKind,
) {
    *count = count.saturating_add(1);
    first.get_or_insert(kind);
}

/// Return the newest UTC day named by a canonical event or metric file.
///
/// Unknown names and abandoned telemetry temporaries are ignored. Directory
/// entry failures abort discovery because overlooking a later day could reopen
/// a daily file that private state had already closed.
pub(super) fn newest_day(directory: &Path) -> io::Result<Option<UtcDay>> {
    let mut newest = None;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let Some(day) = file_day(&entry.file_name()) else {
            continue;
        };
        newest = Some(newest.map_or(day, |current: UtcDay| current.max(day)));
    }

    Ok(newest)
}

pub(super) fn file_day(file_name: &OsStr) -> Option<UtcDay> {
    file_identity(file_name).map(|(day, _)| day)
}

fn file_identity(file_name: &OsStr) -> Option<(UtcDay, DailyFileKind)> {
    let file_name = file_name.to_str()?;
    let (day, kind) = DailyFileKind::ALL
        .into_iter()
        .find_map(|kind| file_name.strip_prefix(kind.prefix()).map(|day| (day, kind)))?;
    let day = day.strip_suffix(DAILY_FILE_SUFFIX)?.parse().ok()?;
    Some((day, kind))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::NaiveDate;

    use super::*;
    use crate::telemetry::storage::paths::StoragePaths;

    fn day(year: i32, month: u32, day: u32) -> UtcDay {
        UtcDay::from_date(NaiveDate::from_ymd_opt(year, month, day).unwrap())
    }

    #[test]
    fn newest_day_considers_event_and_metric_files() {
        let temporary = tempfile::tempdir().unwrap();
        for name in [
            "events-2026-08-03.jsonl",
            "metrics-2026-09-11.jsonl",
            "events-2026-08-30.jsonl",
        ] {
            fs::write(temporary.path().join(name), []).unwrap();
        }

        let newest = newest_day(temporary.path()).unwrap();

        assert_eq!(newest, Some(day(2026, 9, 11)));
    }

    #[test]
    fn newest_day_ignores_noncanonical_and_unrelated_names() {
        let temporary = tempfile::tempdir().unwrap();
        fs::create_dir(temporary.path().join("events-2026-08-03.jsonl")).unwrap();
        for name in [
            ".lock",
            ".telemetry-tmp-state",
            "events-2026-8-03.jsonl",
            "events-2026-02-30.jsonl",
            "events-2026-08-03.json",
            "metrics-2026-08-03.jsonl.tmp",
            "other-2026-08-03.jsonl",
        ] {
            fs::write(temporary.path().join(name), []).unwrap();
        }

        assert_eq!(newest_day(temporary.path()).unwrap(), None);
    }

    #[test]
    fn newest_day_propagates_directory_read_failures() {
        let temporary = tempfile::tempdir().unwrap();
        let missing = temporary.path().join("missing");

        let error = newest_day(&missing).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn canonical_paths_round_trip_through_the_daily_scanner() {
        let temporary = tempfile::tempdir().unwrap();
        let paths = StoragePaths::new(temporary.path());
        let expected = day(2026, 8, 3);

        for path in [paths.event_file(expected), paths.metrics_file(expected)] {
            assert_eq!(file_day(path.file_name().unwrap()), Some(expected));
        }
    }

    #[test]
    fn canonical_prefixes_identify_their_own_file_kinds() {
        let expected = day(2026, 8, 3);

        assert_eq!(
            file_identity(OsStr::new("events-2026-08-03.jsonl")),
            Some((expected, DailyFileKind::Events))
        );
        assert_eq!(
            file_identity(OsStr::new("metrics-2026-08-03.jsonl")),
            Some((expected, DailyFileKind::Metrics))
        );
    }

    #[test]
    fn canonical_file_inspection_orders_days_then_events_before_metrics() {
        let temporary = tempfile::tempdir().unwrap();
        for name in [
            "metrics-2026-08-04.jsonl",
            "events-2026-08-04.jsonl",
            "metrics-2026-08-03.jsonl",
            "events-2026-08-03.jsonl",
            ".telemetry-tmp-state",
        ] {
            fs::write(temporary.path().join(name), []).unwrap();
        }

        let inspection = inspect_canonical_files(temporary.path());
        let names: Vec<_> = inspection
            .files
            .iter()
            .map(|file| file.path.file_name().unwrap().to_str().unwrap())
            .collect();

        assert_eq!(inspection.directory_failure, None);
        assert_eq!(inspection.entry_inspection_failures, 0);
        assert_eq!(inspection.first_entry_inspection_failure, None);
        assert_eq!(
            names,
            [
                "events-2026-08-03.jsonl",
                "metrics-2026-08-03.jsonl",
                "events-2026-08-04.jsonl",
                "metrics-2026-08-04.jsonl",
            ]
        );
    }

    #[test]
    fn missing_directory_is_an_empty_archive() {
        let temporary = tempfile::tempdir().unwrap();

        let inspection = inspect_canonical_files(&temporary.path().join("missing"));

        assert!(inspection.files.is_empty());
        assert_eq!(inspection.directory_failure, None);
        assert_eq!(inspection.entry_inspection_failures, 0);
        assert_eq!(inspection.first_entry_inspection_failure, None);
    }
}
