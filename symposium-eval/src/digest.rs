//! SHA-256 digests of what a trial ran, so a result names its inputs exactly (RFD
//! reference, "Retained results").
//!
//! A file's digest is of its bytes alone, as `sha256sum` prints it. A tree's covers
//! every entry by its `/`-joined path relative to the root, in byte order: its kind,
//! and a file's bytes. Each length is written before what it measures, so no two trees
//! encode alike. Nothing else counts: no timestamps, permissions, or absolute paths,
//! so a copy elsewhere has the same digest. A link or special file is refused rather
//! than followed or skipped, since either would leave out what it stands for.

use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};
use thiserror::Error;

use crate::task::DirProblem;
use crate::tree::{self, EntryKind, TreeEntry, WalkError};

/// Names what each encoding hashes, so a tree's digest is never another's.
const TREE: &[u8] = b"symposium-eval tree v1\n";

/// A SHA-256 digest, shown as lowercase hex.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Digest([u8; 32]);

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Digest({self})")
    }
}

#[derive(Debug, Error)]
pub(crate) enum DigestError {
    #[error("cannot read `{}`", .path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("`{}` {problem}", .root.display())]
    Root { root: PathBuf, problem: DirProblem },
    #[error("cannot inspect `{}`", .root.display())]
    Inspect {
        root: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cannot list `{}`", .root.display())]
    Walk {
        root: PathBuf,
        #[source]
        source: WalkError,
    },
    #[error("`{}` holds a {kind} at `{path}`, which has no content to hash", .root.display())]
    Entry {
        root: PathBuf,
        path: String,
        kind: EntryKind,
    },
}

/// The digest of the file at `path`.
///
/// # Errors
///
/// When the file cannot be read.
pub(crate) fn file(path: &Path) -> Result<Digest, DigestError> {
    let read_error = |source| DigestError::Read {
        path: path.to_owned(),
        source,
    };
    let mut hasher = Sha256::new();
    io::copy(&mut File::open(path).map_err(read_error)?, &mut hasher).map_err(read_error)?;
    Ok(Digest(hasher.finalize().into()))
}

/// The digest of the tree at `root`.
///
/// # Errors
///
/// When `root` is not a real directory, or the tree cannot be listed or read, or holds
/// a link or special file.
pub(crate) fn tree(root: &Path) -> Result<Digest, DigestError> {
    require_real_dir(root)?;
    let mut hasher = Sha256::new();
    hasher.update(TREE);
    for entry in sorted_entries(root)? {
        hash_entry(&mut hasher, root, &entry)?;
    }
    Ok(Digest(hasher.finalize().into()))
}

/// Refuses a `root` that is not a real directory, since listing a linked one would
/// follow it.
fn require_real_dir(root: &Path) -> Result<(), DigestError> {
    match DirProblem::of(root) {
        Ok(None) => Ok(()),
        Ok(Some(problem)) => Err(DigestError::Root {
            root: root.to_owned(),
            problem,
        }),
        Err(source) => Err(DigestError::Inspect {
            root: root.to_owned(),
            source,
        }),
    }
}

/// Every entry under `root`, sorted by the bytes of its path, so the order is the same
/// on every platform.
fn sorted_entries(root: &Path) -> Result<Vec<TreeEntry>, DigestError> {
    let mut entries = tree::walk(root).map_err(|source| DigestError::Walk {
        root: root.to_owned(),
        source,
    })?;
    entries.sort_by(|left, right| left.path.as_bytes().cmp(right.path.as_bytes()));
    Ok(entries)
}

/// Hashes one entry: its kind, its path, and a file's contents.
fn hash_entry(hasher: &mut Sha256, root: &Path, entry: &TreeEntry) -> Result<(), DigestError> {
    let kind = match entry.kind {
        EntryKind::Dir => b'd',
        EntryKind::File => b'f',
        kind @ (EntryKind::Symlink | EntryKind::Other) => {
            return Err(DigestError::Entry {
                root: root.to_owned(),
                path: entry.path.clone(),
                kind,
            });
        }
    };
    hasher.update([kind]);
    measured(hasher, entry.path.as_bytes());
    if entry.kind == EntryKind::File {
        hash_contents(hasher, &under(root, &entry.path))?;
    }
    Ok(())
}

/// `/`-joined `relative` under `root`.
fn under(root: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .fold(root.to_owned(), |path, part| path.join(part))
}

/// Hashes the file at `path`: its length, then exactly that many bytes.
fn hash_contents(hasher: &mut Sha256, path: &Path) -> Result<(), DigestError> {
    let read_error = |source| DigestError::Read {
        path: path.to_owned(),
        source,
    };
    let file = File::open(path).map_err(read_error)?;
    let length = file.metadata().map_err(read_error)?.len();
    hasher.update(length.to_le_bytes());
    exactly(file, length, hasher).map_err(read_error)
}

/// Hashes exactly `length` bytes of `source`: a file that shrinks or grows while it is
/// read is an error, not a digest of part of it.
fn exactly(mut source: impl io::Read, length: u64, hasher: &mut Sha256) -> io::Result<()> {
    let copied = io::copy(&mut source.by_ref().take(length), hasher)?;
    let mut extra = [0];
    if copied != length || source.read(&mut extra)? != 0 {
        return Err(io::Error::other("the file changed while it was hashed"));
    }
    Ok(())
}

/// `bytes`, after their length.
fn measured(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    /// A tree holding `files`, each a `/`-joined path and its contents.
    fn tree_of(files: &[(&str, &str)]) -> TempDir {
        let root = TempDir::new().unwrap();
        for (path, contents) in files {
            let path = path
                .split('/')
                .fold(root.path().to_owned(), |path, part| path.join(part));
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        root
    }

    fn digest_of(files: &[(&str, &str)]) -> Digest {
        tree(tree_of(files).path()).unwrap()
    }

    #[test]
    fn a_files_digest_is_its_sha256() {
        let root = tree_of(&[("file", "abc")]);

        let digest = file(&root.path().join("file")).unwrap();

        assert_eq!(
            digest.to_string(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn the_tree_encoding_is_fixed() {
        // Pinned to the value an independent implementation of the encoding gives, so a
        // change shows; a changed encoding must name a new version.
        let digest = digest_of(&[("a/b.txt", "content"), ("c", "")]);

        assert_eq!(digest.to_string(), PINNED);
    }

    const PINNED: &str = "2498757614a2b8a1b8776266e07c73a577c83bd29c137c05da24ac8c73d9af47";

    #[test]
    fn a_copy_elsewhere_has_the_same_digest() {
        let files = [("src/lib.rs", "pub fn f() {}"), ("Cargo.toml", "[package]")];

        assert_eq!(digest_of(&files), digest_of(&files));
    }

    #[test]
    fn every_part_of_an_entry_counts() {
        let base = digest_of(&[("dir/file", "bc")]);
        let changed = [
            digest_of(&[("dir/file", "bd")]),
            digest_of(&[("dir/other", "bc")]),
            digest_of(&[("file", "bc")]),
            // Without lengths, a path and the content after it could shift between them.
            digest_of(&[("dir/fileb", "c")]),
        ];
        for digest in changed {
            assert_ne!(digest, base);
        }
        let with_empty_dir = tree_of(&[("dir/file", "bc")]);
        fs::create_dir(with_empty_dir.path().join("empty")).unwrap();
        assert_ne!(tree(with_empty_dir.path()).unwrap(), base);
    }

    #[test]
    fn a_file_must_hold_exactly_its_length() {
        let mut hasher = Sha256::new();

        assert!(exactly(&b"abc"[..], 3, &mut hasher).is_ok());
        assert!(exactly(&b"abcd"[..], 3, &mut hasher).is_err(), "grew");
        assert!(exactly(&b"ab"[..], 3, &mut hasher).is_err(), "shrank");
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_root_is_refused() {
        let root = tree_of(&[("dir/file", "x")]);
        let link = root.path().join("link");
        std::os::unix::fs::symlink(root.path().join("dir"), &link).unwrap();

        let error = tree(&link).unwrap_err();

        assert!(
            matches!(
                error,
                DigestError::Root {
                    problem: DirProblem::Symlink,
                    ..
                }
            ),
            "{error:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_refused() {
        let root = tree_of(&[("file", "x")]);
        std::os::unix::fs::symlink("file", root.path().join("alias")).unwrap();

        let error = tree(root.path()).unwrap_err();

        assert!(
            matches!(&error, DigestError::Entry { path, kind: EntryKind::Symlink, .. } if path == "alias"),
            "{error:?}"
        );
    }
}
