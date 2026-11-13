//! Keeping what a trial left under `results/` (RFD reference, "Retained results").
//!
//! Every source is reached from the runner-owned evaluation root, and every component
//! is inspected without following links before each read. Recursive walks apply the
//! same rule to every entry. Retention also requires a quiescent trial: after process
//! supervision becomes uncertain, path-based evidence is not read.
//!
//! This closes symlink and Windows reparse-point traversal. Hard links and mount points
//! are excluded by the sandbox, which gives submissions neither a host source for a
//! hard link nor mount capability. Publication is atomic: a failed retention leaves no
//! destination behind.

#[cfg(test)]
mod tests;

use std::fs::{self, File};
use std::io::{self, BufWriter, Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Component, Path, PathBuf};

use tempfile::TempDir;
use thiserror::Error;

use super::{Done, RESULT_FILE};
use crate::run::Configuration;
use crate::tree::{EntryKind, TreeEntry, WalkError};

pub(crate) const SKIPPED_FILE: &str = "retention-skipped.txt";

#[derive(Debug, Error)]
pub(crate) enum RetainError {
    #[error("`{}` already exists", .0.display())]
    Exists(PathBuf),
    #[error("the trial is not quiescent, so its paths cannot be retained safely")]
    NotQuiescent,
    #[error("`{}` is outside the runner-owned evaluation root", .0.display())]
    OutsideAnchor(PathBuf),
    #[error("`{}` contains a path component retention does not accept", .0.display())]
    UnsafePath(PathBuf),
    #[error("`{}` is missing, linked, or not a regular file", .0.display())]
    Required(PathBuf),
    #[error("cannot inspect `{}`", .path.display())]
    Inspect {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cannot create `{}`", .path.display())]
    Create {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cannot keep `{}`", .path.display())]
    Copy {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

struct Sources<'a> {
    anchor: &'a Path,
    root: &'a Path,
    root_from_anchor: PathBuf,
}

#[derive(Debug)]
enum Found {
    Missing,
    Entry(EntryKind),
    Blocked { at: PathBuf, kind: EntryKind },
}

struct Keeper<'a> {
    sources: Sources<'a>,
    staging: TempDir,
    skipped: Vec<String>,
}

impl Done {
    /// Copies this trial's retained evidence into a new `destination`.
    ///
    /// # Errors
    ///
    /// When containment is uncertain, the trial is outside its anchor, a required
    /// artifact is missing or unsafe, inspection fails, or output cannot be written.
    pub(crate) fn retain(&self, destination: &Path) -> Result<(), RetainError> {
        if !self.quiescent {
            return Err(RetainError::NotQuiescent);
        }
        let sources = Sources::new(&self.anchor, &self.root)?;
        sources.require_dir(Path::new(""))?;
        let mut keeper = Keeper::new(sources, destination)?;
        self.keep(&mut keeper)?;
        keeper.publish(destination)
    }

    fn keep(&self, keeper: &mut Keeper<'_>) -> Result<(), RetainError> {
        keeper.required(Path::new(RESULT_FILE), RESULT_FILE)?;
        keeper.file(Path::new("srt.json"), "srt.json")?;
        keeper.tree(Path::new("project"), "submission", |path| {
            path != "target" && !path.starts_with("target/")
        })?;
        keeper.tree(Path::new("grading/logs"), "grading", |_| true)?;
        if let Some(session) = &self.session {
            keeper.required(&self.relative(&session.transcript)?, "transcript.jsonl")?;
            keeper.required(&self.relative(&session.stderr)?, "stderr.log")?;
            if let Some(response) = &session.response {
                keeper.write("response.txt", response.as_bytes())?;
            }
        }
        if self.configuration == Configuration::Symposium {
            keeper.file(Path::new("evidence/hook-trace.jsonl"), "hook-trace.jsonl")?;
            keeper.tree(Path::new("evidence"), "evidence", |_| true)?;
            keeper.joined_logs(Path::new("home/.symposium/logs"), "symposium.log")?;
        }
        Ok(())
    }

    fn relative(&self, path: &Path) -> Result<PathBuf, RetainError> {
        path.strip_prefix(&self.root)
            .map(Path::to_owned)
            .map_err(|_| RetainError::OutsideAnchor(path.to_owned()))
    }
}

impl<'a> Sources<'a> {
    fn new(anchor: &'a Path, root: &'a Path) -> Result<Self, RetainError> {
        if !anchor.is_absolute() || !root.is_absolute() {
            return Err(RetainError::OutsideAnchor(root.to_owned()));
        }
        let root_from_anchor = root
            .strip_prefix(anchor)
            .map_err(|_| RetainError::OutsideAnchor(root.to_owned()))?
            .to_owned();
        validate_relative(&root_from_anchor)?;
        if root_from_anchor.as_os_str().is_empty() {
            return Err(RetainError::OutsideAnchor(root.to_owned()));
        }
        Ok(Self {
            anchor,
            root,
            root_from_anchor,
        })
    }

    fn inspect(&self, relative: &Path) -> Result<Found, RetainError> {
        validate_relative(relative)?;
        let target = self.root.join(relative);
        let mut current = self.anchor.to_owned();
        match inspect_one(&current)? {
            Found::Entry(EntryKind::Dir) => {}
            Found::Entry(kind) => return Ok(Found::Blocked { at: current, kind }),
            other => return Ok(other),
        }
        for component in self
            .root_from_anchor
            .components()
            .chain(relative.components())
        {
            let Component::Normal(name) = component else {
                unreachable!("relative paths were validated")
            };
            current.push(name);
            match inspect_one(&current)? {
                Found::Entry(kind) if current == target => return Ok(Found::Entry(kind)),
                Found::Entry(EntryKind::Dir) => {}
                Found::Entry(kind) => return Ok(Found::Blocked { at: current, kind }),
                other => return Ok(other),
            }
        }
        Ok(Found::Entry(EntryKind::Dir))
    }

    fn require_dir(&self, relative: &Path) -> Result<(), RetainError> {
        match self.inspect(relative)? {
            Found::Entry(EntryKind::Dir) => Ok(()),
            _ => Err(RetainError::Required(self.path(relative))),
        }
    }

    fn path(&self, relative: &Path) -> PathBuf {
        self.root.join(relative)
    }
}

impl<'a> Keeper<'a> {
    fn new(sources: Sources<'a>, destination: &Path) -> Result<Self, RetainError> {
        match inspect_one(destination)? {
            Found::Missing => {}
            Found::Entry(_) | Found::Blocked { .. } => {
                return Err(RetainError::Exists(destination.to_owned()));
            }
        }
        let parent = destination.parent().unwrap_or(Path::new("."));
        fs::create_dir_all(parent).map_err(|source| create(parent, source))?;
        let staging = tempfile::Builder::new()
            .prefix(".retaining-")
            .tempdir_in(parent)
            .map_err(|source| create(parent, source))?;
        Ok(Self {
            sources,
            staging,
            skipped: Vec::new(),
        })
    }

    fn staging_dir(&self) -> &Path {
        self.staging.path()
    }

    fn required(&self, source: &Path, name: &str) -> Result<(), RetainError> {
        match self.sources.inspect(source)? {
            Found::Entry(EntryKind::File) => self.copy(source, name),
            _ => Err(RetainError::Required(self.sources.path(source))),
        }
    }

    fn inspect(&mut self, source: &Path, name: &str) -> Option<EntryKind> {
        match self.sources.inspect(source) {
            Ok(Found::Missing) => None,
            Ok(Found::Entry(kind)) => Some(kind),
            Ok(Found::Blocked { at, kind }) => {
                self.skip(name, &format!("{kind} in parent `{}`", at.display()));
                None
            }
            Err(error) => {
                self.skip(name, &format!("cannot inspect: {error}"));
                None
            }
        }
    }

    fn file(&mut self, source: &Path, name: &str) -> Result<(), RetainError> {
        match self.inspect(source, name) {
            None => Ok(()),
            Some(EntryKind::File) => self.copy(source, name),
            Some(other) => {
                self.skip(name, &other.to_string());
                Ok(())
            }
        }
    }

    fn tree(
        &mut self,
        source: &Path,
        name: &str,
        keep: impl Fn(&str) -> bool,
    ) -> Result<(), RetainError> {
        let Some(entries) = self.listing(source, name) else {
            return Ok(());
        };
        let destination = self.staging_dir().join(name);
        fs::create_dir(&destination).map_err(|source| create(&destination, source))?;
        for entry in entries.into_iter().filter(|entry| keep(&entry.path)) {
            let to = destination.join(&entry.path);
            match entry.kind {
                EntryKind::Dir => fs::create_dir(&to).map_err(|source| create(&to, source))?,
                EntryKind::File => {
                    self.copy(&source.join(&entry.path), &format!("{name}/{}", entry.path))?
                }
                EntryKind::Symlink | EntryKind::Other => {
                    self.skip(&format!("{name}/{}", entry.path), &entry.kind.to_string());
                }
            }
        }
        Ok(())
    }

    fn joined_logs(&mut self, source: &Path, name: &str) -> Result<(), RetainError> {
        let Some(entries) = self.listing(source, name) else {
            return Ok(());
        };
        let joined = self.staging_dir().join(name);
        let file = File::create_new(&joined).map_err(|source| create(&joined, source))?;
        let mut out = BufWriter::new(file);
        for entry in entries {
            if entry.kind != EntryKind::File || entry.path.contains('/') {
                self.skip(&format!("{name}: {}", entry.path), &entry.kind.to_string());
                continue;
            }
            let relative = source.join(&entry.path);
            if !matches!(
                self.sources.inspect(&relative)?,
                Found::Entry(EntryKind::File)
            ) {
                self.skip(&format!("{name}: {}", entry.path), "changed before reading");
                continue;
            }
            let path = self.sources.path(&relative);
            append_log(&mut out, &entry.path, &path)
                .map_err(|source| RetainError::Copy { path, source })?;
        }
        out.flush().map_err(|source| RetainError::Copy {
            path: joined,
            source,
        })
    }

    fn listing(&mut self, source: &Path, name: &str) -> Option<Vec<TreeEntry>> {
        match self.inspect(source, name)? {
            EntryKind::Dir => {}
            other => {
                self.skip(name, &other.to_string());
                return None;
            }
        }
        walk(&self.sources, source)
            .inspect_err(|error| self.skip(name, &error.to_string()))
            .ok()
    }

    fn copy(&self, source: &Path, name: &str) -> Result<(), RetainError> {
        if !matches!(self.sources.inspect(source)?, Found::Entry(EntryKind::File)) {
            return Err(RetainError::Required(self.sources.path(source)));
        }
        let from = self.sources.path(source);
        let to = self.staging_dir().join(name);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(|source| create(parent, source))?;
        }
        fs::copy(&from, &to)
            .map(drop)
            .map_err(|source| RetainError::Copy { path: from, source })
    }

    fn write(&self, name: &str, bytes: &[u8]) -> Result<(), RetainError> {
        let path = self.staging_dir().join(name);
        fs::write(&path, bytes).map_err(|source| RetainError::Copy { path, source })
    }

    fn skip(&mut self, what: &str, why: &str) {
        self.skipped.push(format!("{what}: {why}\n"));
    }

    fn publish(self, destination: &Path) -> Result<(), RetainError> {
        if !self.skipped.is_empty() {
            self.write(SKIPPED_FILE, self.skipped.concat().as_bytes())?;
        }
        fs::rename(self.staging_dir(), destination)
            .map_err(|source| create(destination, source))?;
        let _ = self.staging.keep();
        Ok(())
    }
}

fn walk(sources: &Sources<'_>, root: &Path) -> Result<Vec<TreeEntry>, WalkError> {
    let mut entries = Vec::new();
    walk_into(sources, root, Path::new(""), &mut entries)?;
    Ok(entries)
}

fn walk_into(
    sources: &Sources<'_>,
    root: &Path,
    prefix: &Path,
    entries: &mut Vec<TreeEntry>,
) -> Result<(), WalkError> {
    let relative = root.join(prefix);
    let shown = show(prefix);
    sources
        .require_dir(&relative)
        .map_err(|error| WalkError::Inspect {
            path: shown.clone(),
            source: io::Error::other(error),
        })?;
    let mut children = Vec::new();
    for child in fs::read_dir(sources.path(&relative)).map_err(|source| WalkError::Read {
        path: shown.clone(),
        source,
    })? {
        let child = child.map_err(|source| WalkError::Read {
            path: shown.clone(),
            source,
        })?;
        children.push(child.file_name());
    }
    children.sort();

    for name in children {
        let name = name.into_string().map_err(|name| WalkError::NonUtf8 {
            path: show(&prefix.join(name.to_string_lossy().as_ref())),
        })?;
        let child = prefix.join(name);
        let child_relative = root.join(&child);
        let child_shown = show(&child);
        let kind = match sources
            .inspect(&child_relative)
            .map_err(|error| WalkError::Inspect {
                path: child_shown.clone(),
                source: io::Error::other(error),
            })? {
            Found::Entry(kind) | Found::Blocked { kind, .. } => kind,
            Found::Missing => {
                return Err(WalkError::Inspect {
                    path: child_shown,
                    source: io::ErrorKind::NotFound.into(),
                });
            }
        };
        entries.push(TreeEntry {
            path: child_shown,
            kind,
        });
        if kind == EntryKind::Dir {
            walk_into(sources, root, &child, entries)?;
        }
    }
    Ok(())
}

fn show(path: &Path) -> String {
    if path.as_os_str().is_empty() {
        ".".to_owned()
    } else {
        path.components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    }
}

fn validate_relative(path: &Path) -> Result<(), RetainError> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        Ok(())
    } else {
        Err(RetainError::UnsafePath(path.to_owned()))
    }
}

fn inspect_one(path: &Path) -> Result<Found, RetainError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Found::Entry(EntryKind::of_metadata(&metadata))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Found::Missing),
        Err(source) => Err(RetainError::Inspect {
            path: path.to_owned(),
            source,
        }),
    }
}

fn append_log(out: &mut impl io::Write, name: &str, path: &Path) -> io::Result<()> {
    writeln!(out, "== {name} ==")?;
    let mut log = File::open(path)?;
    if io::copy(&mut log, out)? > 0 && !ends_with_newline(&mut log)? {
        writeln!(out)?;
    }
    Ok(())
}

fn ends_with_newline(file: &mut File) -> io::Result<bool> {
    file.seek(SeekFrom::End(-1))?;
    let mut last = [0];
    file.read_exact(&mut last)?;
    Ok(last == *b"\n")
}

fn create(path: &Path, source: io::Error) -> RetainError {
    RetainError::Create {
        path: path.to_owned(),
        source,
    }
}
