//! Listing a directory tree without following links.
//!
//! Grading decides what crosses into the grading copy from this listing, so a link is
//! reported as a link and never entered: a link could point anywhere on the machine.

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryKind {
    File,
    Dir,
    Symlink,
    /// A socket, FIFO, device, or anything else that is neither file, directory, nor link.
    Other,
}

impl EntryKind {
    /// The kind a `FileType` from a lookup that does not follow links describes.
    pub(crate) fn of(file_type: fs::FileType) -> Self {
        if file_type.is_symlink() {
            Self::Symlink
        } else if file_type.is_dir() {
            Self::Dir
        } else if file_type.is_file() {
            Self::File
        } else {
            Self::Other
        }
    }
}

impl fmt::Display for EntryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::File => "file",
            Self::Dir => "directory",
            Self::Symlink => "symlink",
            Self::Other => "special file",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TreeEntry {
    /// Relative to the walked root, with `/` between components on every platform.
    pub(crate) path: String,
    pub(crate) kind: EntryKind,
}

#[derive(Debug, Error)]
pub(crate) enum WalkError {
    #[error("cannot list `{path}`")]
    Read {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("cannot inspect `{path}`")]
    Inspect {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("`{path}` is not valid UTF-8")]
    NonUtf8 { path: String },
}

/// Every entry under `root`, depth first and sorted by name within each directory.
/// Directories are listed before their contents; links are listed but not entered.
///
/// # Errors
///
/// When a directory cannot be listed, or a name is not valid UTF-8 and so cannot be
/// matched against `/`-separated patterns.
pub(crate) fn walk(root: &Path) -> Result<Vec<TreeEntry>, WalkError> {
    let mut entries = Vec::new();
    walk_into(root, "", &mut entries)?;
    Ok(entries)
}

fn walk_into(dir: &Path, prefix: &str, entries: &mut Vec<TreeEntry>) -> Result<(), WalkError> {
    let shown = if prefix.is_empty() { "." } else { prefix };
    let read_error = |source| WalkError::Read {
        path: shown.to_owned(),
        source,
    };
    let mut children = Vec::new();
    for child in fs::read_dir(dir).map_err(read_error)? {
        let child = child.map_err(read_error)?;
        // `DirEntry::file_type` does not follow links, so a link stays a link.
        let file_type = child.file_type().map_err(|source| WalkError::Inspect {
            path: join(prefix, &child.file_name().to_string_lossy()),
            source,
        })?;
        children.push((child.file_name(), file_type));
    }
    children.sort_by(|(left, _), (right, _)| left.cmp(right));

    for (name, file_type) in children {
        let name = name.into_string().map_err(|name| WalkError::NonUtf8 {
            path: join(prefix, &name.to_string_lossy()),
        })?;
        let path = join(prefix, &name);
        let kind = EntryKind::of(file_type);
        entries.push(TreeEntry {
            path: path.clone(),
            kind,
        });
        if kind == EntryKind::Dir {
            walk_into(&dir.join(&name), &path, entries)?;
        }
    }
    Ok(())
}

fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{prefix}/{name}")
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn listing(root: &Path) -> Vec<(String, EntryKind)> {
        walk(root)
            .unwrap()
            .into_iter()
            .map(|entry| (entry.path, entry.kind))
            .collect()
    }

    #[test]
    fn lists_depth_first_in_name_order() {
        let root = TempDir::new().unwrap();
        fs::create_dir_all(root.path().join("src/nested")).unwrap();
        fs::write(root.path().join("src/nested/deep.rs"), "").unwrap();
        fs::write(root.path().join("src/lib.rs"), "").unwrap();
        fs::write(root.path().join("Cargo.toml"), "").unwrap();

        assert_eq!(
            listing(root.path()),
            [
                ("Cargo.toml".to_owned(), EntryKind::File),
                ("src".to_owned(), EntryKind::Dir),
                ("src/lib.rs".to_owned(), EntryKind::File),
                ("src/nested".to_owned(), EntryKind::Dir),
                ("src/nested/deep.rs".to_owned(), EntryKind::File),
            ]
        );
    }

    // Creating a symlink on Windows needs developer mode or elevation.
    #[cfg(unix)]
    #[test]
    fn links_are_listed_but_not_entered() {
        let root = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        fs::write(outside.path().join("secret"), "").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("link")).unwrap();

        assert_eq!(
            listing(root.path()),
            [("link".to_owned(), EntryKind::Symlink)]
        );
    }

    #[test]
    fn a_missing_root_is_a_read_error() {
        let root = TempDir::new().unwrap();

        let error = walk(&root.path().join("absent")).unwrap_err();

        assert_eq!(error.to_string(), "cannot list `.`");
    }
}
