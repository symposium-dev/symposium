//! Audits: what the community list says about a package's plugin payload.
//!
//! The list is the builtin `symposium-audits` git registry, read from its
//! cache directory ([`Symposium::audits_dir`]): one file per package at
//! `audits/<pm>/<name>.toml`, in the format [`symposium_sdk::audits`]
//! defines. It is the only place audits are read from. This module is their
//! only reader: the package managers stay ignorant of audits.
//!
//! The lookup ([`AuditIndex::lookup`]) gives exactly one [`Answer`]:
//!
//! - **revoked**: a `[[revoked]]` entry matches the version or the payload
//!   digest.
//! - **audited**: otherwise, the digest matches an `[[audit]]` entry.
//! - **not audited**: anything else, including a payload that cannot be
//!   digested and a file that cannot be used.
//!
//! Only a revocation changes what loads: a revoked package never loads, on
//! any route (see [`crate::plugins::active_plugins`] and
//! [`crate::discovery`]). Only packages of the cargo package manager are
//! looked up: a registry or the workspace is a trust root, and a revocation
//! has nothing to say about what the user put there.
//!
//! Looking up a package reads only its own `audits/<pm>/<name>.toml`, so a
//! package nobody audited costs one failed `stat`. Reading is cache-only: the
//! registry is refreshed where every registry is, never here. The payload
//! digest ([`digest`]) is computed only when the file needs it.
//!
//! **The client is lenient** where the list's own CI is strict: unknown keys
//! are ignored, a malformed entry (a bad digest, an entry that names both or
//! neither of `versions` and `digest`, an unparsable requirement, a missing
//! `reason`) is dropped with a warning naming the file and the entry, and a
//! file whose `format` is missing or unknown is ignored whole. A `reason` code
//! this version does not know is not malformed: the revocation still applies,
//! with a warning, so a list that adds a code never fails open here. Warnings
//! go to `tracing`, never to stdout, because this runs on the hook path where
//! stdout belongs to the agent. A dropped revocation fails open, which is why
//! its warning names exactly what was dropped.
//!
//! [`Symposium::audits_dir`]: crate::config::Symposium::audits_dir

use std::path::{Path, PathBuf};

use symposium_install::UpdateLevel;
use symposium_sdk::audits::{AUDITS_DIR, Digest, FORMAT, RawRevoked, RevokeReason};
use symposium_sdk::pm::{CARGO_PM, PackageId};

use crate::config::{BUILTIN_AUDITS_REGISTRY, Symposium};
use crate::pm::PmRegistry;

pub mod digest;
pub mod digest_command;

/// A file larger than this is ignored whole, with a warning: audits are small,
/// and the file is read whole. Documented in the audits reference, since an
/// oversized file loses its revocations too.
const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// The audits list, read on demand from the root of the registry holding it.
#[derive(Debug, Clone, Default)]
pub struct AuditIndex {
    /// The registry root; audits live in `<dir>/audits/`. `None` when there
    /// is no list, so every answer is "not audited".
    dir: Option<PathBuf>,
}

/// What the lookup says about one package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// The list revokes this release or payload: it never loads.
    Revoked(Revocation),
    /// The list audited this payload.
    Audited,
    /// Neither: no file, no matching entry, or no digest to match.
    NotAudited,
}

/// A revocation that matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revocation {
    pub reason: RevokeReason,
}

/// The sentence the activation report uses: who issued the revocation and
/// why.
impl std::fmt::Display for Revocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "revoked by registry `{BUILTIN_AUDITS_REGISTRY}`: it {}",
            self.reason.sentence()
        )
    }
}

impl AuditIndex {
    /// The list kept in the registry rooted at `dir`.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: Some(dir.into()),
        }
    }

    /// The configured list: the builtin `symposium-audits` registry's cache,
    /// or no list while that registry is off.
    pub fn configured(sym: &Symposium) -> Self {
        Self {
            dir: sym.audits_dir(),
        }
    }

    /// Everything the list says about the package `name` of package manager
    /// `pm`: reads `audits/<pm>/<name>.toml`.
    pub fn for_package(&self, pm: &str, name: &str) -> PackageAudits {
        let (Some(dir), Some(rel)) = (&self.dir, audit_file_path(pm, name)) else {
            return PackageAudits::default();
        };
        let path = dir.join(AUDITS_DIR).join(rel);
        let Some(text) = read_small(&path) else {
            return PackageAudits::default();
        };
        let (parsed, warnings) = parse_file(&text);
        for warning in warnings {
            tracing::warn!(path = %path.display(), "{warning}");
        }
        parsed.unwrap_or_default()
    }

    /// The lookup: what the list says about release `version` of the package
    /// `name` of package manager `pm`, whose payload digest is `digest`
    /// (`None` when the payload cannot be digested).
    pub fn lookup(&self, pm: &str, name: &str, version: &str, digest: Option<&Digest>) -> Answer {
        let version = semver::Version::parse(version).ok();
        self.for_package(pm, name).answer(version.as_ref(), digest)
    }

    /// Whether the list revokes the loaded package `id`. Every route that
    /// loads a dependency's plugin asks this, so it must stay cheap: the
    /// payload is digested only when no version revocation matched and the
    /// file holds a digest revocation, and only cache-only.
    pub async fn revocation(&self, pms: &PmRegistry, id: &PackageId) -> Option<Revocation> {
        if id.pm != CARGO_PM {
            return None;
        }
        let audits = self.for_package(&id.pm, &id.name);
        let version = semver::Version::parse(&id.version).ok();
        if let Some(hit) = audits.revocation(version.as_ref(), None) {
            return Some(hit);
        }
        if !audits.has_digest_revocation() {
            return None;
        }
        let digest = loaded_digest(pms, id).await?;
        audits.revocation(None, Some(&digest))
    }

    /// The full [`Answer`] for the loaded package `id`. The payload is
    /// digested only when no version revocation matched and the file holds
    /// an audit or a digest revocation, the only entries a digest could
    /// match.
    pub async fn answer(&self, pms: &PmRegistry, id: &PackageId) -> Answer {
        if id.pm != CARGO_PM {
            return Answer::NotAudited;
        }
        let audits = self.for_package(&id.pm, &id.name);
        let version = semver::Version::parse(&id.version).ok();
        if let Some(hit) = audits.revocation(version.as_ref(), None) {
            return Answer::Revoked(hit);
        }
        if audits.audited.is_empty() && !audits.has_digest_revocation() {
            return Answer::NotAudited;
        }
        let digest = loaded_digest(pms, id).await;
        audits.answer(version.as_ref(), digest.as_ref())
    }
}

/// The payload digest of the package `id` already loaded, read from where its
/// package manager keeps it (cache-only). `None` when it cannot be found or
/// cannot be digested, which never matches a digest.
///
/// The fetch asks for the name at any version first, which is how the package
/// was loaded: for a workspace dependency that is the source `cargo metadata`
/// resolved (a path, git or `[patch]` override included), where an exact
/// version would look in the registry cache instead. Only when that answers
/// with another release is the exact one asked for, pinned with `=`: cargo
/// reads a bare `1.5.3` as `^1.5.3`, which would serve a cached 1.6.0. A fetch
/// that still answers with another release yields no digest rather than the
/// digest of a payload that did not load.
async fn loaded_digest(pms: &PmRegistry, id: &PackageId) -> Option<Digest> {
    let fetched = match pms
        .fetch(&PackageId::any_version(&id.pm, &id.name), UpdateLevel::None)
        .await
    {
        Ok(fetched) if fetched.id.version == id.version => Ok(fetched),
        _ => pms.fetch(&exact_release(id), UpdateLevel::None).await,
    };
    let fetched = match fetched {
        Ok(fetched) if fetched.id.version == id.version => fetched,
        Ok(fetched) => {
            tracing::debug!(id = %id, served = %fetched.id, "no payload of this release to digest");
            return None;
        }
        Err(e) => {
            tracing::debug!(id = %id, error = %format!("{e:#}"), "no payload to digest");
            return None;
        }
    };
    match digest::payload_digest(&fetched.root) {
        Ok(digest) => Some(digest),
        Err(why) => {
            tracing::debug!(id = %id, %why, "no payload digest; digest entries cannot match");
            None
        }
    }
}

/// `id` as a requirement that only its own release meets: `=<version>` when
/// the version is semver, `id` unchanged otherwise.
fn exact_release(id: &PackageId) -> PackageId {
    match semver::Version::parse(&id.version) {
        Ok(version) => PackageId::new(&id.pm, &id.name, format!("={version}")),
        Err(_) => id.clone(),
    }
}

/// What the list says about one package.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageAudits {
    /// Every audited payload.
    audited: Vec<Digest>,
    revoked: Vec<RevokedEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RevokedEntry {
    selector: Selector,
    reason: RevokeReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Selector {
    Versions(semver::VersionReq),
    Digest(Digest),
}

impl PackageAudits {
    /// No audits and no revocations.
    pub fn is_empty(&self) -> bool {
        self.audited.is_empty() && self.revoked.is_empty()
    }

    /// The answer for this release and payload. A revocation wins over an
    /// audit, so a digest both audited and revoked is revoked.
    pub fn answer(&self, version: Option<&semver::Version>, digest: Option<&Digest>) -> Answer {
        if let Some(hit) = self.revocation(version, digest) {
            Answer::Revoked(hit)
        } else if digest.is_some_and(|d| self.audited.contains(d)) {
            Answer::Audited
        } else {
            Answer::NotAudited
        }
    }

    /// Does any revocation name a payload digest (so the payload must be
    /// hashed to evaluate them)?
    pub fn has_digest_revocation(&self) -> bool {
        self.revoked
            .iter()
            .any(|r| matches!(r.selector, Selector::Digest(_)))
    }

    /// The first revocation matching this version or digest. A version that
    /// is not semver (`None`) never matches a requirement; a pre-release is
    /// matched as its release too (see [`revokes_version`]).
    pub fn revocation(
        &self,
        version: Option<&semver::Version>,
        digest: Option<&Digest>,
    ) -> Option<Revocation> {
        self.revoked
            .iter()
            .find(|r| match &r.selector {
                Selector::Versions(req) => version.is_some_and(|v| revokes_version(req, v)),
                Selector::Digest(d) => digest == Some(d),
            })
            .map(|r| Revocation { reason: r.reason })
    }
}

/// Whether a revocation's requirement covers `version`. semver (and cargo)
/// lets a pre-release meet only a requirement that names a pre-release of the
/// same release, so `*` would not cover `2.0.0-alpha.1` and no requirement
/// could cover future pre-releases. That rule is for choosing a version; for a
/// list of what must not load it fails open, so a pre-release is also matched
/// as its release: `1.5.0-rc.1` is revoked by any requirement `1.5.0` meets.
fn revokes_version(req: &semver::VersionReq, version: &semver::Version) -> bool {
    req.matches(version)
        || (!version.pre.is_empty()
            && req.matches(&semver::Version::new(
                version.major,
                version.minor,
                version.patch,
            )))
}

/// The path of a package's audits file inside `audits/`: `<pm>/<name>.toml`,
/// with `name` lowercased and nothing else changed (hyphens stay hyphens).
/// `None` when `pm` or `name` is not a plain name, so a name can never reach
/// outside the `audits/` directory.
pub fn audit_file_path(pm: &str, name: &str) -> Option<PathBuf> {
    let plain = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    };
    let name = name.to_ascii_lowercase();
    (plain(pm) && plain(&name)).then(|| Path::new(pm).join(format!("{name}.toml")))
}

fn read_small(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    if meta.len() > MAX_FILE_BYTES {
        tracing::warn!(path = %path.display(), "ignoring oversized audits file");
        return None;
    }
    match std::fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "cannot read audits file");
            None
        }
    }
}

/// Parse one audits file leniently, returning what it yields (`None` ignores
/// the whole file) and a warning per thing dropped. Warnings are returned so
/// the caller decides where they go (it adds the file's path), and so tests
/// can read them.
fn parse_file(text: &str) -> (Option<PackageAudits>, Vec<String>) {
    let mut warnings = Vec::new();
    let table: toml::Table = match text.parse() {
        Ok(t) => t,
        Err(e) => {
            warnings.push(format!("ignoring audits file that is not valid TOML: {e}"));
            return (None, warnings);
        }
    };
    match table.get("format").and_then(toml::Value::as_integer) {
        Some(f) if f == i64::from(FORMAT) => {}
        other => {
            warnings.push(format!(
                "ignoring audits file with unsupported format {} (this symposium reads format {FORMAT})",
                other.map_or("(missing)".to_string(), |f| f.to_string()),
            ));
            return (None, warnings);
        }
    }

    let mut out = PackageAudits::default();
    for (i, value) in array(&table, "audit", &mut warnings)
        .into_iter()
        .enumerate()
    {
        match audit_entry(value) {
            Ok(digest) => out.audited.push(digest),
            Err(why) => warnings.push(format!("dropping [[audit]] entry #{}: {why}", i + 1)),
        }
    }
    for (i, value) in array(&table, "revoked", &mut warnings)
        .into_iter()
        .enumerate()
    {
        match revoked_entry(value) {
            Ok((selector, reason)) => {
                if reason == RevokeReason::Unknown {
                    let code = value.get("reason").and_then(toml::Value::as_str);
                    warnings.push(format!(
                        "[[revoked]] entry #{} has reason `{}`, which this symposium does not know; it still revokes",
                        i + 1,
                        code.unwrap_or_default(),
                    ));
                }
                out.revoked.push(RevokedEntry { selector, reason });
            }
            Err(why) => warnings.push(format!("dropping [[revoked]] entry #{}: {why}", i + 1)),
        }
    }
    (Some(out), warnings)
}

/// The entries of the `[[key]]` array of tables, empty when the key is absent.
/// A key that is present but not an array (`[revoked]` for `[[revoked]]`, or
/// `revoked = "x"`) yields nothing too, but says so: dropping it silently would
/// lose every revocation in the file because of one missing bracket.
fn array<'a>(
    table: &'a toml::Table,
    key: &str,
    warnings: &mut Vec<String>,
) -> Vec<&'a toml::Value> {
    match table.get(key) {
        None => Vec::new(),
        Some(toml::Value::Array(a)) => a.iter().collect(),
        Some(other) => {
            warnings.push(format!(
                "ignoring `{key}`: expected an array of tables ([[{key}]]), found {}",
                other.type_str()
            ));
            Vec::new()
        }
    }
}

/// An audit's digest, or why the entry is malformed. Only `digest` is read:
/// `versions` is bookkeeping the client never matches on, so a malformed one
/// must not cost the entry its digest.
fn audit_entry(value: &toml::Value) -> Result<Digest, String> {
    let table = value
        .as_table()
        .ok_or_else(|| format!("expected a table, found {}", value.type_str()))?;
    let digest = table.get("digest").ok_or("missing `digest`")?;
    let digest = digest
        .as_str()
        .ok_or_else(|| format!("`digest` must be a string, found {}", digest.type_str()))?;
    digest.parse().map_err(|e| format!("{e}"))
}

/// A revocation's selector and reason, or why the entry is malformed.
fn revoked_entry(value: &toml::Value) -> Result<(Selector, RevokeReason), String> {
    let raw: RawRevoked = value.clone().try_into().map_err(|e| e.to_string())?;
    let selector = match (raw.versions, raw.digest) {
        (Some(req), None) => Selector::Versions(
            semver::VersionReq::parse(&req)
                .map_err(|e| format!("`versions = \"{req}\"` is not a version requirement: {e}"))?,
        ),
        (None, Some(digest)) => Selector::Digest(digest.parse().map_err(|e| format!("{e}"))?),
        (Some(_), Some(_)) => return Err("names both `versions` and `digest`".to_string()),
        (None, None) => return Err("names neither `versions` nor `digest`".to_string()),
    };
    Ok((selector, raw.reason))
}

#[cfg(test)]
mod tests;
