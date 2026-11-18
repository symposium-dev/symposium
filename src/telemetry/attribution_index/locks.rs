//! Lock domain for installed skill slots and attribution indexes.
//!
//! Writers acquire locks in one fixed order:
//!
//! 1. the configuration-local sync lock;
//! 2. the user-global parent-registry lock;
//! 3. sorted logical parent locks;
//! 4. sorted physical parent locks after desired parents are established.
//!
//! The registry lock makes parent planning and metadata-absence decisions one
//! coherent snapshot. Writers release it after acquiring every physical lock;
//! readers retain it through attribution combination. The logical phase
//! serializes creation through one path spelling. The physical phase collapses
//! aliases onto the canonical parent before any skill, marker, or index
//! mutation. Readers open only existing registry and physical locks and never
//! create a parent, lock file, or index directory.
//!
//! Lock-domain version 1 is deliberately stable, including the registry
//! filename and encoded parent keys. A future encoding must keep acquiring
//! the v1 locks as well as their replacements while old binaries may still
//! run, otherwise the two versions would stop excluding each other.

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
    fs::{self, File, OpenOptions},
    io,
    marker::PhantomData,
    path::{Path, PathBuf},
};

use sha2::{Digest as _, Sha256};

use crate::config::Symposium;

const LOCK_DIRECTORY: &str = "installation-locks";
const CONFIG_LOCK_FILE: &str = "sync.lock";
const REGISTRY_LOCK_FILE: &str = "v1-registry.lock";
const LOCK_KEY_DOMAIN: &[u8] = b"symposium-installed-parent-lock\0v1\0";

/// Configuration-local exclusion acquired before any installation locks.
///
/// The file handle owns the operating-system lock. This value is neither
/// cloneable nor constructible outside this module.
pub(crate) struct ConfigSyncGuard {
    _file: File,
}

/// One skills parent that the writer may need to inspect or mutate.
///
/// `desired` means the parent must be established before physical locking.
/// Cleanup-only parents remain absent when they do not already exist.
pub(crate) struct ParentLockRequest {
    path: PathBuf,
    desired: bool,
}

impl ParentLockRequest {
    #[must_use]
    pub(crate) fn desired(path: PathBuf) -> Self {
        Self {
            path,
            desired: true,
        }
    }

    #[must_use]
    pub(crate) fn existing(path: PathBuf) -> Self {
        Self {
            path,
            desired: false,
        }
    }
}

/// A desired parent that could not be established.
///
/// Deliberately omits `Debug`: the path is private working state. The reason
/// is content-free and may be logged independently.
pub(crate) struct ExcludedParent {
    path: PathBuf,
    reason: ParentExclusionReason,
}

impl ExcludedParent {
    #[must_use]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub(crate) const fn reason(&self) -> ParentExclusionReason {
        self.reason
    }
}

/// Content-free reason a desired parent was excluded before physical locks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParentExclusionReason {
    Create(io::ErrorKind),
    Unsafe,
}

/// Content-free failure while acquiring the installation lock domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParentLockError {
    ConfigDirectory(io::ErrorKind),
    ConfigLock(io::ErrorKind),
    UnsafeConfigDirectory,
    LockRoot(io::ErrorKind),
    UnsafeLockRoot,
    UnsafeLockFile,
    InvalidParent,
    InspectParent(io::ErrorKind),
    Acquire(io::ErrorKind),
    Revalidate,
}

impl fmt::Display for ParentLockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConfigDirectory(_) => {
                formatter.write_str("failed to prepare the sync lock directory")
            }
            Self::ConfigLock(_) => formatter.write_str("failed to acquire the sync lock"),
            Self::UnsafeConfigDirectory => {
                formatter.write_str("sync lock directory is not a real directory")
            }
            Self::LockRoot(_) => formatter.write_str("failed to prepare the parent lock root"),
            Self::UnsafeLockRoot => formatter.write_str("parent lock root is not a real directory"),
            Self::UnsafeLockFile => formatter.write_str("parent lock file is not a real file"),
            Self::InvalidParent => formatter.write_str("skills parent is not an absolute path"),
            Self::InspectParent(_) => formatter.write_str("failed to inspect a skills parent"),
            Self::Acquire(_) => formatter.write_str("failed to acquire a skills-parent lock"),
            Self::Revalidate => {
                formatter.write_str("skills parent changed while its locks were acquired")
            }
        }
    }
}

impl Error for ParentLockError {}

/// Complete writer acquisition. No guard escapes until every lock succeeds.
///
/// Desired-parent creation failures are the exception: those parents are
/// excluded before the physical phase, while established parents continue.
pub(crate) struct ParentWriteAcquisition {
    guards: ParentWriteGuards,
    excluded: Vec<ExcludedParent>,
}

impl ParentWriteAcquisition {
    #[must_use]
    pub(crate) fn guards(&self) -> &ParentWriteGuards {
        &self.guards
    }

    #[must_use]
    pub(crate) fn excluded(&self) -> &[ExcludedParent] {
        &self.excluded
    }
}

/// Owns every logical and physical writer lock for one mutation pass.
///
/// The lock files stay here rather than in individual tokens. A token borrows
/// this complete set, so dropping any intermediate value cannot release only
/// part of the exclusion domain.
pub(crate) struct ParentWriteGuards {
    _logical_files: Vec<File>,
    _physical_files: Vec<File>,
    covered: BTreeMap<PathBuf, PathBuf>,
}

impl ParentWriteGuards {
    /// Borrow proof that this guard set covers `requested` and its canonical
    /// destination parent.
    #[must_use]
    pub(crate) fn parent(&self, requested: &Path) -> Option<LockedParent<'_>> {
        let (requested, canonical) = self.covered.get_key_value(requested)?;
        Some(LockedParent {
            requested: requested.as_path(),
            canonical,
            _guards: PhantomData,
        })
    }
}

/// Non-cloneable proof that one live writer guard set covers a parent.
pub(crate) struct LockedParent<'guards> {
    requested: &'guards Path,
    canonical: &'guards Path,
    _guards: PhantomData<&'guards ParentWriteGuards>,
}

impl LockedParent<'_> {
    #[must_use]
    pub(crate) fn path(&self) -> &Path {
        self.canonical
    }

    #[must_use]
    pub(crate) fn requested_path(&self) -> &Path {
        self.requested
    }
}

/// Owns every shared physical lock used by one lookup snapshot.
pub(super) struct ParentReadGuards {
    _registry_file: Option<File>,
    _physical_files: Vec<File>,
    candidates: Vec<ReadCandidateRecord>,
}

impl ParentReadGuards {
    pub(super) fn candidates(&self) -> impl Iterator<Item = ReadParentCandidate<'_>> {
        self.candidates
            .iter()
            .map(|candidate| match candidate.state {
                ReadCandidateState::Locked => ReadParentCandidate::Locked(ReadLockedParent {
                    path: &candidate.path,
                    _guards: PhantomData,
                }),
                ReadCandidateState::Unlocked => ReadParentCandidate::Unlocked(&candidate.path),
                ReadCandidateState::Unavailable => ReadParentCandidate::Unavailable,
            })
    }
}

struct ReadCandidateRecord {
    path: PathBuf,
    state: ReadCandidateState,
}

#[derive(Clone, Copy)]
enum ReadCandidateState {
    Locked,
    Unlocked,
    Unavailable,
}

/// One planned parent after its existing physical lock was attempted.
///
/// `Unlocked` means the permanent lock did not exist. The caller may check
/// only whether index metadata is absent; present metadata is unavailable and
/// must never be read without a guard.
pub(super) enum ReadParentCandidate<'guards> {
    Locked(ReadLockedParent<'guards>),
    Unlocked(&'guards Path),
    Unavailable,
}

/// Borrowed proof that a reader holds the physical lock for one parent.
pub(super) struct ReadLockedParent<'guards> {
    path: &'guards Path,
    _guards: PhantomData<&'guards ParentReadGuards>,
}

impl ReadLockedParent<'_> {
    #[must_use]
    pub(super) fn path(&self) -> &Path {
        self.path
    }
}

/// Acquire the configuration-local lock at the head of writer lock order.
pub(crate) fn acquire_config_guard(sym: &Symposium) -> Result<ConfigSyncGuard, ParentLockError> {
    let directory = sym.cache_dir();
    fs::create_dir_all(directory)
        .map_err(|error| ParentLockError::ConfigDirectory(error.kind()))?;
    ensure_real_directory(directory).map_err(|error| match error {
        RealPathError::Io(kind) => ParentLockError::ConfigDirectory(kind),
        RealPathError::Unsafe => ParentLockError::UnsafeConfigDirectory,
    })?;
    let file =
        open_lock_file(&directory.join(CONFIG_LOCK_FILE), true).map_err(map_config_lock_error)?;
    file.lock()
        .map_err(|error| ParentLockError::ConfigLock(error.kind()))?;
    Ok(ConfigSyncGuard { _file: file })
}

/// Acquire the complete writer lock set in registry-logical-physical order.
///
/// `config` is intentionally required even though its fields are not read:
/// possession proves the configuration-local lock was acquired first.
pub(crate) fn acquire_write_guards(
    sym: &Symposium,
    _config: &ConfigSyncGuard,
    requests: impl IntoIterator<Item = ParentLockRequest>,
) -> Result<ParentWriteAcquisition, ParentLockError> {
    acquire_write_guards_in(lock_root(sym), requests)
}

fn acquire_write_guards_in(
    lock_root: PathBuf,
    requests: impl IntoIterator<Item = ParentLockRequest>,
) -> Result<ParentWriteAcquisition, ParentLockError> {
    prepare_lock_root(&lock_root)?;
    let registry_file =
        open_lock_file(&lock_root.join(REGISTRY_LOCK_FILE), true).map_err(map_parent_lock_error)?;
    registry_file
        .lock()
        .map_err(|error| ParentLockError::Acquire(error.kind()))?;

    let mut requested = BTreeMap::<PathBuf, bool>::new();
    for request in requests {
        if !request.path.is_absolute() {
            return Err(ParentLockError::InvalidParent);
        }
        requested
            .entry(request.path)
            .and_modify(|desired| *desired |= request.desired)
            .or_insert(request.desired);
    }

    let logical_keys = requested
        .keys()
        .map(|path| (lock_key(LockKind::Logical, path), path))
        .collect::<BTreeMap<_, _>>();
    let logical_files = acquire_exclusive_keys(&lock_root, logical_keys.keys())?;

    let mut excluded = Vec::new();
    let mut established = BTreeMap::new();
    for (path, desired) in requested {
        match establish_parent(&path, desired) {
            Ok(Some(canonical)) => {
                established.insert(path, canonical);
            }
            Ok(None) => {}
            Err(reason) => excluded.push(ExcludedParent { path, reason }),
        }
    }

    let physical_keys = established
        .values()
        .map(|path| lock_key(LockKind::Physical, path))
        .collect::<BTreeSet<_>>();
    let physical_files = acquire_exclusive_keys(&lock_root, physical_keys.iter())?;

    for (requested, canonical) in &established {
        let current = canonical_real_directory(requested).map_err(|error| match error {
            RealPathError::Io(kind) => ParentLockError::InspectParent(kind),
            RealPathError::Unsafe => ParentLockError::Revalidate,
        })?;
        if &current != canonical {
            return Err(ParentLockError::Revalidate);
        }
    }

    // Physical locks now cover every established parent. Releasing the
    // registry lock lets readers plan while mutations remain excluded by the
    // parent locks carried in the returned guard set.
    drop(registry_file);

    Ok(ParentWriteAcquisition {
        guards: ParentWriteGuards {
            _logical_files: logical_files,
            _physical_files: physical_files,
            covered: established,
        },
        excluded,
    })
}

/// Attempt one coherent read guard for every planned canonical parent.
///
/// The shared registry lock is acquired before parent existence or metadata
/// absence is inspected and remains live in the returned guard set. Successful
/// physical guards remain held even when another candidate is contended or
/// unsafe. A missing permanent lock is represented separately: the caller may
/// decide that metadata is absent only after this acquisition pass, but may
/// not read metadata that is present without a guard.
pub(super) fn acquire_read_guards(
    sym: &Symposium,
    parents: impl IntoIterator<Item = PathBuf>,
) -> ParentReadGuards {
    acquire_read_guards_in(&lock_root(sym), parents)
}

fn acquire_read_guards_in(
    lock_root: &Path,
    parents: impl IntoIterator<Item = PathBuf>,
) -> ParentReadGuards {
    let requested = parents.into_iter().collect::<BTreeSet<_>>();
    let mut candidates = Vec::new();
    let registry_file = match ensure_real_directory(lock_root) {
        Ok(()) => match open_lock_file(&lock_root.join(REGISTRY_LOCK_FILE), false) {
            Ok(file) => match file.try_lock_shared() {
                Ok(()) => Some(file),
                Err(_) => {
                    candidates.extend(requested.into_iter().map(|path| ReadCandidateRecord {
                        path,
                        state: ReadCandidateState::Unavailable,
                    }));
                    return ParentReadGuards {
                        _registry_file: None,
                        _physical_files: Vec::new(),
                        candidates,
                    };
                }
            },
            Err(_) => {
                candidates.extend(requested.into_iter().map(|path| ReadCandidateRecord {
                    path,
                    state: ReadCandidateState::Unavailable,
                }));
                return ParentReadGuards {
                    _registry_file: None,
                    _physical_files: Vec::new(),
                    candidates,
                };
            }
        },
        Err(RealPathError::Io(io::ErrorKind::NotFound)) => None,
        Err(_) => {
            candidates.extend(requested.into_iter().map(|path| ReadCandidateRecord {
                path,
                state: ReadCandidateState::Unavailable,
            }));
            return ParentReadGuards {
                _registry_file: None,
                _physical_files: Vec::new(),
                candidates,
            };
        }
    };

    let mut canonical = BTreeSet::new();
    for parent in requested {
        match canonical_real_directory(&parent) {
            Ok(parent) => {
                canonical.insert(parent);
            }
            Err(RealPathError::Io(io::ErrorKind::NotFound)) => {}
            Err(_) => candidates.push(ReadCandidateRecord {
                path: parent,
                state: ReadCandidateState::Unavailable,
            }),
        }
    }

    if registry_file.is_none() {
        candidates.extend(canonical.into_iter().map(|path| ReadCandidateRecord {
            path,
            state: ReadCandidateState::Unlocked,
        }));
        return ParentReadGuards {
            _registry_file: None,
            _physical_files: Vec::new(),
            candidates,
        };
    }

    let keyed = canonical
        .iter()
        .map(|path| (lock_key(LockKind::Physical, path), path.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut files = Vec::new();
    for (key, parent) in keyed {
        let file = match open_lock_file(&lock_root.join(key), false) {
            Ok(file) => file,
            Err(OpenLockError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                candidates.push(ReadCandidateRecord {
                    path: parent,
                    state: ReadCandidateState::Unlocked,
                });
                continue;
            }
            Err(_) => {
                candidates.push(ReadCandidateRecord {
                    path: parent,
                    state: ReadCandidateState::Unavailable,
                });
                continue;
            }
        };
        if file.try_lock_shared().is_err() {
            candidates.push(ReadCandidateRecord {
                path: parent,
                state: ReadCandidateState::Unavailable,
            });
            continue;
        }
        if canonical_real_directory(&parent).is_ok_and(|current| current == parent) {
            candidates.push(ReadCandidateRecord {
                path: parent,
                state: ReadCandidateState::Locked,
            });
            files.push(file);
        } else {
            candidates.push(ReadCandidateRecord {
                path: parent,
                state: ReadCandidateState::Unavailable,
            });
        }
    }

    ParentReadGuards {
        _registry_file: registry_file,
        _physical_files: files,
        candidates,
    }
}

fn establish_parent(path: &Path, desired: bool) -> Result<Option<PathBuf>, ParentExclusionReason> {
    match fs::symlink_metadata(path) {
        Ok(_) => canonical_real_directory(path)
            .map(Some)
            .map_err(|error| match error {
                RealPathError::Io(kind) => ParentExclusionReason::Create(kind),
                RealPathError::Unsafe => ParentExclusionReason::Unsafe,
            }),
        Err(error) if error.kind() == io::ErrorKind::NotFound && !desired => Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(path)
                .map_err(|error| ParentExclusionReason::Create(error.kind()))?;
            canonical_real_directory(path)
                .map(Some)
                .map_err(|error| match error {
                    RealPathError::Io(kind) => ParentExclusionReason::Create(kind),
                    RealPathError::Unsafe => ParentExclusionReason::Unsafe,
                })
        }
        Err(error) => Err(ParentExclusionReason::Create(error.kind())),
    }
}

fn prepare_lock_root(path: &Path) -> Result<(), ParentLockError> {
    fs::create_dir_all(path).map_err(|error| ParentLockError::LockRoot(error.kind()))?;
    ensure_real_directory(path).map_err(|error| match error {
        RealPathError::Io(kind) => ParentLockError::LockRoot(kind),
        RealPathError::Unsafe => ParentLockError::UnsafeLockRoot,
    })?;
    #[cfg(unix)]
    fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o700))
        .map_err(|error| ParentLockError::LockRoot(error.kind()))?;
    Ok(())
}

fn acquire_exclusive_keys<'a>(
    root: &Path,
    keys: impl IntoIterator<Item = &'a String>,
) -> Result<Vec<File>, ParentLockError> {
    let mut files = Vec::new();
    for key in keys {
        let file = open_lock_file(&root.join(key), true).map_err(map_parent_lock_error)?;
        file.lock()
            .map_err(|error| ParentLockError::Acquire(error.kind()))?;
        files.push(file);
    }
    Ok(files)
}

fn open_lock_file(path: &Path, create: bool) -> Result<File, OpenLockError> {
    let open_existing = || {
        let metadata = fs::symlink_metadata(path).map_err(OpenLockError::Io)?;
        if !is_real_file(&metadata) {
            return Err(OpenLockError::Unsafe);
        }
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(OpenLockError::Io)
    };

    let file = if create {
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => open_existing()?,
            Err(error) => return Err(OpenLockError::Io(error)),
        }
    } else {
        open_existing()?
    };

    let path_metadata = fs::symlink_metadata(path).map_err(OpenLockError::Io)?;
    let handle_metadata = file.metadata().map_err(OpenLockError::Io)?;
    if !is_real_file(&path_metadata) || !handle_metadata.is_file() {
        return Err(OpenLockError::Unsafe);
    }
    #[cfg(unix)]
    file.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(0o600))
        .map_err(OpenLockError::Io)?;
    Ok(file)
}

#[derive(Debug)]
enum OpenLockError {
    Io(io::Error),
    Unsafe,
}

fn map_config_lock_error(error: OpenLockError) -> ParentLockError {
    match error {
        OpenLockError::Io(error) => ParentLockError::ConfigLock(error.kind()),
        OpenLockError::Unsafe => ParentLockError::UnsafeLockFile,
    }
}

fn map_parent_lock_error(error: OpenLockError) -> ParentLockError {
    match error {
        OpenLockError::Io(error) => ParentLockError::Acquire(error.kind()),
        OpenLockError::Unsafe => ParentLockError::UnsafeLockFile,
    }
}

fn lock_root(sym: &Symposium) -> PathBuf {
    sym.home_dir().join(".symposium").join(LOCK_DIRECTORY)
}

#[derive(Clone, Copy)]
enum LockKind {
    Logical,
    Physical,
}

impl LockKind {
    const fn label(self) -> &'static [u8] {
        match self {
            Self::Logical => b"logical\0",
            Self::Physical => b"physical\0",
        }
    }

    const fn filename_prefix(self) -> &'static str {
        match self {
            Self::Logical => "v1-logical-",
            Self::Physical => "v1-physical-",
        }
    }
}

fn lock_key(kind: LockKind, path: &Path) -> String {
    let (encoding, bytes) = encoded_path(path.as_os_str());
    lock_key_for_encoded(kind, encoding, &bytes)
}

fn lock_key_for_encoded(kind: LockKind, encoding: &[u8], bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(LOCK_KEY_DOMAIN);
    hasher.update(kind.label());
    hasher.update(encoding);
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut encoded = String::with_capacity(kind.filename_prefix().len() + digest.len() * 2 + 5);
    encoded.push_str(kind.filename_prefix());
    for byte in digest {
        use fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    encoded.push_str(".lock");
    encoded
}

#[cfg(unix)]
fn encoded_path(path: &std::ffi::OsStr) -> (&'static [u8], Vec<u8>) {
    use std::os::unix::ffi::OsStrExt as _;

    (b"unix\0", path.as_bytes().to_vec())
}

#[cfg(windows)]
fn encoded_path(path: &std::ffi::OsStr) -> (&'static [u8], Vec<u8>) {
    use std::os::windows::ffi::OsStrExt as _;

    let mut encoded = Vec::new();
    for unit in path.encode_wide() {
        encoded.extend_from_slice(&unit.to_le_bytes());
    }
    (b"windows-utf16le\0", encoded)
}

#[derive(Debug, Clone, Copy)]
enum RealPathError {
    Io(io::ErrorKind),
    Unsafe,
}

fn canonical_real_directory(path: &Path) -> Result<PathBuf, RealPathError> {
    ensure_real_directory(path)?;
    dunce::canonicalize(path).map_err(|error| RealPathError::Io(error.kind()))
}

fn ensure_real_directory(path: &Path) -> Result<(), RealPathError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| RealPathError::Io(error.kind()))?;
    if is_real_directory(&metadata) {
        Ok(())
    } else {
        Err(RealPathError::Unsafe)
    }
}

fn is_real_directory(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_dir()
        && !metadata.file_type().is_symlink()
        && !has_reparse_point(metadata)
}

fn is_real_file(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_file()
        && !metadata.file_type().is_symlink()
        && !has_reparse_point(metadata)
}

#[cfg(windows)]
fn has_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
const fn has_reparse_point(_: &fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(path: &Path) -> ParentLockRequest {
        ParentLockRequest::desired(path.to_path_buf())
    }

    #[test]
    fn writer_establishes_and_covers_a_desired_parent() {
        let temporary = tempfile::tempdir().unwrap();
        let parent = temporary.path().join("project").join(".agents/skills");
        let acquisition =
            acquire_write_guards_in(temporary.path().join("locks"), [request(&parent)]).unwrap();

        assert!(acquisition.excluded().is_empty());
        let locked = acquisition.guards().parent(&parent).unwrap();
        assert_eq!(locked.requested_path(), parent);
        assert_eq!(locked.path(), dunce::canonicalize(&parent).unwrap());
        assert!(
            acquisition
                .guards()
                .parent(&temporary.path().join("uncovered"))
                .is_none()
        );
    }

    #[test]
    fn failed_parent_creation_is_excluded_before_physical_locking() {
        let temporary = tempfile::tempdir().unwrap();
        let blocked = temporary.path().join("blocked");
        fs::write(&blocked, []).unwrap();
        let excluded = blocked.join("skills");
        let included = temporary.path().join("included");

        let acquisition = acquire_write_guards_in(
            temporary.path().join("locks"),
            [request(&excluded), request(&included)],
        )
        .unwrap();

        assert!(acquisition.guards().parent(&excluded).is_none());
        assert!(acquisition.guards().parent(&included).is_some());
        assert_eq!(acquisition.excluded().len(), 1);
        assert_eq!(acquisition.excluded()[0].path(), excluded);
        assert!(matches!(
            acquisition.excluded()[0].reason(),
            ParentExclusionReason::Create(_)
        ));
    }

    #[test]
    fn logical_and_physical_lock_keys_are_distinct_and_stable() {
        let path = Path::new(r"C:\workspace\.agents\skills");
        let logical = lock_key(LockKind::Logical, path);
        let physical = lock_key(LockKind::Physical, path);

        assert_ne!(logical, physical);
        assert_eq!(logical, lock_key(LockKind::Logical, path));
        assert!(logical.starts_with("v1-logical-"));
        assert!(physical.starts_with("v1-physical-"));
    }

    #[test]
    fn v1_lock_key_vectors_cover_lossless_platform_encodings() {
        let unix = lock_key_for_encoded(
            LockKind::Physical,
            b"unix\0",
            b"/workspace/non-utf8-\xff/.agents/skills",
        );
        let windows_wide = [
            0x43, 0x00, 0x3a, 0x00, 0x5c, 0x00, 0x3d, 0xd8, 0x80, 0xde, 0x5c, 0x00, 0x73, 0x00,
        ];
        let windows = lock_key_for_encoded(LockKind::Physical, b"windows-utf16le\0", &windows_wide);

        assert_eq!(
            unix,
            "v1-physical-286adb11e3437d27800c0aa2247dfaae0311735c29f542471a1f681438f52149.lock"
        );
        assert_eq!(
            windows,
            "v1-physical-74f9812f493adbf7fb0920084d7c833bc21b796ba27d5071db369f7f057a0350.lock"
        );
    }

    #[test]
    fn failed_multi_lock_acquisition_releases_every_prior_lock() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("locks");
        prepare_lock_root(&root).unwrap();
        let first = "a.lock".to_owned();
        let second = "b.lock".to_owned();
        fs::create_dir(root.join(&second)).unwrap();

        assert!(acquire_exclusive_keys(&root, [&first, &second]).is_err());

        let file = open_lock_file(&root.join(first), false).unwrap();
        file.try_lock().unwrap();
    }

    #[test]
    fn existing_only_request_does_not_create_a_parent() {
        let temporary = tempfile::tempdir().unwrap();
        let parent = temporary.path().join("missing");
        let acquisition = acquire_write_guards_in(
            temporary.path().join("locks"),
            [ParentLockRequest::existing(parent.clone())],
        )
        .unwrap();

        assert!(!parent.exists());
        assert!(acquisition.guards().parent(&parent).is_none());
    }

    #[test]
    fn path_aliases_share_one_physical_parent_guard() {
        let temporary = tempfile::tempdir().unwrap();
        let parent = temporary.path().join("project/.agents/skills");
        fs::create_dir_all(&parent).unwrap();
        let alias = parent.join(".");

        let acquisition = acquire_write_guards_in(
            temporary.path().join("locks"),
            [request(&parent), request(&alias)],
        )
        .unwrap();
        let direct = acquisition.guards().parent(&parent).unwrap();
        let through_alias = acquisition.guards().parent(&alias).unwrap();

        assert_eq!(direct.path(), through_alias.path());
    }

    #[test]
    fn held_writer_makes_a_shared_snapshot_contended() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("locks");
        let parent = temporary.path().join("project/.agents/skills");
        let held = acquire_write_guards_in(root.clone(), [request(&parent)]).unwrap();
        let snapshot = acquire_read_guards_in(&root, [parent]);

        assert!(matches!(
            snapshot.candidates().next(),
            Some(ReadParentCandidate::Unavailable)
        ));
        drop(held);
    }

    #[test]
    fn read_snapshot_retains_the_registry_lock_until_combination_finishes() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("locks");
        let parent = temporary.path().join("project/.agents/skills");
        drop(acquire_write_guards_in(root.clone(), [request(&parent)]).unwrap());

        let snapshot = acquire_read_guards_in(&root, [parent]);
        let registry = open_lock_file(&root.join(REGISTRY_LOCK_FILE), false).unwrap();
        assert!(registry.try_lock().is_err());

        drop(snapshot);
        registry.try_lock().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn lock_root_symlink_is_rejected() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().unwrap();
        let real = temporary.path().join("real");
        let link = temporary.path().join("link");
        fs::create_dir(&real).unwrap();
        symlink(&real, &link).unwrap();

        assert!(matches!(
            acquire_write_guards_in(link, std::iter::empty()),
            Err(ParentLockError::UnsafeLockRoot)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn lock_file_symlink_is_rejected() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("locks");
        prepare_lock_root(&root).unwrap();
        let parent = temporary.path().join("parent");
        fs::create_dir(&parent).unwrap();
        let key = lock_key(LockKind::Physical, &dunce::canonicalize(&parent).unwrap());
        let target = temporary.path().join("target");
        fs::write(&target, []).unwrap();
        symlink(&target, root.join(key)).unwrap();
        let snapshot = acquire_read_guards_in(&root, [parent]);

        assert!(matches!(
            snapshot.candidates().next(),
            Some(ReadParentCandidate::Unavailable)
        ));
    }
}
