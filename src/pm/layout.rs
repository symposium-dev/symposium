//! Registry directory layout: how a registry source's directory tree maps to
//! plugin-bearing entries.
//!
//! A registry is a collection of *entries* — one per plugin or standalone
//! skill directory. Which directories constitute entries is packaging
//! convention and lives here, in the package-manager layer; *interpreting*
//! an entry's manifest (the TOML schema, predicates, gating) stays in
//! [`crate::plugins`].
//!
//! The flat layout defined here — every directory containing a
//! [`MANIFEST_FILE`] or [`SKILL_FILE`] is an entry, discovered recursively,
//! and a claimed directory is not recursed into — backs [`PathPm`](super::PathPm).
//! The recommendations convention (pm-named namespace directories) is its own
//! PM, [`RecommendationsPm`](super::RecommendationsPm), which reuses the
//! classification and walk defined here.

use std::path::{Path, PathBuf};

use anyhow::Result;

/// Plugin manifest filename that marks a directory as a plugin entry.
pub const MANIFEST_FILE: &str = "SYMPOSIUM.toml";

/// Skill file that marks a directory as a standalone-skill entry.
pub const SKILL_FILE: &str = "SKILL.md";

/// What kind of entry a directory is.
#[derive(Debug)]
pub enum EntryKind {
    /// A plugin entry; carries the path to its `SYMPOSIUM.toml`.
    Plugin(PathBuf),
    /// A standalone-skill entry; carries the path to its `SKILL.md`.
    Skill(PathBuf),
}

/// Classify a directory as an entry, or `None` when it is neither.
/// [`MANIFEST_FILE`] takes precedence over [`SKILL_FILE`].
pub fn classify(dir: &Path) -> Option<EntryKind> {
    let manifest = dir.join(MANIFEST_FILE);
    if manifest.is_file() {
        return Some(EntryKind::Plugin(manifest));
    }
    let skill_md = dir.join(SKILL_FILE);
    if skill_md.is_file() {
        return Some(EntryKind::Skill(skill_md));
    }
    None
}

/// One entry in a registry source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryEntry {
    /// The entry directory, relative to the source root.
    pub subpath: PathBuf,
    /// For recommendations `cargo/<name>/` entries: the dependency this
    /// entry recommends a plugin for.
    pub recommends: Option<String>,
}

/// Enumerate the entries in the flat-layout registry source rooted at `root`,
/// sorted by subpath. A missing root yields no entries; a root that is itself
/// an entry is an error (a source should *contain* plugins, not be one).
pub fn enumerate(root: &Path) -> Result<Vec<RegistryEntry>> {
    match classify(root) {
        Some(EntryKind::Plugin(_)) => anyhow::bail!(
            "plugin source root contains SYMPOSIUM.toml — it should contain subdirectories with plugins, not be a plugin itself: {}",
            root.display()
        ),
        Some(EntryKind::Skill(_)) => anyhow::bail!(
            "plugin source root contains SKILL.md — it should contain subdirectories with skills, not be a skill itself: {}",
            root.display()
        ),
        None => {}
    }
    let mut entries = Vec::new();
    walk(root, Path::new(""), &mut entries);
    entries.sort_by(|a, b| a.subpath.cmp(&b.subpath));
    Ok(entries)
}

/// Recursively collect entry directories under `dir`, unsorted and without
/// the root guard. Subpaths are relative to `rel`.
pub(crate) fn walk(dir: &Path, rel: &Path, entries: &mut Vec<RegistryEntry>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let sub = rel.join(entry.file_name());
        if classify(&path).is_some() {
            entries.push(RegistryEntry {
                subpath: sub,
                recommends: None,
            });
        } else {
            walk(&path, &sub, entries);
        }
    }
}

/// A subpath as it appears in a package id: slash-separated, so ids are
/// stable across platforms.
pub(crate) fn subpath_key(subpath: &Path) -> String {
    subpath
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }

    #[test]
    fn flat_layout_finds_nested_entries_with_pruning() {
        let tmp = tempfile::tempdir().unwrap();
        touch(&tmp.path().join("plug/SYMPOSIUM.toml"));
        // Claimed as a plugin — the nested skill is not a separate entry.
        touch(&tmp.path().join("plug/inner/SKILL.md"));
        touch(&tmp.path().join("group/deep/skill/SKILL.md"));

        let entries = enumerate(tmp.path()).unwrap();
        let subpaths: Vec<_> = entries.iter().map(|e| e.subpath.clone()).collect();
        assert_eq!(
            subpaths,
            vec![PathBuf::from("group/deep/skill"), PathBuf::from("plug")]
        );
        assert!(entries.iter().all(|e| e.recommends.is_none()));

        // Missing root: no entries, no error.
        assert!(enumerate(&tmp.path().join("nope")).unwrap().is_empty());
    }

    #[test]
    fn flat_layout_rejects_root_that_is_an_entry() {
        let tmp = tempfile::tempdir().unwrap();
        touch(&tmp.path().join("SKILL.md"));
        let err = enumerate(tmp.path()).unwrap_err();
        assert!(err.to_string().contains("not be a skill itself"));
    }
}
