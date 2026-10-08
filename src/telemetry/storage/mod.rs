//! Local telemetry paths and exclusive filesystem access.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the storage foundation is built before the recorder uses it."
    )
)]

mod atomic;
mod daily_files;
mod lock;
mod paths;
mod private_state;

use std::path::Path;

pub(in crate::telemetry) use lock::LockError;

use lock::TelemetryLock;
use paths::StoragePaths;

/// Exclusive access to all local telemetry files and sibling private state.
#[derive(Debug)]
pub(super) struct LockedStorage {
    paths: StoragePaths,
    _lock: TelemetryLock,
}

impl LockedStorage {
    /// Make one non-blocking attempt to enter the telemetry storage boundary.
    ///
    /// # Errors
    ///
    /// Returns contention separately from filesystem failures.
    pub(super) fn try_acquire(config_dir: &Path) -> Result<Self, LockError> {
        let paths = StoragePaths::new(config_dir);
        let storage_lock = TelemetryLock::try_acquire(&paths)?;
        Ok(Self {
            paths,
            _lock: storage_lock,
        })
    }
}
