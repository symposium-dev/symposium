use std::{
    error::Error,
    fmt,
    fs::{DirBuilder, File, OpenOptions, TryLockError},
    io,
    path::Path,
};

#[cfg(unix)]
use std::fs;

use super::paths::StoragePaths;

/// Owns the telemetry lock for the lifetime of the underlying file handle.
#[derive(Debug)]
pub(super) struct TelemetryLock {
    _file: File,
}

/// Failure to prepare or acquire the telemetry lock.
#[derive(Debug)]
pub(in crate::telemetry) enum LockError {
    /// Another handle, in this process or another, currently owns the lock.
    Contended,
    /// The filesystem operation failed for a reason other than contention.
    Io(io::Error),
}

impl fmt::Display for LockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Contended => formatter.write_str("telemetry lock is held by another handle"),
            Self::Io(_) => formatter.write_str("telemetry lock I/O failed"),
        }
    }
}

impl Error for LockError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Contended => None,
            Self::Io(error) => Some(error),
        }
    }
}

impl From<io::Error> for LockError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<TryLockError> for LockError {
    fn from(error: TryLockError) -> Self {
        match error {
            TryLockError::WouldBlock => Self::Contended,
            TryLockError::Error(error) => Self::Io(error),
        }
    }
}

impl TelemetryLock {
    /// Make one non-blocking attempt to acquire the telemetry lock.
    ///
    /// # Errors
    ///
    /// Returns [`LockError::Contended`] when another handle, in this process or
    /// another, holds the lock. Other filesystem failures are returned as
    /// [`LockError::Io`].
    pub(super) fn try_acquire(paths: &StoragePaths) -> Result<Self, LockError> {
        create_private_dir(paths.telemetry_dir())?;
        let file = open_private_lock_file(paths.lock_file())?;
        file.try_lock()?;

        Ok(Self { _file: file })
    }
}

fn create_private_dir(path: &Path) -> io::Result<()> {
    let mut builder = DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;

        // Protect new directories immediately; set_permissions also repairs existing ones.
        builder.mode(0o700);
    }
    builder.create(path)?;
    #[cfg(unix)]
    fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;

    Ok(())
}

fn open_private_lock_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;

        // Protect new files immediately; set_permissions also repairs existing ones.
        options.mode(0o600);
    }
    let file = options.open(path)?;
    #[cfg(unix)]
    file.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(0o600))?;

    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::{LockError, TelemetryLock};
    use crate::telemetry::storage::paths::StoragePaths;

    #[test]
    fn second_acquisition_reports_contention() {
        let temporary = tempfile::tempdir().unwrap();
        let paths = StoragePaths::new(temporary.path());
        let _held = TelemetryLock::try_acquire(&paths).unwrap();

        let error = TelemetryLock::try_acquire(&paths).unwrap_err();

        assert!(matches!(error, LockError::Contended));
    }

    #[test]
    fn storage_preparation_failure_reports_io() {
        let temporary = tempfile::tempdir().unwrap();
        let paths = StoragePaths::new(temporary.path());
        std::fs::write(paths.telemetry_dir(), []).unwrap();

        let error = TelemetryLock::try_acquire(&paths).unwrap_err();

        assert!(matches!(error, LockError::Io(_)));
    }

    #[test]
    fn dropping_the_holder_releases_the_lock() {
        let temporary = tempfile::tempdir().unwrap();
        let paths = StoragePaths::new(temporary.path());
        let held = TelemetryLock::try_acquire(&paths).unwrap();
        drop(held);

        let _reacquired = TelemetryLock::try_acquire(&paths).unwrap();
    }

    #[test]
    fn unwinding_releases_the_lock() {
        let temporary = tempfile::tempdir().unwrap();
        let paths = StoragePaths::new(temporary.path());

        let panic = std::panic::catch_unwind(|| {
            let _held = TelemetryLock::try_acquire(&paths).unwrap();
            panic!("test panic while holding the telemetry lock");
        });

        assert!(panic.is_err());
        let _reacquired = TelemetryLock::try_acquire(&paths).unwrap();
    }

    #[test]
    fn acquisition_creates_only_the_data_directory_and_permanent_lock_file() {
        let temporary = tempfile::tempdir().unwrap();
        let paths = StoragePaths::new(temporary.path());

        let held = TelemetryLock::try_acquire(&paths).unwrap();
        drop(held);

        assert!(paths.telemetry_dir().is_dir());
        assert!(paths.lock_file().is_file());
        assert!(!paths.state_file().exists());
    }

    #[cfg(unix)]
    #[test]
    fn created_storage_is_owner_only() {
        use std::{fs, os::unix::fs::PermissionsExt as _};

        let temporary = tempfile::tempdir().unwrap();
        let paths = StoragePaths::new(temporary.path());

        let _held = TelemetryLock::try_acquire(&paths).unwrap();

        let directory_mode = fs::metadata(paths.telemetry_dir())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let lock_mode = fs::metadata(paths.lock_file())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(directory_mode, 0o700);
        assert_eq!(lock_mode, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn acquisition_tightens_existing_storage_permissions() {
        use std::{fs, os::unix::fs::PermissionsExt as _};

        let temporary = tempfile::tempdir().unwrap();
        let paths = StoragePaths::new(temporary.path());
        fs::create_dir(paths.telemetry_dir()).unwrap();
        fs::set_permissions(paths.telemetry_dir(), fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(paths.lock_file(), []).unwrap();
        fs::set_permissions(paths.lock_file(), fs::Permissions::from_mode(0o644)).unwrap();

        let _held = TelemetryLock::try_acquire(&paths).unwrap();

        let directory_mode = fs::metadata(paths.telemetry_dir())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let lock_mode = fs::metadata(paths.lock_file())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(directory_mode, 0o700);
        assert_eq!(lock_mode, 0o600);
    }
}
