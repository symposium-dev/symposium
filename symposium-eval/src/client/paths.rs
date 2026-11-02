//! Paths a transcript names, resolved by name alone.
//!
//! A transcript is read after its session ended, when the files it names may be gone
//! and links may have moved, so nothing here consults the filesystem. `..` is resolved
//! lexically, which names a different file than the OS would if the directory before it
//! was a link; a link made and removed during the session cannot be seen afterwards.

use std::path::{Component, Path, PathBuf};

/// `reported`, joined onto `cwd` if relative, with `.` and `..` resolved by name.
/// `None` unless the result has a root, which `..` cannot climb past, so a leading `..`
/// is never dropped.
pub(crate) fn lexical_absolute(cwd: &Path, reported: &Path) -> Option<PathBuf> {
    lexical(&cwd.join(reported))
}

/// `path` with `.` and `..` resolved by name; `None` unless it has a root.
pub(crate) fn lexical(path: &Path) -> Option<PathBuf> {
    if !path.has_root() {
        return None;
    }
    let mut normal = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            // At the root, `..` stays there, as the OS resolves it.
            Component::ParentDir => {
                normal.pop();
            }
            other => normal.push(other),
        }
    }
    Some(normal)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An absolute path on this platform.
    fn root(path: &str) -> PathBuf {
        let root = if cfg!(windows) { "C:\\" } else { "/" };
        Path::new(root).join(path)
    }

    #[test]
    fn a_relative_path_resolves_against_the_cwd_by_name() {
        let cwd = root("trial/project/src");

        assert_eq!(
            lexical_absolute(&cwd, Path::new("../notes/./a.txt")),
            Some(root("trial/project/notes/a.txt"))
        );
    }

    #[test]
    fn an_absolute_path_ignores_the_cwd() {
        let cwd = root("trial/project");

        assert_eq!(
            lexical_absolute(&cwd, &root("trial/evidence/../evidence/x")),
            Some(root("trial/evidence/x"))
        );
    }

    #[test]
    fn a_path_without_a_root_resolves_to_nothing() {
        assert_eq!(
            lexical_absolute(Path::new("trial"), Path::new("../x")),
            None
        );
        assert_eq!(lexical(Path::new("../x")), None);
    }

    #[test]
    fn dot_dot_at_the_root_stays_there() {
        assert_eq!(lexical(&root("../x")), Some(root("x")));
    }
}
