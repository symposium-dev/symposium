//! Writing a result whole, and never over another.

use std::io::{self, Write as _};
use std::path::Path;

use super::TrialResult;

impl TrialResult {
    /// Writes the result to `path`, never over an existing file. It is written and
    /// synced in full to a temporary file beside `path`, then given its name with
    /// `persist_noclobber`. On Linux and macOS, where trials run, that is a rename that
    /// refuses to replace, or a hard link where the filesystem has no such rename; either
    /// way the name appears only for the complete file. Elsewhere `tempfile` promises
    /// less, and a crash may still lose the name, since the directory is not synced.
    ///
    /// # Errors
    ///
    /// When `path` already exists, or the result cannot be written.
    pub(crate) fn write(&self, path: &Path) -> io::Result<()> {
        let mut bytes = serde_json::to_vec_pretty(self)?;
        bytes.push(b'\n');
        let dir = path.parent().unwrap_or(Path::new("."));
        let mut temp = tempfile::NamedTempFile::new_in(dir)?;
        temp.write_all(&bytes)?;
        temp.as_file().sync_all()?;
        temp.persist_noclobber(path).map_err(|error| error.error)?;
        Ok(())
    }
}
