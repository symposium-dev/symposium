//! Watch sets, fingerprints, and on-disk cache for custom predicate results.
//!
//! A predicate emits [`CustomPredicateEvent`]s naming inputs whose changes
//! invalidate its result. Those events are unioned into a [`WatchSet`]. When
//! the predicate runs, we take a [`Fingerprints`] snapshot of the watched
//! inputs and store the result in a [`PredicateCache`]; on the next sync we
//! compare a fresh snapshot to the stored one and reuse the cached result
//! while they match.
//!
//! Each workspace gets its own cache file at
//! `<cache_dir>/predicates/<hash>.json`, where `<hash>` is a SHA-256 digest of
//! the canonicalized workspace root. This isolation is what lets two
//! workspaces on the same machine share a predicate string (for example
//! `depends-on(serde)`) without stepping on each other's cached answers.
//! `cache_dir` is resolved by [`symposium_sdk::dirs::SymposiumDirs`] and
//! typically expands to `~/.symposium/cache`. The cache is discarded on
//! Symposium version upgrade via the `version` tag written into each file.
//!
//! The cache is consumed by [`crate::predicate::PredicateContext`] via
//! [`PredicateContext::with_disk_cache`](crate::predicate::PredicateContext::with_disk_cache)
//! and [`PredicateContext::persist_disk_cache`](crate::predicate::PredicateContext::persist_disk_cache).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use symposium_sdk::predicate::CustomPredicateEvent;

/// Current schema version of the on-disk cache. Bump when the file layout
/// changes; existing cache files with a different version are treated as
/// empty.
const CACHE_SCHEMA_VERSION: u32 = 1;

/// Subdirectory of `SymposiumDirs::cache_dir` that holds one predicate cache
/// file per workspace.
const PREDICATES_SUBDIR: &str = "predicates";

/// The union of watched inputs and cache lifetime derived from one predicate
/// execution.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WatchSet {
    pub files: BTreeSet<PathBuf>,
    pub env: BTreeSet<String>,
    pub cache_ttl: CacheTtl,
}

/// How long the predicate result may be cached, independent of file / env
/// invalidation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CacheTtl {
    /// No `WatchTime` events were emitted; the result never becomes stale by
    /// time alone.
    #[default]
    Forever,
    /// The shortest `WatchTime(N>0)` reported by the predicate.
    For(Duration),
    /// `WatchTime(0)` was emitted; the result must not be cached.
    Never,
}

impl WatchSet {
    /// Union every event from a single predicate execution into one set.
    pub fn from_events(events: &[CustomPredicateEvent]) -> Self {
        let mut set = Self::default();
        for event in events {
            match event {
                CustomPredicateEvent::WatchFile(path) => {
                    set.files.insert(path.clone());
                }
                CustomPredicateEvent::WatchEnv(name) => {
                    set.env.insert(name.clone());
                }
                CustomPredicateEvent::WatchTime(0) => {
                    set.cache_ttl = CacheTtl::Never;
                }
                CustomPredicateEvent::WatchTime(ms) => {
                    let next = Duration::from_millis(*ms);
                    set.cache_ttl = match set.cache_ttl {
                        CacheTtl::Never => CacheTtl::Never,
                        CacheTtl::Forever => CacheTtl::For(next),
                        CacheTtl::For(current) => CacheTtl::For(current.min(next)),
                    };
                }
                _ => {}
            }
        }
        set
    }
}

/// Snapshot of the watched inputs at a point in time. Two snapshots that
/// compare equal mean nothing observable to the cache has changed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fingerprints {
    #[serde(default)]
    pub files: BTreeMap<PathBuf, FileFingerprint>,
    #[serde(default)]
    pub env: BTreeMap<String, Option<String>>,
}

/// A file's `mtime` in nanoseconds and byte size. Both are `None` when the
/// file is missing so an absent → present transition invalidates the entry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileFingerprint {
    pub mtime_ns: Option<i128>,
    pub size: Option<u64>,
}

impl FileFingerprint {
    /// Fingerprint a file on disk. Missing files, unreadable metadata, and
    /// metadata without a usable timestamp all resolve to a deterministic
    /// `None`-only state; predicates fail to their conservative branch when
    /// the file transitions to a readable state.
    pub fn of(path: &Path) -> Self {
        match fs::metadata(path) {
            Ok(meta) => Self {
                size: Some(meta.len()),
                mtime_ns: meta.modified().ok().and_then(|t| {
                    t.duration_since(std::time::UNIX_EPOCH)
                        .ok()
                        .map(|d| d.as_nanos() as i128)
                        .or_else(|| {
                            std::time::UNIX_EPOCH
                                .duration_since(t)
                                .ok()
                                .map(|d| -(d.as_nanos() as i128))
                        })
                }),
            },
            Err(_) => Self::default(),
        }
    }
}

impl Fingerprints {
    /// Capture fingerprints for every input in `set`. Non-existent files and
    /// missing env vars are stored as their "absent" fingerprint so future
    /// appearances count as an invalidating change.
    pub fn capture(set: &WatchSet) -> Self {
        let files = set
            .files
            .iter()
            .map(|path| (path.clone(), FileFingerprint::of(path)))
            .collect();
        let env = set
            .env
            .iter()
            .map(|name| (name.clone(), std::env::var(name).ok()))
            .collect();
        Self { files, env }
    }
}

/// One cached predicate result. Serialized as an entry inside
/// [`PredicateCache::entries`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheEntry {
    /// Exit-status result (`true` = passed).
    pub result: bool,
    /// Fingerprints of every watched file and env var at the time the result
    /// was captured.
    pub fingerprints: Fingerprints,
    /// Wall-clock deadline in milliseconds since the Unix epoch. `None` means
    /// the entry never becomes stale by time alone.
    #[serde(default)]
    pub stale_at_ms: Option<u64>,
}

impl CacheEntry {
    /// Build an entry from a result and a `WatchSet`. `WatchSet` with
    /// `CacheTtl::Never` produces an entry with `stale_at_ms = Some(0)`, but
    /// the caller is expected to skip persisting such an entry entirely.
    pub fn from_result(result: bool, set: &WatchSet) -> Self {
        let stale_at_ms = match set.cache_ttl {
            CacheTtl::Forever => None,
            CacheTtl::Never => Some(0),
            CacheTtl::For(d) => now_ms().checked_add(d.as_millis() as u64),
        };
        Self {
            result,
            fingerprints: Fingerprints::capture(set),
            stale_at_ms,
        }
    }

    /// True if the entry's TTL has already elapsed at wall-clock `now_ms`.
    pub fn is_time_expired(&self, now: u64) -> bool {
        match self.stale_at_ms {
            Some(deadline) => now >= deadline,
            None => false,
        }
    }
}

/// On-disk cache of custom predicate results.
///
/// Cache entries are keyed by the normalized predicate invocation
/// (`name(arg)`). The cache is discarded when the schema version on disk
/// differs from [`CACHE_SCHEMA_VERSION`], so a Symposium upgrade that changes
/// the layout automatically invalidates existing entries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PredicateCache {
    version: u32,
    #[serde(default)]
    pub entries: BTreeMap<String, CacheEntry>,
}

impl Default for PredicateCache {
    fn default() -> Self {
        Self {
            version: CACHE_SCHEMA_VERSION,
            entries: BTreeMap::new(),
        }
    }
}

impl PredicateCache {
    /// Return the cache file path for the given workspace. The filename is a
    /// SHA-256 digest of the canonicalized workspace root, so different
    /// workspaces on the same machine never collide even when they use the
    /// same predicate string.
    pub fn path_for_workspace(cache_dir: &Path, workspace_root: &Path) -> PathBuf {
        let canonical = workspace_root
            .canonicalize()
            .unwrap_or_else(|_| workspace_root.to_path_buf());
        let mut hasher = Sha256::new();
        hasher.update(canonical.as_os_str().as_encoded_bytes());
        let digest = hex_encode(&hasher.finalize());
        cache_dir
            .join(PREDICATES_SUBDIR)
            .join(format!("{digest}.json"))
    }

    /// Read the cache from disk. Missing files, malformed JSON, or a schema
    /// version mismatch all yield an empty cache; the on-disk error case is
    /// deliberately non-fatal because a stale cache must never break sync.
    pub fn load(path: &Path) -> Self {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(err) => {
                tracing::warn!(?path, error = %err, "failed to read predicate cache");
                return Self::default();
            }
        };
        match serde_json::from_slice::<Self>(&bytes) {
            Ok(cache) if cache.version == CACHE_SCHEMA_VERSION => cache,
            Ok(cache) => {
                tracing::info!(
                    ?path,
                    stored = cache.version,
                    current = CACHE_SCHEMA_VERSION,
                    "predicate cache schema version mismatch; discarding"
                );
                Self::default()
            }
            Err(err) => {
                tracing::warn!(?path, error = %err, "failed to parse predicate cache");
                Self::default()
            }
        }
    }

    /// Look up an entry by its normalized predicate key.
    pub fn get(&self, key: &str) -> Option<&CacheEntry> {
        self.entries.get(key)
    }

    /// Insert or replace an entry for `key`.
    pub fn put(&mut self, key: impl Into<String>, entry: CacheEntry) {
        self.entries.insert(key.into(), entry);
    }

    /// Persist the cache atomically at `path`. Parents are created as needed.
    /// Errors bubble up because a failed write indicates a real problem, but
    /// the caller may downgrade to a warning to avoid failing the outer sync.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating cache directory {}", parent.display()))?;
        }
        let serialized = serde_json::to_vec_pretty(self).context("serializing predicate cache")?;
        // Atomic replace: write to a sibling temp file, fsync, rename.
        let dir = path.parent().unwrap_or_else(|| Path::new("."));
        let mut tmp = tempfile::NamedTempFile::new_in(dir)
            .with_context(|| format!("creating temp file in {}", dir.display()))?;
        tmp.write_all(&serialized)
            .context("writing predicate cache")?;
        tmp.as_file_mut().sync_all().ok();
        tmp.persist(path)
            .map_err(|e| e.error)
            .with_context(|| format!("persisting predicate cache to {}", path.display()))?;
        Ok(())
    }
}

/// Normalized cache key for a predicate invocation. `arg` may be the empty
/// string, matching the parser's convention.
pub fn cache_key(name: &str, arg: &str) -> String {
    format!("{name}({arg})")
}

/// Current wall-clock time in milliseconds since the Unix epoch. Falls back
/// to `0` if the system clock is before the epoch (should never happen).
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Lowercase hex encoding without pulling in the `hex` crate.
fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_time_zero_produces_never_ttl() {
        let events = vec![
            CustomPredicateEvent::WatchTime(60_000),
            CustomPredicateEvent::WatchTime(0),
        ];
        let set = WatchSet::from_events(&events);
        assert_eq!(set.cache_ttl, CacheTtl::Never);
    }

    #[test]
    fn fingerprint_changes_when_file_grows() {
        use std::io::Write;
        let tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp.as_file(), "one").unwrap();

        let set = WatchSet {
            files: [tmp.path().to_path_buf()].into_iter().collect(),
            ..WatchSet::default()
        };
        let before = Fingerprints::capture(&set);

        // Wait long enough for the mtime to move on filesystems with 1s
        // resolution and grow the file so `size` shifts even when mtime does
        // not advance.
        std::thread::sleep(std::time::Duration::from_millis(1_100));
        writeln!(tmp.as_file(), "two").unwrap();
        tmp.as_file().sync_all().unwrap();

        let after = Fingerprints::capture(&set);
        assert_ne!(before, after);
    }

    #[test]
    fn cache_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let path = PredicateCache::path_for_workspace(dir.path(), workspace.path());

        let entry = CacheEntry {
            result: true,
            fingerprints: Fingerprints {
                files: [(
                    PathBuf::from("CargoBrazil.toml"),
                    FileFingerprint {
                        mtime_ns: Some(1_700_000_000_000_000_000),
                        size: Some(42),
                    },
                )]
                .into_iter()
                .collect(),
                env: [("LAMBDA_ENV".to_string(), Some("prod".to_string()))]
                    .into_iter()
                    .collect(),
            },
            stale_at_ms: Some(1_700_000_060_000),
        };
        let mut cache = PredicateCache::default();
        cache.put("depends-on(lambda)", entry.clone());
        cache.save(&path).unwrap();

        let loaded = PredicateCache::load(&path);
        assert_eq!(loaded.get("depends-on(lambda)"), Some(&entry));
    }

    #[test]
    fn cache_version_mismatch_yields_empty() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let path = PredicateCache::path_for_workspace(dir.path(), workspace.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            br#"{"version": 999, "entries": {"stale": {"result": true, "fingerprints": {}}}}"#,
        )
        .unwrap();

        let loaded = PredicateCache::load(&path);
        assert!(loaded.entries.is_empty());
    }
}
