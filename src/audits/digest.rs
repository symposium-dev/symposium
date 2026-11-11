//! The payload digest: a hash of exactly the part of a crate that an audit
//! covers.
//!
//! What an audit reviews is the *plugin payload*: the manifest that says what
//! the crate contributes, and the skill content that manifest names. A crate's
//! code is out of scope, so a release that changes only `src/` keeps the same
//! digest, and one audit covers every such release. The digest is the match
//! key an audits file uses, which is why it must come out identical on every
//! platform and in tooling outside Symposium (`cargo agents plugin digest`
//! exists so that tooling can ask Symposium instead of reimplementing this).
//!
//! # What is hashed
//!
//! A set of named entries, framed and sorted bytewise by name:
//!
//! 1. `Symposium.toml` at the crate root, as the raw bytes, when present.
//! 2. `[package.metadata.symposium]` from `Cargo.toml`, when present, as
//!    canonical compact JSON (keys sorted, no whitespace, TOML datetimes as
//!    strings) under the name `Cargo.toml#package.metadata.symposium`. This is
//!    the only canonicalized part; nothing else in `Cargo.toml` is read.
//! 3. Every file, of any type, in each *skill directory* the manifest names
//!    inside the crate: the `[[skills]] source.path` groups, including the
//!    default `skills/` group unless `[defaults] skills = false`. A skill
//!    directory is one that holds a `SKILL.md`, and its whole tree counts. A
//!    file reachable twice is hashed once. Names are paths relative to the
//!    crate root with `/` separators.
//!
//! The input is the domain-separation prefix `symposium-payload-v1\0`, then
//! per entry `u64` big-endian name length, the name, `u64` big-endian content
//! length, the content. Bytes are hashed as published: no line-ending
//! normalization.
//!
//! Group predicates are not consulted: every group counts, so the digest does
//! not depend on the workspace it is computed in.
//!
//! # When there is no digest
//!
//! A payload that cannot be pinned gets [`Ineligible`], not a digest, and the
//! caller falls back to asking: a `source.git` skill group (what it fetches
//! can change under the same crate release), a skill path that leaves the
//! crate, a broken symlink, a directory loop or an unreadable directory
//! anywhere under a skill group directory (not only inside a skill directory:
//! the whole group is walked to find its skills, and any of these could hide
//! one), an entry in a skill directory that is neither file nor directory, or
//! more than [`MAX_PAYLOAD_BYTES`] of content. Any I/O error other than a plain "not there" (permissions, a
//! dangling link where a manifest or group directory should be) is likewise
//! ineligible: it is never read as absence.
//!
//! Directory traversal is [`crate::dir_walk::walk`], the same walk sync
//! copies skills with: it follows symlinks and refuses loops.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};
use symposium_sdk::audits::Digest;
use symposium_sdk::manifest::RawPluginManifest;

use crate::dir_walk::{EntryKind, Refused};
use crate::plugins::PluginSource;

/// Most content bytes a payload may have and still get a digest (4 MiB).
pub const MAX_PAYLOAD_BYTES: u64 = 4 * 1024 * 1024;

/// Prefix of the hashed input, separating this digest from any other use of
/// SHA-256. The `v1` is the version of this scheme.
const DOMAIN: &[u8] = b"symposium-payload-v1\0";

/// Entry name of the crate-root plugin manifest.
const MANIFEST_ENTRY: &str = "Symposium.toml";

/// Entry name of the canonicalized `[package.metadata.symposium]` table.
const METADATA_ENTRY: &str = "Cargo.toml#package.metadata.symposium";

/// Why a payload has no digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ineligible {
    /// The crate root is not a directory.
    NotADirectory(PathBuf),
    /// A manifest source could not be read or does not validate as a plugin
    /// manifest, so there is no plugin to pin.
    Manifest(String),
    /// A `source.git` skill group: it fetches content the crate release does
    /// not pin.
    GitSkillSource(String),
    /// A skill path (as written in the manifest) resolves outside the crate.
    EscapesCrate(String),
    /// A symlink anywhere under a skill group directory (or where a manifest
    /// or group directory should be) that points nowhere.
    BrokenLink(String),
    /// A directory symlink anywhere under a skill group directory that leads
    /// back into the directory it sits in.
    LinkLoop(String),
    /// An entry in a skill directory that is neither a file nor a directory,
    /// or whose path is not valid UTF-8.
    Unsupported(String),
    /// The payload is larger than [`MAX_PAYLOAD_BYTES`].
    TooLarge,
    /// A file in the payload, or a directory anywhere under a skill group
    /// directory, could not be read.
    Unreadable(String),
}

impl fmt::Display for Ineligible {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotADirectory(p) => write!(f, "{} is not a directory", p.display()),
            Self::Manifest(why) => write!(f, "the plugin manifest is not usable: {why}"),
            Self::GitSkillSource(url) => write!(
                f,
                "a skill group fetches `source.git = \"{url}\"`, which a crate release does not pin"
            ),
            Self::EscapesCrate(path) => {
                write!(f, "skill path `{path}` leaves the crate directory")
            }
            Self::BrokenLink(path) => write!(f, "`{path}` is a broken symlink"),
            Self::LinkLoop(path) => write!(f, "`{path}` is a symlink loop"),
            Self::Unsupported(path) => write!(
                f,
                "`{path}` is not a regular file or directory, or its path is not UTF-8"
            ),
            Self::TooLarge => write!(f, "the payload is larger than {} bytes", MAX_PAYLOAD_BYTES),
            Self::Unreadable(why) => write!(f, "cannot read the payload: {why}"),
        }
    }
}

impl std::error::Error for Ineligible {}

/// The payload digest of the crate whose source is at `root`.
pub fn payload_digest(root: &Path) -> Result<Digest, Ineligible> {
    let entries = payload_entries(root)?;
    Ok(hash_entries(&entries))
}

/// The framed entries of the payload, by name.
fn payload_entries(root: &Path) -> Result<BTreeMap<String, Vec<u8>>, Ineligible> {
    if !root.is_dir() {
        return Err(Ineligible::NotADirectory(root.to_path_buf()));
    }
    let mut payload = Payload::default();

    // 1. The manifest file, raw.
    let manifest_path = root.join(crate::pm::layout::MANIFEST_FILE);
    let manifest_text = if is_present_file(&manifest_path, MANIFEST_ENTRY)? {
        let bytes = payload.read(&manifest_path)?;
        let text = String::from_utf8(bytes.clone())
            .map_err(|_| Ineligible::Manifest("Symposium.toml is not UTF-8".into()))?;
        payload.put(MANIFEST_ENTRY.to_string(), bytes);
        Some(text)
    } else {
        None
    };

    // 2. The metadata table, canonicalized.
    let cargo_toml = root.join("Cargo.toml");
    let metadata = if is_present_file(&cargo_toml, "Cargo.toml")? {
        symposium_pm_cargo::metadata::symposium_metadata(&cargo_toml)
            .map_err(|e| Ineligible::Manifest(format!("{e:#}")))?
    } else {
        None
    };
    if let Some(table) = &metadata {
        let json = canonical_json(&toml::Value::Table(table.clone()));
        payload.charge(json.len() as u64)?;
        payload.put(METADATA_ENTRY.to_string(), json.into_bytes());
    }

    // 3. The skill directories the (merged, validated) manifest names. The
    // merge drops a layer that does not parse, so each is parsed strictly
    // first: otherwise a broken manifest would be digested as its defaults.
    if let Some(text) = &manifest_text {
        toml::from_str::<RawPluginManifest>(text)
            .map_err(|e| Ineligible::Manifest(format!("Symposium.toml: {e}")))?;
    }
    if let Some(table) = &metadata {
        toml::Value::Table(table.clone())
            .try_into::<RawPluginManifest>()
            .map_err(|e| Ineligible::Manifest(format!("[package.metadata.symposium]: {e}")))?;
    }
    let manifest = crate::plugins::package_manifest(metadata, manifest_text.as_deref(), "payload")
        .map_err(|e| Ineligible::Manifest(format!("{e:#}")))?;
    let canonical_root = root
        .canonicalize()
        .map_err(|e| Ineligible::Unreadable(format!("{}: {e}", root.display())))?;
    for group in &manifest.skills {
        let rel = match &group.source {
            PluginSource::Git(url) => return Err(Ineligible::GitSkillSource(url.clone())),
            PluginSource::Path(rel) => rel,
        };
        // A group directory that does not exist contributes nothing.
        let joined = root.join(rel);
        if present(&joined, &rel.display().to_string())?.is_none() {
            continue;
        }
        let group_dir = joined
            .canonicalize()
            .map_err(|e| Ineligible::Unreadable(format!("{}: {e}", joined.display())))?;
        if !group_dir.starts_with(&canonical_root) {
            return Err(Ineligible::EscapesCrate(rel.display().to_string()));
        }
        if group_dir.is_dir() {
            let prefix = group_dir
                .strip_prefix(&canonical_root)
                .unwrap_or(Path::new(""));
            payload.add_group(&group_dir, prefix)?;
        }
    }
    Ok(payload.entries)
}

/// What is at `path` (`name` in errors), links followed: `None` only when
/// nothing is there. A dangling link or any other error is never read as
/// absence.
fn present(path: &Path, name: &str) -> Result<Option<std::fs::Metadata>, Ineligible> {
    match std::fs::metadata(path) {
        Ok(meta) => Ok(Some(meta)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::symlink_metadata(path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Ok(_) => Err(Ineligible::BrokenLink(name.to_string())),
                Err(e) => Err(Ineligible::Unreadable(format!("{name}: {e}"))),
            }
        }
        Err(e) => Err(Ineligible::Unreadable(format!("{name}: {e}"))),
    }
}

/// Whether a manifest file at the crate root is present; one that exists but
/// is not a regular file is refused.
fn is_present_file(path: &Path, name: &str) -> Result<bool, Ineligible> {
    match present(path, name)? {
        Some(meta) if !meta.is_file() => Err(Ineligible::Unsupported(name.to_string())),
        found => Ok(found.is_some()),
    }
}

/// Entries gathered so far, with the running content size.
#[derive(Default)]
struct Payload {
    entries: BTreeMap<String, Vec<u8>>,
    bytes: u64,
}

impl Payload {
    /// Read a file, refusing it before reading if it would push the payload
    /// over the cap.
    fn read(&mut self, path: &Path) -> Result<Vec<u8>, Ineligible> {
        let len = std::fs::metadata(path)
            .map_err(|e| Ineligible::Unreadable(format!("{}: {e}", path.display())))?
            .len();
        self.charge(len)?;
        std::fs::read(path).map_err(|e| Ineligible::Unreadable(format!("{}: {e}", path.display())))
    }

    fn charge(&mut self, len: u64) -> Result<(), Ineligible> {
        self.bytes = self.bytes.saturating_add(len);
        if self.bytes > MAX_PAYLOAD_BYTES {
            Err(Ineligible::TooLarge)
        } else {
            Ok(())
        }
    }

    /// Add an entry. A name present already (a file reached through two
    /// groups) keeps its first bytes.
    fn put(&mut self, name: String, bytes: Vec<u8>) {
        self.entries.entry(name).or_insert(bytes);
    }

    /// Add every file in each skill directory under the group directory
    /// `dir`, whose entry names start with `prefix`. The whole group is walked
    /// to find its skill directories, so a broken link or a loop anywhere in it
    /// is refused: either could hide a skill. An installed copy (carrying
    /// symposium's marker) is never a source, so its subtree is ignored.
    fn add_group(&mut self, dir: &Path, prefix: &Path) -> Result<(), Ineligible> {
        let tree = crate::dir_walk::walk(dir).map_err(|e| Ineligible::Unreadable(e.to_string()))?;
        let has_file = |dir: &Path, name: &str| {
            tree.entries.iter().any(|(p, kind)| {
                *kind == EntryKind::File && p.parent() == Some(dir) && p.ends_with(name)
            })
        };
        let dirs = std::iter::once(Path::new("")).chain(
            tree.entries
                .iter()
                .filter(|(_, kind)| *kind == EntryKind::Dir)
                .map(|(p, _)| p.as_path()),
        );
        let (installed, skills): (Vec<&Path>, Vec<&Path>) = dirs
            .filter(|d| has_file(d, crate::sync::MARKER_FILE) || has_file(d, "SKILL.md"))
            .partition(|d| has_file(d, crate::sync::MARKER_FILE));
        let skills: Vec<&Path> = skills
            .into_iter()
            .filter(|d| !installed.iter().any(|i| d.starts_with(i)))
            .collect();
        let name = |rel: &Path| {
            let full = prefix.join(rel);
            let parts: Option<Vec<&str>> =
                full.components().map(|c| c.as_os_str().to_str()).collect();
            parts
                .map(|p| p.join("/"))
                .ok_or_else(|| Ineligible::Unsupported(full.display().to_string()))
        };
        for refused in &tree.refused {
            let (Refused::BrokenLink(rel, _) | Refused::Loop(rel) | Refused::Special(rel)) =
                refused;
            if installed.iter().any(|i| rel.starts_with(i)) {
                continue;
            }
            match refused {
                Refused::BrokenLink(..) => return Err(Ineligible::BrokenLink(name(rel)?)),
                Refused::Loop(_) => return Err(Ineligible::LinkLoop(name(rel)?)),
                // Only skill content must be a file or a directory.
                Refused::Special(_) if skills.iter().any(|s| rel.starts_with(s)) => {
                    return Err(Ineligible::Unsupported(name(rel)?));
                }
                Refused::Special(_) => {}
            }
        }
        for (rel, kind) in &tree.entries {
            if *kind != EntryKind::File || !skills.iter().any(|s| rel.starts_with(s)) {
                continue;
            }
            let name = name(rel)?;
            // The synthesized metadata entry's name is reserved.
            if name == METADATA_ENTRY {
                return Err(Ineligible::Unsupported(name));
            }
            // Reached through another group already: same name, same file, so
            // it is neither read nor counted twice.
            if !self.entries.contains_key(&name) {
                let bytes = self.read(&dir.join(rel))?;
                self.put(name, bytes);
            }
        }
        Ok(())
    }
}

/// Hash the framed entries (already sorted by name, bytewise).
fn hash_entries(entries: &BTreeMap<String, Vec<u8>>) -> Digest {
    let mut hasher = Sha256::new();
    hasher.update(DOMAIN);
    for (name, content) in entries {
        hasher.update((name.len() as u64).to_be_bytes());
        hasher.update(name.as_bytes());
        hasher.update((content.len() as u64).to_be_bytes());
        hasher.update(content);
    }
    Digest::from_bytes(hasher.finalize().into())
}

/// Compact JSON with object keys sorted bytewise. TOML has no `null`; a
/// datetime is its string form and a non-finite float is spelled as a string,
/// since JSON has no number for it.
fn canonical_json(value: &toml::Value) -> String {
    fn convert(value: &toml::Value) -> serde_json::Value {
        match value {
            toml::Value::String(s) => s.as_str().into(),
            toml::Value::Integer(i) => (*i).into(),
            toml::Value::Float(f) => serde_json::Number::from_f64(*f)
                .map_or_else(|| f.to_string().into(), serde_json::Value::Number),
            toml::Value::Boolean(b) => (*b).into(),
            toml::Value::Datetime(d) => d.to_string().into(),
            toml::Value::Array(items) => items.iter().map(convert).collect(),
            // Sorted here, whatever order the map keeps (`serde_json` is built
            // with `preserve_order`, so insertion order is output order).
            toml::Value::Table(table) => table
                .iter()
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .map(|(key, item)| (key.clone(), convert(item)))
                .collect::<serde_json::Map<_, _>>()
                .into(),
        }
    }
    convert(value).to_string()
}

#[cfg(test)]
mod tests;
