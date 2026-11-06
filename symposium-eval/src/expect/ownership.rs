//! Which skill copies are wholly Symposium's, so grading leaves them out of the agent's
//! edits.
//!
//! This is stricter than [`installed`](super::installed), which reads a copy as sync
//! reads it, links followed: a copy that matches there can still hold an entry the agent
//! made, such as a link to the right bytes. Here a copy is Symposium's only when its
//! literal tree is exactly what sync writes from the source: real files and directories
//! only, the source's entries and no others, an empty `.symposium`, and a `.gitignore`
//! holding `*`. Anything else stays the agent's to account for.

use std::fs;
use std::io;
use std::path::{Component, Path};

use super::installed::{GITIGNORE, GITIGNORE_CONTENTS, MARKER, copy_candidates, same_bytes};
use crate::task::SkillSource;
use crate::tree::{self, EntryKind, TreeEntry};

/// Discovers copies independently of assertions, comparing every candidate against
/// the task's trusted providers. Returns `/`-joined paths relative to `project`.
pub(crate) fn symposium_copies(
    project: &Path,
    skills: &Path,
    sources: &[SkillSource],
) -> Vec<String> {
    let mut owned = sources
        .iter()
        .flat_map(|source| {
            copy_candidates(skills, &source.name)
                .into_iter()
                .filter_map(|copy| {
                    is_symposium_copy(&copy, &source.dir)
                        .then(|| relative(project, &copy))
                        .flatten()
                })
        })
        .collect::<Vec<_>>();
    owned.sort();
    owned.dedup();
    owned
}

/// Whether `copy` is exactly what sync writes from `source`. One that cannot be read is
/// not shown to be.
fn is_symposium_copy(copy: &Path, source: &Path) -> bool {
    exact_copy(copy, source).unwrap_or(false)
}

fn exact_copy(copy: &Path, source: &Path) -> io::Result<bool> {
    if !fs::symlink_metadata(copy)?.is_dir() {
        return Ok(false);
    }
    let (Ok(copied), Ok(expected)) = (tree::walk(copy), tree::walk(source)) else {
        return Ok(false);
    };
    if copied
        .iter()
        .any(|entry| !matches!(entry.kind, EntryKind::File | EntryKind::Dir))
    {
        return Ok(false);
    }
    // Sync writes its own marker and `.gitignore` over any the source has.
    let unmanaged = |entry: &&TreeEntry| entry.path != MARKER && entry.path != GITIGNORE;
    let content: Vec<_> = copied.iter().filter(unmanaged).collect();
    let expected: Vec<_> = expected.iter().filter(unmanaged).collect();
    if content != expected || !managed_files(copy)? {
        return Ok(false);
    }
    for entry in expected
        .iter()
        .filter(|entry| entry.kind == EntryKind::File)
    {
        if !same_file(&copy.join(&entry.path), &source.join(&entry.path))? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether `copy` holds the marker and `.gitignore` exactly as sync writes them.
fn managed_files(copy: &Path) -> io::Result<bool> {
    let marker = fs::symlink_metadata(copy.join(MARKER))?;
    let gitignore = copy.join(GITIGNORE);
    let ignore = fs::symlink_metadata(&gitignore)?;
    // The size is checked first, since the agent chose the file's size.
    Ok(marker.is_file()
        && marker.len() == 0
        && ignore.is_file()
        && ignore.len() == GITIGNORE_CONTENTS.len() as u64
        && fs::read(gitignore)? == GITIGNORE_CONTENTS)
}

fn same_file(copy: &Path, source: &Path) -> io::Result<bool> {
    let len = fs::symlink_metadata(source)?.len();
    Ok(fs::symlink_metadata(copy)?.len() == len && same_bytes(copy, source, len)?)
}

/// `path` relative to `project`, `/`-joined, when it is plainly inside it.
fn relative(project: &Path, path: &Path) -> Option<String> {
    let parts = path
        .strip_prefix(project)
        .ok()?
        .components()
        .map(|component| match component {
            Component::Normal(part) => part.to_str(),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    (!parts.is_empty()).then(|| parts.join("/"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::grade::copy_exact;
    use tempfile::TempDir;

    use super::*;

    /// A skill source, and a project whose skill directory holds sync's copy of it.
    struct Installed {
        _dir: TempDir,
        source: PathBuf,
        project: PathBuf,
        copy: PathBuf,
    }

    impl Installed {
        fn new() -> Self {
            let dir = TempDir::new().unwrap();
            let source = dir.path().join("source");
            fs::create_dir_all(source.join("notes")).unwrap();
            fs::write(source.join("SKILL.md"), "---\nname: first-skill\n---\n").unwrap();
            fs::write(source.join("notes/more.md"), "more").unwrap();
            let project = dir.path().join("project");
            let copy = project.join(".claude/skills/first-skill");
            fs::create_dir_all(copy.join("notes")).unwrap();
            fs::copy(source.join("SKILL.md"), copy.join("SKILL.md")).unwrap();
            fs::copy(source.join("notes/more.md"), copy.join("notes/more.md")).unwrap();
            fs::write(copy.join(MARKER), "").unwrap();
            fs::write(copy.join(GITIGNORE), GITIGNORE_CONTENTS).unwrap();
            Self {
                _dir: dir,
                source,
                project,
                copy,
            }
        }

        fn owned(&self) -> Vec<String> {
            let source = SkillSource {
                name: "first-skill".to_owned(),
                dir: self.source.clone(),
            };
            symposium_copies(
                &self.project,
                &self.project.join(".claude/skills"),
                &[source],
            )
        }
    }

    #[test]
    fn an_exact_copy_is_symposiums() {
        let installed = Installed::new();

        assert_eq!(installed.owned(), [".claude/skills/first-skill"]);
    }

    #[test]
    fn a_hashed_copy_is_checked_against_each_provided_source() {
        let installed = Installed::new();
        let other_source = installed.project.join("other-source");
        copy_exact(&installed.source, &other_source, "test source").unwrap();
        fs::write(other_source.join("notes/more.md"), "different source").unwrap();
        let hashed = installed.copy.with_file_name("first-skill-0123abcd");
        copy_exact(&installed.copy, &hashed, "test copy").unwrap();
        fs::write(hashed.join("notes/more.md"), "different source").unwrap();
        let sources = [
            SkillSource {
                name: "first-skill".to_owned(),
                dir: installed.source.clone(),
            },
            SkillSource {
                name: "first-skill".to_owned(),
                dir: other_source,
            },
        ];

        let owned = symposium_copies(
            &installed.project,
            &installed.project.join(".claude/skills"),
            &sources,
        );

        assert_eq!(
            owned,
            [
                ".claude/skills/first-skill",
                ".claude/skills/first-skill-0123abcd"
            ]
        );
    }

    #[test]
    fn a_copy_of_a_source_with_its_own_gitignore_is_symposiums() {
        let installed = Installed::new();
        fs::write(installed.source.join(GITIGNORE), "drafts/\n").unwrap();

        assert_eq!(installed.owned(), [".claude/skills/first-skill"]);
    }

    #[test]
    fn a_copy_with_an_extra_entry_is_not() {
        let installed = Installed::new();
        fs::write(installed.copy.join("notes/extra.md"), "extra").unwrap();

        assert!(installed.owned().is_empty());
    }

    #[test]
    fn a_copy_with_other_bytes_is_not() {
        let installed = Installed::new();
        fs::write(installed.copy.join("notes/more.md"), "less").unwrap();

        assert!(installed.owned().is_empty());
    }

    #[test]
    fn a_copy_with_a_written_marker_or_gitignore_is_not() {
        for (name, contents) in [(MARKER, "written"), (GITIGNORE, "*\n!SKILL.md\n")] {
            let installed = Installed::new();
            fs::write(installed.copy.join(name), contents).unwrap();

            assert!(installed.owned().is_empty(), "{name}");
        }
    }

    #[test]
    fn a_copy_without_its_marker_is_not() {
        let installed = Installed::new();
        fs::remove_file(installed.copy.join(MARKER)).unwrap();

        assert!(installed.owned().is_empty());
    }

    #[test]
    fn a_copy_without_a_provided_source_is_not_considered() {
        let installed = Installed::new();
        let owned = symposium_copies(
            &installed.project,
            &installed.project.join(".claude/skills"),
            &[],
        );

        assert!(owned.is_empty());
    }

    // Creating a symlink on Windows needs developer mode or elevation.
    #[cfg(unix)]
    #[test]
    fn a_copy_holding_a_link_to_the_right_bytes_is_not() {
        let installed = Installed::new();
        let file = installed.copy.join("notes/more.md");
        fs::remove_file(&file).unwrap();
        std::os::unix::fs::symlink(installed.source.join("notes/more.md"), &file).unwrap();

        assert!(installed.owned().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_copy_is_not() {
        let installed = Installed::new();
        let elsewhere = installed.project.join("elsewhere");
        fs::rename(&installed.copy, &elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &installed.copy).unwrap();

        assert!(installed.owned().is_empty());
    }
}
