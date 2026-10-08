//! Discovery of canonical telemetry day files.

use std::{ffi::OsStr, fs, io, path::Path};

use super::paths::{DAILY_FILE_SUFFIX, EVENT_FILE_PREFIX, METRIC_FILE_PREFIX};
use crate::telemetry::schema::UtcDay;

const DAILY_FILE_PREFIXES: [&str; 2] = [EVENT_FILE_PREFIX, METRIC_FILE_PREFIX];

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

fn file_day(file_name: &OsStr) -> Option<UtcDay> {
    let file_name = file_name.to_str()?;
    let day = DAILY_FILE_PREFIXES
        .iter()
        .find_map(|prefix| file_name.strip_prefix(prefix))?
        .strip_suffix(DAILY_FILE_SUFFIX)?;
    day.parse().ok()
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
}
