//! Atomic replacement for fully prepared local telemetry files.

use std::{
    error::Error,
    fmt,
    io::{self, Write as _},
    path::Path,
};

use crate::telemetry::TEMPORARY_FILE_PREFIX;

/// Failure while preparing or committing an atomic file replacement.
#[derive(Debug)]
pub(in crate::telemetry) enum AtomicReplaceError {
    Create(io::Error),
    Write(io::Error),
    Persist(io::Error),
}

impl fmt::Display for AtomicReplaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Create(_) => formatter.write_str("failed to create temporary replacement file"),
            Self::Write(_) => formatter.write_str("failed to write temporary replacement file"),
            Self::Persist(_) => {
                formatter.write_str("failed to atomically replace destination file")
            }
        }
    }
}

impl Error for AtomicReplaceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Create(error) | Self::Write(error) | Self::Persist(error) => Some(error),
        }
    }
}

/// Replace one file with a completely prepared sibling temporary file.
///
/// The replacement is atomic at the filesystem path boundary but is not
/// crash-durable: this intentionally does not call `sync_all` on the file or
/// its parent directory. Callers must finish serialization before calling.
///
/// # Errors
///
/// Returns the stage that failed while creating, writing, or persisting the
/// temporary file.
pub(super) fn replace(path: &Path, contents: &[u8]) -> Result<(), AtomicReplaceError> {
    let parent = path.parent().ok_or_else(|| {
        AtomicReplaceError::Create(io::Error::new(
            io::ErrorKind::InvalidInput,
            "replacement path has no parent directory",
        ))
    })?;
    let mut builder = tempfile::Builder::new();
    builder.prefix(TEMPORARY_FILE_PREFIX);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;

        builder.permissions(std::fs::Permissions::from_mode(0o600));
    }
    let mut temporary = builder
        .tempfile_in(parent)
        .map_err(AtomicReplaceError::Create)?;

    temporary
        .write_all(contents)
        .map_err(AtomicReplaceError::Write)?;
    temporary
        .persist(path)
        .map_err(|error| AtomicReplaceError::Persist(error.error))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{AtomicReplaceError, replace};

    #[test]
    fn replacement_creates_a_complete_new_file() {
        let temporary = tempfile::tempdir().unwrap();
        let target = temporary.path().join("state.toml");

        replace(&target, b"version = 1\n").unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"version = 1\n");
    }

    #[test]
    fn replacement_overwrites_the_complete_existing_file() {
        let temporary = tempfile::tempdir().unwrap();
        let target = temporary.path().join("state.toml");
        fs::write(&target, b"old contents that are longer").unwrap();

        replace(&target, b"new\n").unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"new\n");
    }

    #[test]
    fn missing_parent_directory_reports_temporary_file_creation() {
        let temporary = tempfile::tempdir().unwrap();
        let target = temporary.path().join("missing").join("state.toml");

        let error = replace(&target, b"version = 1\n").unwrap_err();

        assert!(matches!(error, AtomicReplaceError::Create(_)));
    }

    #[test]
    fn failed_persistence_preserves_the_destination_and_cleans_up_the_temporary_file() {
        let temporary = tempfile::tempdir().unwrap();
        let target = temporary.path().join("state.toml");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("sentinel"), b"old").unwrap();

        let error = replace(&target, b"new").unwrap_err();

        assert!(matches!(error, AtomicReplaceError::Persist(_)));
        assert_eq!(fs::read(target.join("sentinel")).unwrap(), b"old");
        assert_eq!(fs::read_dir(temporary.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn replacement_is_owner_only_even_when_the_old_file_was_permissive() {
        use std::os::unix::fs::PermissionsExt as _;

        let temporary = tempfile::tempdir().unwrap();
        let target = temporary.path().join("state.toml");
        fs::write(&target, b"old").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();

        replace(&target, b"new").unwrap();

        let mode = fs::metadata(target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
