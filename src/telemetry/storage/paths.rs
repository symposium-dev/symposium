use std::path::{Path, PathBuf};

use crate::telemetry::schema::UtcDay;

pub(super) const EVENT_FILE_PREFIX: &str = "events-";
pub(super) const METRIC_FILE_PREFIX: &str = "metrics-";
pub(super) const DAILY_FILE_SUFFIX: &str = ".jsonl";

/// Canonical locations used by local telemetry storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StoragePaths {
    telemetry_dir: PathBuf,
    lock_file: PathBuf,
    state_file: PathBuf,
}

impl StoragePaths {
    #[must_use]
    pub(super) fn new(config_dir: &Path) -> Self {
        let telemetry_dir = config_dir.join("telemetry");
        let lock_file = telemetry_dir.join(".lock");
        let state_file = config_dir.join("telemetry-state.toml");

        Self {
            telemetry_dir,
            lock_file,
            state_file,
        }
    }

    #[must_use]
    pub(super) fn telemetry_dir(&self) -> &Path {
        &self.telemetry_dir
    }

    #[must_use]
    pub(super) fn lock_file(&self) -> &Path {
        &self.lock_file
    }

    #[must_use]
    pub(super) fn state_file(&self) -> &Path {
        &self.state_file
    }

    /// Return the canonical append-only event file for one UTC day.
    #[must_use]
    pub(super) fn event_file(&self, day: UtcDay) -> PathBuf {
        self.daily_file(EVENT_FILE_PREFIX, day)
    }

    /// Return the canonical aggregate snapshot file for one UTC day.
    #[must_use]
    pub(super) fn metrics_file(&self, day: UtcDay) -> PathBuf {
        self.daily_file(METRIC_FILE_PREFIX, day)
    }

    fn daily_file(&self, prefix: &str, day: UtcDay) -> PathBuf {
        self.telemetry_dir
            .join(format!("{prefix}{day}{DAILY_FILE_SUFFIX}"))
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::StoragePaths;
    use crate::telemetry::schema::UtcDay;

    #[test]
    fn storage_paths_keep_private_state_outside_inspectable_data() {
        let temporary = tempfile::tempdir().unwrap();

        let paths = StoragePaths::new(temporary.path());

        assert_eq!(paths.telemetry_dir(), temporary.path().join("telemetry"));
        assert_eq!(
            paths.lock_file(),
            temporary.path().join("telemetry").join(".lock")
        );
        assert_eq!(
            paths.state_file(),
            temporary.path().join("telemetry-state.toml")
        );
    }

    #[test]
    fn daily_paths_use_the_contract_filenames() {
        let temporary = tempfile::tempdir().unwrap();
        let paths = StoragePaths::new(temporary.path());
        let day = UtcDay::from_date(NaiveDate::from_ymd_opt(2026, 8, 3).unwrap());

        assert_eq!(
            paths.event_file(day),
            temporary.path().join("telemetry/events-2026-08-03.jsonl")
        );
        assert_eq!(
            paths.metrics_file(day),
            temporary.path().join("telemetry/metrics-2026-08-03.jsonl")
        );
    }
}
