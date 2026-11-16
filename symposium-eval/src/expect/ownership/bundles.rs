//! Independent checks of compiled bundles. Neither the delivered bundle nor the
//! compiled cache is a source of truth: every payload is compared with `evals/`.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::json;

use super::{GITIGNORE, GITIGNORE_CONTENTS, MARKER, relative, same_file};
use crate::expect::file;
use crate::task::{BundleIdentity, SkillName};
use crate::tree::{self, EntryKind, TreeEntry};

type ExpectedTree = BTreeMap<String, EntryKind>;

pub(super) fn symposium_bundles(
    project: &Path,
    source_of: &impl Fn(&SkillName) -> Option<(PathBuf, BundleIdentity)>,
) -> Vec<String> {
    let mut owned: Vec<_> = [".claude/skills", ".symposium/plugins"]
        .into_iter()
        .flat_map(|root| owned_in(project, root, source_of))
        .collect();
    // Compilation creates only this file at the project metadata root. Exclude that
    // file individually, so an extra file beside `plugins/` remains an agent edit.
    if owned
        .iter()
        .any(|path| path.starts_with(".symposium/plugins/"))
        && matches_bytes(&project.join(".symposium/.gitignore"), GITIGNORE_CONTENTS)
            .unwrap_or(false)
    {
        owned.push(".symposium/.gitignore".to_owned());
    }
    owned
}

fn owned_in(
    project: &Path,
    root: &str,
    source_of: &impl Fn(&SkillName) -> Option<(PathBuf, BundleIdentity)>,
) -> Vec<String> {
    let root = project.join(root);
    // A tree walk does not follow child links; establish the same for its root.
    if !real_directories(project, &root) {
        return Vec::new();
    }
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let bundle = entry.path();
            exact_bundle(&bundle, source_of)
                .unwrap_or(false)
                .then(|| relative(project, &bundle))
                .flatten()
        })
        .collect()
}

fn real_directories(project: &Path, root: &Path) -> bool {
    root.ancestors()
        .take_while(|path| *path != project)
        .all(|path| fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir()))
}

fn exact_bundle(
    bundle: &Path,
    source_of: &impl Fn(&SkillName) -> Option<(PathBuf, BundleIdentity)>,
) -> io::Result<bool> {
    if !fs::symlink_metadata(bundle)?.is_dir() {
        return Ok(false);
    }
    let Ok(entries) = tree::walk(bundle) else {
        return Ok(false);
    };
    if entries
        .iter()
        .any(|entry| !matches!(entry.kind, EntryKind::File | EntryKind::Dir))
    {
        return Ok(false);
    }
    let payloads = skill_payloads(&entries);
    if payloads.is_empty() {
        return Ok(false);
    }
    let mut identity = None;
    let mut expected = ExpectedTree::new();
    for payload in payloads {
        let Some((contents, payload_identity)) = payload_tree(bundle, &payload.path, source_of)?
        else {
            return Ok(false);
        };
        if identity
            .as_ref()
            .is_some_and(|identity| identity != &payload_identity)
        {
            return Ok(false);
        }
        identity = Some(payload_identity);
        expected.extend(contents);
    }
    let Some(identity) = identity else {
        return Ok(false);
    };
    let Some(metadata) = metadata_tree(bundle, &entries, &identity)? else {
        return Ok(false);
    };
    expected.extend(metadata);
    Ok(entries.len() == expected.len()
        && entries
            .iter()
            .all(|entry| expected.get(&entry.path) == Some(&entry.kind)))
}

fn metadata_tree(
    bundle: &Path,
    entries: &[TreeEntry],
    identity: &BundleIdentity,
) -> io::Result<Option<ExpectedTree>> {
    if bundle.file_name().and_then(|name| name.to_str()) != Some(identity.name.as_str()) {
        return Ok(None);
    }
    let manifest = serde_json::to_vec_pretty(&json!({
        "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
        "name": identity.name,
        "version": identity.version,
    }))?;
    let mut expected = BTreeMap::from([
        (MARKER.to_owned(), EntryKind::File),
        ("plugin.json".to_owned(), EntryKind::File),
        (".claude-plugin".to_owned(), EntryKind::Dir),
        (".claude-plugin/plugin.json".to_owned(), EntryKind::File),
        ("skills".to_owned(), EntryKind::Dir),
    ]);
    if !matches_bytes(&bundle.join(MARKER), b"")? {
        return Ok(None);
    }
    for path in ["plugin.json", ".claude-plugin/plugin.json"] {
        if !matches_bytes(&bundle.join(path), &manifest)? {
            return Ok(None);
        }
    }
    if entries.iter().any(|entry| entry.path == GITIGNORE) {
        if !matches_bytes(&bundle.join(GITIGNORE), GITIGNORE_CONTENTS)? {
            return Ok(None);
        }
        expected.insert(GITIGNORE.to_owned(), EntryKind::File);
    }
    Ok(Some(expected))
}

fn matches_bytes(path: &Path, expected: &[u8]) -> io::Result<bool> {
    Ok(file::read(path, expected.len() as u64)?.as_deref() == Some(expected))
}

fn skill_payloads(entries: &[TreeEntry]) -> Vec<&TreeEntry> {
    entries
        .iter()
        .filter(|entry| {
            entry.kind == EntryKind::Dir
                && entry
                    .path
                    .strip_prefix("skills/")
                    .is_some_and(|name| !name.contains('/'))
        })
        .collect()
}

fn payload_tree(
    bundle: &Path,
    payload: &str,
    source_of: &impl Fn(&SkillName) -> Option<(PathBuf, BundleIdentity)>,
) -> io::Result<Option<(ExpectedTree, BundleIdentity)>> {
    let Ok(skill) = SkillName::new(&payload["skills/".len()..]) else {
        return Ok(None);
    };
    let Some((source, identity)) = source_of(&skill) else {
        return Ok(None);
    };
    let Ok(contents) = tree::walk(&source) else {
        return Ok(None);
    };
    let mut expected = BTreeMap::from([(payload.to_owned(), EntryKind::Dir)]);
    for entry in contents {
        if entry.path == MARKER || entry.path == GITIGNORE {
            continue;
        }
        let path = format!("{payload}/{}", entry.path);
        if !matches!(entry.kind, EntryKind::File | EntryKind::Dir)
            || (entry.kind == EntryKind::File
                && (!fs::symlink_metadata(bundle.join(&path))?.is_file()
                    || !same_file(&bundle.join(&path), &source.join(&entry.path))?))
        {
            return Ok(None);
        }
        expected.insert(path, entry.kind);
    }
    Ok(Some((expected, identity)))
}

#[cfg(test)]
mod tests;
