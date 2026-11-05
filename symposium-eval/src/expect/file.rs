//! Opening a file the agent could have written.
//!
//! The checks run on the opened handle, so a path swapped for a link after a check
//! cannot be followed.

use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::path::Path;

/// Opens `path` as a regular file of at most `limit` bytes. `None` when there is no
/// file; an error for a link, anything but a regular file, or a larger file.
pub(crate) fn open(path: &Path, limit: u64) -> io::Result<Option<File>> {
    let file = match no_follow().open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let metadata = file.metadata()?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(io::Error::other("not a regular file"));
    }
    if metadata.len() > limit {
        return Err(io::Error::other("larger than any evidence written"));
    }
    Ok(Some(file))
}

/// The bytes of the file [`open`] opens, refusing more than `limit` even if it grew.
pub(crate) fn read(path: &Path, limit: u64) -> io::Result<Option<Vec<u8>>> {
    let Some(file) = open(path, limit)? else {
        return Ok(None);
    };
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::other("larger than any evidence written"));
    }
    Ok(Some(bytes))
}

/// Read access that does not follow a final link, nor block on a pipe.
fn no_follow() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Opens a link itself, which the metadata then shows.
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn a_regular_file_within_the_limit_opens() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("evidence");
        fs::write(&path, "12345").unwrap();

        assert!(open(&path, 5).unwrap().is_some());
        assert!(open(&path, 4).is_err());
    }

    #[test]
    fn read_gives_the_bytes_within_the_limit() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("evidence");
        fs::write(&path, "12345").unwrap();

        assert_eq!(read(&path, 5).unwrap().unwrap(), b"12345");
        assert!(read(&path, 4).is_err());
        assert!(read(&dir.path().join("absent"), 5).unwrap().is_none());
    }

    #[test]
    fn a_missing_file_is_none_and_a_directory_is_an_error() {
        let dir = TempDir::new().unwrap();

        assert!(open(&dir.path().join("absent"), 5).unwrap().is_none());
        assert!(open(dir.path(), 5).is_err());
    }

    /// Links need developer mode or elevation on Windows.
    #[cfg(unix)]
    #[test]
    fn a_link_is_not_followed() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("target");
        fs::write(&target, "").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        assert!(open(&link, 5).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_pipe_does_not_block() {
        let dir = TempDir::new().unwrap();
        let pipe = dir.path().join("pipe");
        let name = std::ffi::CString::new(pipe.to_str().unwrap()).unwrap();
        // SAFETY: `name` is a valid, NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);

        assert!(open(&pipe, 5).is_err());
    }
}
