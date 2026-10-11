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
pub(in crate::telemetry) mod events;
pub(in crate::telemetry) mod limits;
mod lock;
pub(in crate::telemetry) mod metrics;
mod paths;
mod private_state;
mod writes;

use std::path::Path;

pub(in crate::telemetry) use lock::LockError;

use lock::TelemetryLock;
use paths::StoragePaths;
use writes::{FsWrites, RecordingWriteAccess, RecordingWrites};

#[cfg(test)]
#[expect(
    unused_imports,
    reason = "the recording transaction consumes this test seam in the next branch"
)]
use writes::{TestWriteOperation, TestWrites};

/// Exclusive access to all local telemetry files and sibling private state.
#[derive(Debug)]
struct LockedStorage<Writer> {
    paths: StoragePaths,
    writer: Writer,
    _lock: TelemetryLock,
}

/// Production storage using the real filesystem writer.
type Storage = LockedStorage<FsWrites>;

impl LockedStorage<FsWrites> {
    /// Make one non-blocking attempt to enter the telemetry storage boundary.
    ///
    /// # Errors
    ///
    /// Returns contention separately from filesystem failures.
    fn try_acquire(config_dir: &Path) -> Result<Self, LockError> {
        let paths = StoragePaths::new(config_dir);
        let storage_lock = TelemetryLock::try_acquire(&paths)?;
        Ok(Self {
            paths,
            writer: FsWrites,
            _lock: storage_lock,
        })
    }
}

impl<Writer> LockedStorage<Writer> {
    /// Acquire real storage while substituting the whole-operation writer.
    #[cfg(test)]
    fn try_acquire_with_writer(config_dir: &Path, writer: Writer) -> Result<Self, LockError> {
        let paths = StoragePaths::new(config_dir);
        let storage_lock = TelemetryLock::try_acquire(&paths)?;
        Ok(Self {
            paths,
            writer,
            _lock: storage_lock,
        })
    }
}
