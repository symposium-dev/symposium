//! Directory traversal that follows symlinks without looping.
//!
//! Every tree symposium reads from a live source — a workspace member, a path
//! or git dependency, a `[[registry]]` path entry, `~/.symposium/plugins/`, or
//! a git-cloned registry cache — can contain symlinks, and what sits behind
//! them is skill content like any other. Such a tree is *dereferenced*: a link
//! to a file reads as that file, a link to a directory is walked.
//!
//! Preserving links instead would not work, because the destination is a
//! different tree: a relative link pointing outside the skill directory
//! dangles once copied. Dereferencing also makes a source-tree install agree
//! with a crates.io install, which already delivers real files — `cargo
//! package` follows links, including ones whose target escapes the package
//! root.
//!
//! Links are followed wherever they point; there is no containment check
//! against the source root. See [the skill reference][ref] for that decision
//! and its rationale.
//!
//! Following links costs two guarantees that a plain tree gives for free, and
//! this module holds both: a broken link has no metadata to resolve, and a
//! directory link naming an ancestor turns a walk into an endless descent.
//!
//! [ref]: ../../md/reference/skill-definition.md

use std::fs;
use std::path::{Path, PathBuf};

/// What a directory entry is once symlinks are resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryKind {
    Dir,
    File,
}

/// Classify a directory entry with symlinks resolved.
///
/// `file_type` is the entry's own type, as `DirEntry::file_type` reports it: a
/// symlink reads as neither a directory nor a regular file there, which is why
/// it is re-stated through `fs::metadata`, the call that follows the link.
/// (`DirEntry::metadata` does not follow it either.)
///
/// `Ok(None)` is anything that is neither a directory nor a regular file — a
/// socket or a fifo. `Err` can only come from the follow, so it means a broken
/// link.
pub(crate) fn resolved_kind(
    path: &Path,
    file_type: fs::FileType,
) -> std::io::Result<Option<EntryKind>> {
    let file_type = if file_type.is_symlink() {
        fs::metadata(path)?.file_type()
    } else {
        file_type
    };
    Ok(if file_type.is_dir() {
        Some(EntryKind::Dir)
    } else if file_type.is_file() {
        Some(EntryKind::File)
    } else {
        None
    })
}

/// The directories a walk is currently inside, identified by canonical path so
/// that a followed link landing on one of them is recognized.
///
/// A tree of real directories cannot contain itself, so this only ever fires
/// once a directory link has been followed — but every directory on the way
/// down has to be recorded, since the link that closes the loop may sit several
/// real directories below the one it names.
#[derive(Debug, Default)]
pub(crate) struct Ancestors(Vec<PathBuf>);

impl Ancestors {
    /// Start a walk rooted at `root`.
    pub(crate) fn rooted_at(root: &Path) -> Self {
        let mut ancestors = Self::default();
        ancestors.enter(root);
        ancestors
    }

    /// Record `dir` as entered, or return `false` when the walk is already
    /// inside it — the cycle a followed directory link creates. A `false`
    /// return means the caller must neither descend nor call [`Self::leave`].
    pub(crate) fn enter(&mut self, dir: &Path) -> bool {
        let key = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        if self.0.contains(&key) {
            return false;
        }
        self.0.push(key);
        true
    }

    /// Leave the directory most recently entered.
    pub(crate) fn leave(&mut self) {
        self.0.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(path: &Path) -> std::io::Result<Option<EntryKind>> {
        let file_type = fs::symlink_metadata(path).unwrap().file_type();
        resolved_kind(path, file_type)
    }

    #[test]
    fn plain_entries_classify() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("a.txt");
        fs::write(&file, "a").unwrap();
        let dir = tmp.path().join("sub");
        fs::create_dir(&dir).unwrap();

        assert_eq!(kind(&file).unwrap(), Some(EntryKind::File));
        assert_eq!(kind(&dir).unwrap(), Some(EntryKind::Dir));
    }

    /// A symlink is neither a file nor a directory to `DirEntry::file_type`;
    /// resolving it is what makes the copier see the content behind it.
    #[cfg(unix)]
    #[test]
    fn symlinks_classify_as_their_target() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a.txt"), "a").unwrap();
        fs::create_dir(tmp.path().join("sub")).unwrap();

        let to_file = tmp.path().join("link-file");
        let to_dir = tmp.path().join("link-dir");
        let broken = tmp.path().join("link-broken");
        symlink("a.txt", &to_file).unwrap();
        symlink("sub", &to_dir).unwrap();
        symlink("nope.txt", &broken).unwrap();

        assert_eq!(kind(&to_file).unwrap(), Some(EntryKind::File));
        assert_eq!(kind(&to_dir).unwrap(), Some(EntryKind::Dir));
        assert!(
            kind(&broken).is_err(),
            "a broken link has no target to read"
        );
    }

    #[test]
    fn a_directory_is_entered_once() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("sub");
        fs::create_dir(&dir).unwrap();

        let mut ancestors = Ancestors::rooted_at(tmp.path());
        assert!(ancestors.enter(&dir));
        assert!(!ancestors.enter(&dir));
        ancestors.leave();
        assert!(ancestors.enter(&dir), "leaving frees the directory again");
    }

    /// The loop to catch: a link naming a directory the walk is already inside,
    /// reached through real directories that are themselves not links.
    #[cfg(unix)]
    #[test]
    fn a_link_back_to_an_ancestor_is_refused() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().unwrap();
        let outer = tmp.path().join("outer");
        let inner = outer.join("inner");
        fs::create_dir_all(&inner).unwrap();
        let back = inner.join("back");
        symlink("../..", &back).unwrap();

        let mut ancestors = Ancestors::rooted_at(tmp.path());
        assert!(ancestors.enter(&outer));
        assert!(ancestors.enter(&inner));
        assert!(
            !ancestors.enter(&back),
            "following the link would re-enter the root"
        );
    }
}
