//! Shared test utilities for creating temporary fixture directories.

use std::path::Path;

/// A file entry for [`instantiate_fixture`].
pub struct File<'a>(pub &'a str, pub &'a str);

/// Create a temporary directory populated with the given files.
///
/// Each `File(path, content)` creates the file at `path` relative to the
/// temp directory root, creating parent directories as needed.
///
/// Returns the [`tempfile::TempDir`] handle (drop it to clean up).
///
/// ```ignore
/// let tmp = instantiate_fixture(&[
///     File("foo/SYMPOSIUM.toml", r#"name = "foo"\ncrates = ["*"]"#),
///     File("bar/SKILL.md", "---\nname: bar\n---\nBody."),
/// ]);
/// let root = tmp.path();
/// ```
pub fn instantiate_fixture(files: &[File]) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    write_fixture_files(tmp.path(), files);
    tmp
}

/// Write files into an existing directory.
pub fn write_fixture_files(root: &Path, files: &[File]) {
    for File(path, content) in files {
        let full = root.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&full, content).unwrap();
    }
}
