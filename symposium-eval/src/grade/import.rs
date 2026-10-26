//! The grading copy: the task's starter and fixtures from `evals/`, with only the
//! submission's editable entries taken from it (RFD reference, "Grading the code").
//!
//! Within `editable` the submission replaces the starter exactly: the starter's editable
//! entries are removed first, so a file the agent deleted stays deleted. The submission
//! is listed recursively without following links, so a link or special file under an
//! editable path invalidates it rather than being followed. Everything else it changed is
//! reported and never imported, and protected build files are always the starter's.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use globset::GlobMatcher;
use tempfile::TempDir;

use super::ImportError;
use crate::evals::Evals;
use crate::task::{EditablePattern, Task, is_protected};
use crate::tree::{self, EntryKind, TreeEntry, WalkError};

/// A task's trusted project and fixtures in a private temporary directory, laid out as in
/// a trial (`project/` beside `fixtures/<name>/`) so path dependencies resolve. Removed
/// when dropped.
#[derive(Debug)]
pub(crate) struct GradingCopy {
    root: TempDir,
}

/// A change the submission made that grading ignores.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OutsideEdit {
    pub(crate) path: String,
    pub(crate) change: Change,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Change {
    Added,
    Modified,
    Deleted,
    /// A file became a directory or a link, or the other way around.
    TypeChanged,
}

impl GradingCopy {
    pub(crate) fn project(&self) -> PathBuf {
        self.root.path().join("project")
    }

    /// The copy's own directory, beside `project/`, for anything grading keeps out of
    /// the project, such as Cargo's target directory.
    pub(crate) fn scratch(&self) -> &Path {
        self.root.path()
    }
}

impl fmt::Display for OutsideEdit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let change = match self.change {
            Change::Added => "added",
            Change::Modified => "modified",
            Change::Deleted => "deleted",
            Change::TypeChanged => "changed the type of",
        };
        write!(f, "{change} `{}`", self.path)
    }
}

/// Takes the editable entries of `submission` into `copy`, which still holds the trusted
/// starter. Returns the outside edits, sorted by path.
///
/// # Errors
///
/// [`ImportError::invalidates_submission`] tells an invalid submission from a failure of
/// the task or the machine.
pub(crate) fn import_submission(
    copy: &GradingCopy,
    patterns: &[EditablePattern],
    submission: &Path,
) -> Result<Vec<OutsideEdit>, ImportError> {
    let editable = Editable::new(patterns);
    let submission = Submission::list(submission, &editable)?;
    let starter = copy.listing()?;
    let mut outside_editable = outside_edits(&starter, &submission, &copy.project(), &editable)?;
    copy.clear_editable(&starter, &editable)?;
    outside_editable.extend(copy.import(&submission, &starter, &editable)?);
    outside_editable.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(outside_editable)
}

impl GradingCopy {
    /// The starter and the task's fixtures, as `evals/` holds them.
    pub(crate) fn from_trusted(evals: &Evals, task: &Task) -> Result<Self, ImportError> {
        let root = TempDir::new().map_err(|source| ImportError::Write {
            path: std::env::temp_dir(),
            source,
        })?;
        copy_tree(
            &evals.starter_dir(task.name()),
            &root.path().join("project"),
            "starter",
        )?;
        for fixture in task.fixtures() {
            copy_tree(
                &evals.fixture_dir(fixture),
                &root.path().join("fixtures").join(fixture.as_str()),
                &format!("fixtures/{fixture}"),
            )?;
        }
        Ok(Self { root })
    }

    fn listing(&self) -> Result<Listing, ImportError> {
        listing(&self.project()).map_err(|source| ImportError::Trusted {
            tree: "starter".to_owned(),
            source,
        })
    }

    /// Removes the starter's editable files, then each editable directory this leaves
    /// empty, so the submission decides what is there, even a file where a directory was.
    fn clear_editable(&self, starter: &Listing, editable: &Editable) -> Result<(), ImportError> {
        let project = self.project();
        for (path, &kind) in starter {
            if kind == EntryKind::File && editable.imports(path) {
                let path = project.join(path);
                fs::remove_file(&path).map_err(|source| ImportError::Write { path, source })?;
            }
        }
        // In reverse order, a directory's contents come before the directory.
        for (path, &kind) in starter.iter().rev() {
            if kind == EntryKind::Dir && editable.imports(path) {
                let dir = project.join(path);
                let mut children = fs::read_dir(&dir).map_err(|source| ImportError::Read {
                    path: dir.clone(),
                    source,
                })?;
                if children.next().is_none() {
                    fs::remove_dir(&dir)
                        .map_err(|source| ImportError::Write { path: dir, source })?;
                }
            }
        }
        Ok(())
    }

    /// Copies the submission's editable entries in, returning the ones trusted content
    /// kept out.
    fn import(
        &self,
        submission: &Submission<'_>,
        starter: &Listing,
        editable: &Editable,
    ) -> Result<Vec<OutsideEdit>, ImportError> {
        let project = self.project();
        let resolved_project = project.canonicalize().map_err(|source| ImportError::Read {
            path: project.clone(),
            source,
        })?;
        let mut kept_out = Vec::new();
        for (path, &kind) in &submission.entries {
            if matches!(kind, EntryKind::File | EntryKind::Dir)
                && editable.imports(path)
                && !import_entry(submission.root, &resolved_project, path, kind)?
            {
                let change = if starter.contains_key(path) {
                    Change::TypeChanged
                } else {
                    Change::Added
                };
                kept_out.push(OutsideEdit {
                    path: path.clone(),
                    change,
                });
            }
        }
        Ok(kept_out)
    }
}

/// A submission as grading reads it: listed without following links, with no link or
/// special file where it would stand in for editable content.
struct Submission<'a> {
    root: &'a Path,
    entries: Listing,
}

impl<'a> Submission<'a> {
    fn list(root: &'a Path, editable: &Editable) -> Result<Self, ImportError> {
        // The listing never follows a link below the root, and listing a root that is a
        // link would follow it, so the root must be a real directory too.
        let root_kind = fs::symlink_metadata(root)
            .map(|metadata| EntryKind::of(metadata.file_type()))
            .map_err(|source| {
                ImportError::Submission(WalkError::Read {
                    path: ".".to_owned(),
                    source,
                })
            })?;
        if root_kind != EntryKind::Dir {
            return Err(ImportError::SubmissionRoot(root_kind));
        }
        let entries = listing(root).map_err(ImportError::Submission)?;
        let unsupported = entries.iter().find(|&(path, &kind)| {
            matches!(kind, EntryKind::Symlink | EntryKind::Other) && editable.reaches(path)
        });
        if let Some((path, &kind)) = unsupported {
            return Err(ImportError::UnsupportedEntry {
                path: path.clone(),
                kind,
            });
        }
        Ok(Self { root, entries })
    }
}

/// Copies a tree the repository owns, which holds only files and directories. Existing
/// entries are overlaid, replacing an entry of the other type, so a second tree can be
/// laid over a first.
///
/// # Errors
///
/// When the tree holds anything else, or cannot be read or written.
pub(crate) fn copy_tree(source: &Path, destination: &Path, tree: &str) -> Result<(), ImportError> {
    let entries = tree::walk(source).map_err(|walk_error| ImportError::Trusted {
        tree: tree.to_owned(),
        source: walk_error,
    })?;
    create_dir(destination)?;
    entries
        .into_iter()
        .filter(|entry| !is_build_output(&entry.path))
        .try_for_each(|entry| copy_entry(source, destination, entry, tree))
}

/// Copies one entry of a trusted tree, replacing an entry of the other type.
fn copy_entry(
    source: &Path,
    destination: &Path,
    entry: TreeEntry,
    tree: &str,
) -> Result<(), ImportError> {
    let target = destination.join(&entry.path);
    match entry.kind {
        EntryKind::Dir => {
            clear_other_type(&target, EntryKind::Dir)?;
            create_dir(&target)
        }
        EntryKind::File => {
            clear_other_type(&target, EntryKind::File)?;
            fs::copy(source.join(&entry.path), &target)
                .map(drop)
                .map_err(|source| ImportError::Write {
                    path: target,
                    source,
                })
        }
        EntryKind::Symlink | EntryKind::Other => Err(ImportError::UntrustedEntry {
            tree: tree.to_owned(),
            path: entry.path,
            kind: entry.kind,
        }),
    }
}

/// Removes what is at `target` when it is not a `kind`, so a tree laid over another can
/// turn a file into a directory or the reverse. Only trusted trees are copied, so what is
/// there is the repository's to replace.
fn clear_other_type(target: &Path, kind: EntryKind) -> Result<(), ImportError> {
    let write_error = |source| ImportError::Write {
        path: target.to_owned(),
        source,
    };
    match fs::symlink_metadata(target) {
        Ok(metadata) if metadata.is_dir() && kind == EntryKind::File => {
            fs::remove_dir_all(target).map_err(write_error)
        }
        Ok(metadata) if !metadata.is_dir() && kind == EntryKind::Dir => {
            fs::remove_file(target).map_err(write_error)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(ImportError::Read {
            path: target.to_owned(),
            source,
        }),
    }
}

/// The compiled `editable` patterns.
struct Editable {
    matchers: Vec<GlobMatcher>,
    /// Each pattern's literal prefix, such as `src` for `src/**`.
    roots: Vec<String>,
}

impl Editable {
    fn new(patterns: &[EditablePattern]) -> Self {
        Self {
            matchers: patterns.iter().map(EditablePattern::matcher).collect(),
            roots: patterns
                .iter()
                .map(|pattern| pattern.literal_prefix().to_owned())
                .collect(),
        }
    }

    /// Whether grading takes `path` from the submission.
    fn imports(&self, path: &str) -> bool {
        !is_protected(path) && self.matchers.iter().any(|matcher| matcher.is_match(path))
    }

    /// Whether `path` is an editable root, inside one, or holds one: a link there would
    /// stand in for editable content.
    fn reaches(&self, path: &str) -> bool {
        self.roots
            .iter()
            .any(|root| path == root || is_within(path, root) || is_within(root, path))
    }
}

/// Paths to kinds, sorted by path.
type Listing = BTreeMap<String, EntryKind>;

/// Lists `root`, without the build output Cargo leaves in a project.
fn listing(root: &Path) -> Result<Listing, WalkError> {
    Ok(tree::walk(root)?
        .into_iter()
        .filter(|entry| !is_build_output(&entry.path))
        .map(|entry| (entry.path, entry.kind))
        .collect())
}

/// What the submission changed where grading takes nothing from it.
fn outside_edits(
    starter: &Listing,
    submission: &Submission<'_>,
    project: &Path,
    editable: &Editable,
) -> Result<Vec<OutsideEdit>, ImportError> {
    let paths: BTreeSet<&String> = starter.keys().chain(submission.entries.keys()).collect();
    let mut edits = Vec::new();
    for path in paths.into_iter().filter(|path| !editable.imports(path)) {
        if let Some(change) = change_at(path, starter, submission, project)? {
            edits.push(OutsideEdit {
                path: path.clone(),
                change,
            });
        }
    }
    Ok(edits)
}

/// How the submission changed `path` from the starter, if at all. A directory with
/// contents is reported through them.
fn change_at(
    path: &str,
    starter: &Listing,
    submission: &Submission<'_>,
    project: &Path,
) -> Result<Option<Change>, ImportError> {
    let submitted = &submission.entries;
    Ok(match (starter.get(path), submitted.get(path)) {
        (None, Some(&kind)) if kind != EntryKind::Dir || is_empty_dir(submitted, path) => {
            Some(Change::Added)
        }
        (Some(&kind), None) if kind != EntryKind::Dir || is_empty_dir(starter, path) => {
            Some(Change::Deleted)
        }
        (Some(before), Some(after)) if before != after => Some(Change::TypeChanged),
        (Some(EntryKind::File), Some(EntryKind::File)) => {
            (!same_contents(&project.join(path), &submission.root.join(path))?)
                .then_some(Change::Modified)
        }
        _ => None,
    })
}

/// Whether the directory at `path` has nothing under it in `listing`.
fn is_empty_dir(listing: &Listing, path: &str) -> bool {
    let children = format!("{path}/");
    listing
        .range(children.clone()..)
        .next()
        .is_none_or(|(next, _)| !next.starts_with(&children))
}

/// Copies one submitted file or directory into the copy, or returns `Ok(false)` when
/// trusted content holds its place. The starter's editable entries are already gone, so
/// whatever is still there (a file where a parent directory belongs, a directory with
/// protected contents, or the same name in another case on a case-insensitive
/// filesystem) is not the submission's to replace. A directory may land on a directory.
fn import_entry(
    submission: &Path,
    project: &Path,
    path: &str,
    kind: EntryKind,
) -> Result<bool, ImportError> {
    match place(project, path)? {
        Place::Free => {}
        Place::Directory if kind == EntryKind::Dir => return Ok(true),
        Place::Directory | Place::Taken => return Ok(false),
    }
    let destination = project.join(path);
    create_parent_within(project, &destination, path)?;
    if kind == EntryKind::Dir {
        create_dir(&destination)?;
    } else {
        fs::copy(submission.join(path), &destination).map_err(|source| ImportError::Write {
            path: destination,
            source,
        })?;
    }
    Ok(true)
}

/// Creates the parent of `destination` and proves it resolves inside `project`. Every
/// directory in the copy is real, so this holds by construction; the check keeps it
/// holding if that ever changes.
fn create_parent_within(project: &Path, destination: &Path, path: &str) -> Result<(), ImportError> {
    let parent = destination.parent().unwrap_or(project);
    create_dir(parent)?;
    let resolved = parent.canonicalize().map_err(|source| ImportError::Read {
        path: parent.to_owned(),
        source,
    })?;
    if resolved.starts_with(project) {
        Ok(())
    } else {
        Err(ImportError::Escape {
            path: path.to_owned(),
        })
    }
}

/// What the copy holds where a submitted entry would go.
enum Place {
    Free,
    Directory,
    /// A file or other entry is there, or where one of its parent directories belongs.
    Taken,
}

fn place(project: &Path, path: &str) -> Result<Place, ImportError> {
    let mut current = project.to_owned();
    let components: Vec<&str> = path.split('/').collect();
    for (index, component) in components.iter().enumerate() {
        current.push(component);
        let is_last = index + 1 == components.len();
        match fs::symlink_metadata(&current) {
            Ok(metadata) if !metadata.is_dir() => return Ok(Place::Taken),
            Ok(_) if is_last => return Ok(Place::Directory),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Place::Free),
            Err(source) => {
                return Err(ImportError::Read {
                    path: current,
                    source,
                });
            }
        }
    }
    Ok(Place::Free)
}

/// Compares two files in fixed-size chunks, so a large file costs time, not memory.
fn same_contents(left: &Path, right: &Path) -> Result<bool, ImportError> {
    const CHUNK: usize = 64 * 1024;
    let read_error = |path: &Path| {
        let path = path.to_owned();
        move |source| ImportError::Read { path, source }
    };
    let mut left_file = File::open(left).map_err(read_error(left))?;
    let mut right_file = File::open(right).map_err(read_error(right))?;
    let left_len = left_file.metadata().map_err(read_error(left))?.len();
    let right_len = right_file.metadata().map_err(read_error(right))?.len();
    if left_len != right_len {
        return Ok(false);
    }
    let mut left_chunk = vec![0; CHUNK];
    let mut right_chunk = vec![0; CHUNK];
    loop {
        let read = fill(&mut left_file, &mut left_chunk).map_err(read_error(left))?;
        if read == 0 {
            return Ok(true);
        }
        right_file
            .read_exact(&mut right_chunk[..read])
            .map_err(read_error(right))?;
        if left_chunk[..read] != right_chunk[..read] {
            return Ok(false);
        }
    }
}

/// Reads until `buffer` is full or the reader is exhausted.
fn fill(reader: &mut impl Read, buffer: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match reader.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(filled)
}

fn create_dir(path: &Path) -> Result<(), ImportError> {
    fs::create_dir_all(path).map_err(|source| ImportError::Write {
        path: path.to_owned(),
        source,
    })
}

/// The agent runs Cargo in its project, so its `target/` holds build output, not edits.
fn is_build_output(path: &str) -> bool {
    path == "target" || is_within(path, "target")
}

fn is_within(path: &str, dir: &str) -> bool {
    path.strip_prefix(dir)
        .is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grade::testing::{Imported, STARTER_MANIFEST, TaskFixture};

    fn read_project(import: &Imported, path: &str) -> Option<String> {
        fs::read_to_string(import.copy.project().join(path)).ok()
    }

    #[test]
    fn editable_files_replace_the_starters_and_deletions_stay_deleted() {
        let fixture = TaskFixture::new("");
        let submission = fixture.submission();
        fs::remove_file(submission.path().join("src/old.rs")).unwrap();
        fs::write(submission.path().join("src/lib.rs"), "pub fn new() {}").unwrap();
        fs::create_dir_all(submission.path().join("src/nested")).unwrap();
        fs::write(submission.path().join("src/nested/added.rs"), "added").unwrap();

        let import = fixture.build_copy(submission.path()).unwrap();

        assert_eq!(
            read_project(&import, "src/lib.rs").unwrap(),
            "pub fn new() {}"
        );
        assert_eq!(
            read_project(&import, "src/nested/added.rs").unwrap(),
            "added"
        );
        assert_eq!(read_project(&import, "src/old.rs"), None);
        assert_eq!(import.outside_editable(), []);
    }

    #[test]
    fn edits_outside_editable_are_reported_in_order_and_not_imported() {
        let fixture = TaskFixture::new("");
        let submission = fixture.submission();
        let root = submission.path();
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"changed\"").unwrap();
        fs::remove_file(root.join("Cargo.lock")).unwrap();
        fs::write(root.join("notes.txt"), "").unwrap();
        fs::remove_file(root.join("README.md")).unwrap();
        fs::create_dir(root.join("README.md")).unwrap();
        fs::create_dir_all(root.join("src/.cargo")).unwrap();
        fs::write(root.join("src/.cargo/config.toml"), "").unwrap();

        let import = fixture.build_copy(root).unwrap();

        let edits: Vec<String> = import
            .outside_editable()
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            edits,
            [
                "deleted `Cargo.lock`",
                "modified `Cargo.toml`",
                "changed the type of `README.md`",
                "added `notes.txt`",
                "added `src/.cargo/config.toml`",
            ]
        );
        assert_eq!(
            read_project(&import, "Cargo.toml").unwrap(),
            STARTER_MANIFEST
        );
        assert!(read_project(&import, "Cargo.lock").is_some());
        assert_eq!(read_project(&import, "notes.txt"), None);
        assert!(!import.copy.project().join("src/.cargo").exists());
    }

    #[test]
    fn an_editable_directory_can_become_a_file() {
        let fixture = TaskFixture::new("");
        fs::create_dir_all(fixture.starter().join("src/generated")).unwrap();
        fs::write(fixture.starter().join("src/generated/old.rs"), "").unwrap();
        let submission = fixture.submission();
        fs::remove_dir_all(submission.path().join("src/generated")).unwrap();
        fs::write(submission.path().join("src/generated"), "now a file").unwrap();

        let import = fixture.build_copy(submission.path()).unwrap();

        assert_eq!(
            read_project(&import, "src/generated").unwrap(),
            "now a file"
        );
        assert_eq!(import.outside_editable(), []);
    }

    #[test]
    fn an_editable_file_can_become_a_directory() {
        let fixture = TaskFixture::new("");
        fs::write(fixture.starter().join("src/data"), "").unwrap();
        let submission = fixture.submission();
        fs::remove_file(submission.path().join("src/data")).unwrap();
        fs::create_dir(submission.path().join("src/data")).unwrap();
        fs::write(submission.path().join("src/data/table.rs"), "table").unwrap();

        let import = fixture.build_copy(submission.path()).unwrap();

        assert_eq!(read_project(&import, "src/data/table.rs").unwrap(), "table");
        assert_eq!(import.outside_editable(), []);
    }

    #[test]
    fn an_editable_file_can_become_an_empty_directory() {
        let fixture = TaskFixture::new("");
        let submission = fixture.submission();
        fs::remove_file(submission.path().join("src/old.rs")).unwrap();
        fs::create_dir(submission.path().join("src/old.rs")).unwrap();

        let import = fixture.build_copy(submission.path()).unwrap();

        assert!(import.copy.project().join("src/old.rs").is_dir());
        assert_eq!(import.outside_editable(), []);
    }

    #[test]
    fn empty_editable_directories_are_kept() {
        let fixture = TaskFixture::new("");
        fs::create_dir(fixture.starter().join("src/empty")).unwrap();
        let submission = fixture.submission();
        fs::create_dir(submission.path().join("src/added")).unwrap();

        let import = fixture.build_copy(submission.path()).unwrap();

        assert!(import.copy.project().join("src/empty").is_dir());
        assert!(import.copy.project().join("src/added").is_dir());
        assert_eq!(import.outside_editable(), []);
    }

    #[test]
    fn empty_directories_outside_editable_are_reported() {
        let fixture = TaskFixture::new("");
        fs::create_dir(fixture.starter().join("assets")).unwrap();
        let submission = fixture.submission();
        fs::remove_dir(submission.path().join("assets")).unwrap();
        fs::create_dir(submission.path().join("docs")).unwrap();

        let import = fixture.build_copy(submission.path()).unwrap();

        let edits: Vec<String> = import
            .outside_editable()
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(edits, ["deleted `assets`", "added `docs`"]);
        assert!(import.copy.project().join("assets").is_dir());
        assert!(!import.copy.project().join("docs").exists());
    }

    #[test]
    fn a_type_change_cannot_displace_protected_files() {
        let fixture = TaskFixture::new("");
        let member = fixture.starter().join("src/member");
        fs::create_dir_all(&member).unwrap();
        fs::write(member.join("Cargo.toml"), "trusted").unwrap();
        fs::write(member.join("lib.rs"), "").unwrap();
        let submission = fixture.submission();
        fs::remove_dir_all(submission.path().join("src/member")).unwrap();
        fs::write(submission.path().join("src/member"), "a file").unwrap();

        let import = fixture.build_copy(submission.path()).unwrap();

        assert_eq!(
            read_project(&import, "src/member/Cargo.toml").unwrap(),
            "trusted"
        );
        let edits: Vec<String> = import
            .outside_editable()
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            edits,
            [
                "changed the type of `src/member`",
                "deleted `src/member/Cargo.toml`",
            ]
        );
    }

    #[test]
    fn protected_names_in_another_case_are_not_imported() {
        let fixture = TaskFixture::new("");
        let submission = fixture.submission();
        fs::write(submission.path().join("src/CARGO.TOML"), "").unwrap();

        let import = fixture.build_copy(submission.path()).unwrap();

        assert_eq!(
            import.outside_editable(),
            [OutsideEdit {
                path: "src/CARGO.TOML".to_owned(),
                change: Change::Added,
            }]
        );
        assert_eq!(read_project(&import, "src/CARGO.TOML"), None);
    }

    #[test]
    fn contents_are_compared_across_chunks() {
        let dir = TempDir::new().unwrap();
        let base = vec![7u8; 200 * 1024];
        let mut changed_at_end = base.clone();
        *changed_at_end.last_mut().unwrap() = 8;
        for (name, bytes) in [
            ("base", &base[..]),
            ("same", &base[..]),
            ("end", &changed_at_end[..]),
            ("short", &base[1..]),
        ] {
            fs::write(dir.path().join(name), bytes).unwrap();
        }
        let path = |name| dir.path().join(name);

        assert!(same_contents(&path("base"), &path("same")).unwrap());
        assert!(!same_contents(&path("base"), &path("end")).unwrap());
        assert!(!same_contents(&path("base"), &path("short")).unwrap());
    }

    #[test]
    fn build_output_is_neither_reported_nor_copied() {
        let fixture = TaskFixture::new("");
        let submission = fixture.submission();
        fs::create_dir_all(submission.path().join("target/debug")).unwrap();
        fs::write(submission.path().join("target/debug/demo"), "").unwrap();

        let import = fixture.build_copy(submission.path()).unwrap();

        assert_eq!(import.outside_editable(), []);
        assert!(!import.copy.project().join("target").exists());
    }

    #[test]
    fn fixtures_sit_beside_the_project() {
        let fixture = TaskFixture::new("fixtures = [\"store\"]\n");
        let submission = fixture.submission();

        let import = fixture.build_copy(submission.path()).unwrap();

        let store = import.copy.root.path().join("fixtures/store/src/lib.rs");
        assert_eq!(fs::read_to_string(store).unwrap(), "pub struct Store;");
    }

    #[test]
    fn literal_prefixes_are_editable_roots() {
        let editable = Editable::new(&[EditablePattern::new("src/**").unwrap()]);

        assert!(editable.reaches("src"));
        assert!(editable.reaches("src/nested/link"));
        assert!(!editable.reaches("srcs"));
        assert!(!editable.reaches("docs/link"));

        let nested = Editable::new(&[EditablePattern::new("crates/core/src/*.rs").unwrap()]);
        assert!(nested.reaches("crates"));
        assert!(nested.reaches("crates/core"));
        assert!(!nested.reaches("crates/other"));
    }

    // Creating a symlink on Windows needs developer mode or elevation.
    #[cfg(unix)]
    #[test]
    fn a_link_under_an_editable_path_invalidates_the_submission() {
        let fixture = TaskFixture::new("");
        let submission = fixture.submission();
        std::os::unix::fs::symlink("/etc/hostname", submission.path().join("src/host.rs")).unwrap();

        let error = fixture.build_copy(submission.path()).unwrap_err();

        assert_eq!(
            error.to_string(),
            "the submission's `src/host.rs` is a symlink under an editable path"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_submission_root_that_is_a_link_is_not_followed() {
        let fixture = TaskFixture::new("");
        let real = fixture.submission();
        let links = TempDir::new().unwrap();
        let link = links.path().join("submission");
        std::os::unix::fs::symlink(real.path(), &link).unwrap();

        let error = fixture.build_copy(&link).unwrap_err();

        assert_eq!(
            error.to_string(),
            "the submission is a symlink, not a directory"
        );
    }

    #[test]
    fn a_submission_root_that_is_a_file_is_invalid() {
        let fixture = TaskFixture::new("");
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("submission");
        fs::write(&file, "").unwrap();

        let error = fixture.build_copy(&file).unwrap_err();

        assert!(
            matches!(error, ImportError::SubmissionRoot(EntryKind::File)),
            "{error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_link_outside_editable_is_reported_and_not_followed() {
        let fixture = TaskFixture::new("");
        let submission = fixture.submission();
        std::os::unix::fs::symlink("/etc", submission.path().join("docs")).unwrap();

        let import = fixture.build_copy(submission.path()).unwrap();

        assert_eq!(
            import.outside_editable(),
            [OutsideEdit {
                path: "docs".to_owned(),
                change: Change::Added,
            }]
        );
        assert!(!import.copy.project().join("docs").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_the_starter_is_a_task_error() {
        let fixture = TaskFixture::new("");
        let submission = fixture.submission();
        std::os::unix::fs::symlink("lib.rs", fixture.starter().join("src/alias.rs")).unwrap();

        let error = fixture.build_copy(submission.path()).unwrap_err();

        assert_eq!(
            error.to_string(),
            "`starter/src/alias.rs` is a symlink; only files and directories are copied"
        );
    }
}
