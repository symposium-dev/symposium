#![cfg_attr(
    not(test),
    expect(
        dead_code,
        unused_imports,
        reason = "sync and hook integration consume the attribution-index foundation"
    )
)]

//! Local bindings between installed skill slots and safe resolution evidence.
//!
//! A binding is a random, per-installation join key. It proves that an index
//! entry describes the content currently occupying one Symposium-owned slot;
//! it is not a content fingerprint. Bindings never enter telemetry rows,
//! subjects, reports, or diagnostics.
//!
//! Agent adapters own `invalid_signal`. This module accepts only an exact,
//! path-safe slot and returns [`InstalledSkillAttribution::NotIndexed`] before
//! joining an invalid value onto a skills parent.

mod locks;

#[cfg_attr(test, allow(unused_imports))]
pub(crate) use locks::{
    ConfigSyncGuard, ExcludedParent, LockedParent, ParentExclusionReason, ParentLockError,
    ParentLockRequest, ParentWriteAcquisition, ParentWriteGuards, acquire_config_guard,
    acquire_write_guards,
};

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
    fs::{self, File},
    io::{self, Read as _, Write as _},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use super::{
    resolution_witness::ResolutionEvidence,
    schema::{PublicSkillCoordinate, SafeSkillAttribution},
};

pub(crate) const INDEX_DIRECTORY_NAME: &str = ".symposium-index";
pub(crate) const MARKER_FILE_NAME: &str = ".symposium";
const INDEX_FILE_NAME: &str = "index.json";
const INDEX_IGNORE_FILE_NAME: &str = ".gitignore";
const INDEX_IGNORE_CONTENTS: &[u8] = b"*\n";
const INDEX_TEMPORARY_PREFIX: &str = ".index-tmp-";
const MARKER_TEMPORARY_PREFIX: &str = ".marker-tmp-";
const INDEX_VERSION: u64 = 1;
const MARKER_VERSION: u64 = 1;
const BINDING_BYTES: usize = 16;
const BINDING_HEX_BYTES: usize = BINDING_BYTES * 2;
const MAX_MARKER_BYTES: usize = 1024;
const MAX_INDEX_BYTES: usize = 512 * 1024;
const MAX_INDEX_ENTRIES: usize = 4096;

/// Random local join key shared by one managed slot and its index entry.
///
/// Deliberately omits `Debug` and `Display`: the binding must not reach a
/// telemetry diagnostic even though it is not itself a telemetry subject.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct InstallationBinding([u8; BINDING_BYTES]);

impl InstallationBinding {
    fn generate() -> Result<Self, getrandom::Error> {
        let mut bytes = [0_u8; BINDING_BYTES];
        getrandom::fill(&mut bytes)?;
        Ok(Self(bytes))
    }
}

impl Serialize for InstallationBinding {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut encoded = [0_u8; BINDING_HEX_BYTES];
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let (output_pairs, []) = encoded.as_chunks_mut::<2>() else {
            unreachable!("installation binding hex length is even")
        };
        for (byte, output) in self.0.iter().zip(output_pairs) {
            output[0] = HEX[usize::from(byte >> 4)];
            output[1] = HEX[usize::from(byte & 0x0f)];
        }

        // Every nibble came from the ASCII-only table above.
        let encoded = str::from_utf8(&encoded).expect("BUG: hex encoding is valid UTF-8");
        serializer.serialize_str(encoded)
    }
}

impl<'de> Deserialize<'de> for InstallationBinding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        if encoded.len() != BINDING_HEX_BYTES
            || !encoded
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(D::Error::custom("invalid installation binding"));
        }

        let mut bytes = [0_u8; BINDING_BYTES];
        let (encoded_pairs, []) = encoded.as_bytes().as_chunks::<2>() else {
            unreachable!("validated installation binding hex length is even")
        };
        for (output, pair) in bytes.iter_mut().zip(encoded_pairs) {
            *output = (decode_hex_nibble(pair[0]) << 4) | decode_hex_nibble(pair[1]);
        }
        Ok(Self(bytes))
    }
}

fn decode_hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        _ => unreachable!("installation bindings validate lowercase hexadecimal first"),
    }
}

/// How one sync result updates an installed skill's ownership marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MarkerAction {
    /// Preserve a valid binding and do not refresh the debounce timestamp.
    Ensure,
    /// Preserve a valid binding while refreshing the debounce timestamp.
    Refresh,
    /// Mint a new binding after installing different content.
    Rotate,
}

/// Observable marker transition, without exposing the binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MarkerChange {
    Preserved,
    Repaired,
    Refreshed,
    Rotated,
}

/// Result of reconciling one managed marker.
pub(crate) struct MarkerReconciliation {
    binding: InstallationBinding,
    change: MarkerChange,
}

impl MarkerReconciliation {
    #[must_use]
    pub(crate) const fn binding(&self) -> InstallationBinding {
        self.binding
    }

    #[must_use]
    pub(crate) const fn change(&self) -> MarkerChange {
        self.change
    }
}

/// Content-free failure while reading or updating one ownership marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MarkerReconcileError {
    UnsafeSlot,
    UnsafeDirectory,
    Missing,
    Read(io::ErrorKind),
    Generate,
    Encode,
    Write(io::ErrorKind),
}

impl fmt::Display for MarkerReconcileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsafeSlot => formatter.write_str("installed skill slot is unsafe"),
            Self::UnsafeDirectory => {
                formatter.write_str("installed skill slot is not a real directory")
            }
            Self::Missing => formatter.write_str("installed skill marker is missing"),
            Self::Read(_) => formatter.write_str("failed to read installed skill marker"),
            Self::Generate => formatter.write_str("failed to generate installation binding"),
            Self::Encode => formatter.write_str("failed to serialize installed skill marker"),
            Self::Write(_) => formatter.write_str("failed to write installed skill marker"),
        }
    }
}

impl Error for MarkerReconcileError {}

/// Preserve, refresh, or rotate the binding for one Symposium-owned slot.
///
/// Marker presence establishes ownership before content is decoded. A
/// malformed, oversized, legacy-empty, or unknown-version marker is therefore
/// repaired without making a new ownership claim.
///
/// # Errors
///
/// Returns a content-free I/O stage or random-generation failure.
pub(crate) fn reconcile_marker(
    parent: &LockedParent<'_>,
    slot: &str,
    action: MarkerAction,
) -> Result<MarkerReconciliation, MarkerReconcileError> {
    validate_slot(slot).map_err(|_| MarkerReconcileError::UnsafeSlot)?;
    let skill_dir = parent.path().join(slot);
    ensure_real_directory(&skill_dir).map_err(|_| MarkerReconcileError::UnsafeDirectory)?;
    reconcile_marker_at_with(&skill_dir, action, || {
        InstallationBinding::generate().map_err(|_| MarkerReconcileError::Generate)
    })
}

fn reconcile_marker_at_with(
    skill_dir: &Path,
    action: MarkerAction,
    generate: impl FnOnce() -> Result<InstallationBinding, MarkerReconcileError>,
) -> Result<MarkerReconciliation, MarkerReconcileError> {
    let marker_path = skill_dir.join(MARKER_FILE_NAME);
    if action == MarkerAction::Rotate {
        let binding = generate()?;
        write_marker(&marker_path, binding)?;
        return Ok(MarkerReconciliation {
            binding,
            change: MarkerChange::Rotated,
        });
    }

    let (binding, read_change) = match read_marker(&marker_path) {
        Ok(Some(binding)) => (binding, None),
        Ok(None) => return Err(MarkerReconcileError::Missing),
        Err(MarkerReadError::Io(kind)) => return Err(MarkerReconcileError::Read(kind)),
        Err(
            MarkerReadError::TooLarge { .. }
            | MarkerReadError::Malformed { .. }
            | MarkerReadError::UnsupportedVersion(_),
        ) => (generate()?, Some(MarkerChange::Repaired)),
        Err(MarkerReadError::Unsafe) => return Err(MarkerReconcileError::UnsafeDirectory),
    };

    if action == MarkerAction::Ensure && read_change.is_none() {
        return Ok(MarkerReconciliation {
            binding,
            change: MarkerChange::Preserved,
        });
    }

    write_marker(&marker_path, binding)?;
    Ok(MarkerReconciliation {
        binding,
        change: read_change.unwrap_or(MarkerChange::Refreshed),
    })
}

fn write_marker(
    marker_path: &Path,
    binding: InstallationBinding,
) -> Result<(), MarkerReconcileError> {
    let encoded = serde_json::to_vec(&MarkerDocument {
        version: MARKER_VERSION,
        binding,
    })
    .map_err(|_| MarkerReconcileError::Encode)?;
    let parent = marker_path
        .parent()
        .ok_or(MarkerReconcileError::UnsafeDirectory)?;
    ensure_real_directory(parent).map_err(|_| MarkerReconcileError::UnsafeDirectory)?;
    ensure_real_file_or_absent(marker_path).map_err(|error| match error {
        RealFileError::Io(kind) => MarkerReconcileError::Write(kind),
        RealFileError::Unsafe => MarkerReconcileError::UnsafeDirectory,
    })?;
    let mut builder = tempfile::Builder::new();
    builder.prefix(MARKER_TEMPORARY_PREFIX);
    let mut temporary = builder
        .tempfile_in(parent)
        .map_err(|error| MarkerReconcileError::Write(error.kind()))?;
    temporary
        .write_all(&encoded)
        .map_err(|error| MarkerReconcileError::Write(error.kind()))?;
    temporary
        .persist(marker_path)
        .map_err(|error| MarkerReconcileError::Write(error.error.kind()))?;
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MarkerDocument {
    version: u64,
    binding: InstallationBinding,
}

#[derive(Deserialize)]
struct VersionEnvelope {
    version: u64,
}

/// Content-free failure while loading a present marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkerReadError {
    Io(io::ErrorKind),
    Unsafe,
    TooLarge { maximum: usize },
    Malformed { line: usize, column: usize },
    UnsupportedVersion(u64),
}

fn read_marker(path: &Path) -> Result<Option<InstallationBinding>, MarkerReadError> {
    let Some(bytes) =
        read_bounded_regular(path, MAX_MARKER_BYTES).map_err(|error| match error {
            BoundedReadError::Io(kind) => MarkerReadError::Io(kind),
            BoundedReadError::TooLarge { maximum } => MarkerReadError::TooLarge { maximum },
            BoundedReadError::Unsafe => MarkerReadError::Unsafe,
        })?
    else {
        return Ok(None);
    };

    let envelope: VersionEnvelope = parse_json(&bytes).map_err(MarkerReadError::from)?;
    if envelope.version != MARKER_VERSION {
        return Err(MarkerReadError::UnsupportedVersion(envelope.version));
    }
    let marker: MarkerDocument = parse_json(&bytes).map_err(MarkerReadError::from)?;
    Ok(Some(marker.binding))
}

impl From<JsonContentError> for MarkerReadError {
    fn from(error: JsonContentError) -> Self {
        Self::Malformed {
            line: error.line,
            column: error.column,
        }
    }
}

/// Safe attribution stored for one installed slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "eligibility", rename_all = "snake_case", deny_unknown_fields)]
enum IndexedAttribution {
    Public { attribution: SafeSkillAttribution },
    Ineligible,
    Unavailable,
}

/// One candidate entry for an installed-attribution index.
///
/// Deliberately omits `Debug` because it contains an installation binding.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstallationIndexEntry {
    slot: String,
    binding: InstallationBinding,
    attribution: IndexedAttribution,
}

impl InstallationIndexEntry {
    /// Project one installed skill's retained evidence into index-safe form.
    #[must_use]
    pub(crate) fn from_skill_evidence(
        slot: String,
        binding: InstallationBinding,
        evidence: Option<&ResolutionEvidence>,
    ) -> Self {
        let Some(resolution) = evidence.and_then(ResolutionEvidence::safe_resolution) else {
            return Self::ineligible(slot, binding);
        };
        let Ok(target) = PublicSkillCoordinate::try_from(resolution.target) else {
            return Self::ineligible(slot, binding);
        };
        Self::public(
            slot,
            binding,
            SafeSkillAttribution::new(target, resolution.path),
        )
    }

    #[must_use]
    pub(in crate::telemetry) fn public(
        slot: String,
        binding: InstallationBinding,
        attribution: SafeSkillAttribution,
    ) -> Self {
        Self {
            slot,
            binding,
            attribution: IndexedAttribution::Public { attribution },
        }
    }

    #[must_use]
    pub(crate) fn ineligible(slot: String, binding: InstallationBinding) -> Self {
        Self {
            slot,
            binding,
            attribution: IndexedAttribution::Ineligible,
        }
    }

    #[must_use]
    pub(crate) fn unavailable(slot: String, binding: InstallationBinding) -> Self {
        Self {
            slot,
            binding,
            attribution: IndexedAttribution::Unavailable,
        }
    }

    #[must_use]
    pub(crate) fn has_same_attribution(&self, other: &Self) -> bool {
        self.attribution == other.attribution
    }

    fn as_unavailable(&self) -> Self {
        Self::unavailable(self.slot.clone(), self.binding)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexDocument {
    version: u64,
    availability: IndexAvailability,
    entries: Vec<InstallationIndexEntry>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum IndexAvailability {
    Complete,
    Unavailable,
}

/// Content-free reason an index could not be prepared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrepareIndexError {
    UnsafeSlot,
    ReservedSlot,
    DuplicateSlot,
    Serialize,
    SizeAccounting,
}

impl fmt::Display for PrepareIndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsafeSlot => formatter.write_str("installation index contains an unsafe slot"),
            Self::ReservedSlot => {
                formatter.write_str("installation index contains the reserved index slot")
            }
            Self::DuplicateSlot => {
                formatter.write_str("installation index contains a duplicate slot")
            }
            Self::Serialize => formatter.write_str("failed to serialize installation index"),
            Self::SizeAccounting => {
                formatter.write_str("installation index size accounting was inconsistent")
            }
        }
    }
}

impl Error for PrepareIndexError {}

/// Completely encoded index ready for one atomic replacement.
///
/// Deliberately omits `Debug` because the encoded document contains bindings.
pub(crate) struct PreparedInstallationIndex {
    encoded: Box<[u8]>,
    unavailable: bool,
    retained_entries: usize,
    unavailable_public_entries: usize,
    omitted_entries: usize,
}

struct MeasuredEntry {
    entry: InstallationIndexEntry,
    unavailable_len: usize,
    public_len: Option<usize>,
}

/// Validate, order, and encode one complete parent index.
///
/// Each entry shape is measured once. Public attribution is then admitted in
/// canonical slot order against a running total, and the selected document is
/// encoded once. Public attribution that cannot fit becomes entry-level
/// unavailable, never ineligible. An inventory whose unavailable baseline
/// cannot fit is encoded as document-level unavailable; it is never
/// misrepresented as a complete empty inventory.
///
/// # Errors
///
/// Returns a content-free slot, duplicate, serialization, or internal size
/// accounting reason.
pub(crate) fn prepare_index(
    entries: Vec<InstallationIndexEntry>,
) -> Result<PreparedInstallationIndex, PrepareIndexError> {
    let input_len = entries.len();
    let mut by_slot = BTreeMap::new();
    for entry in entries {
        validate_slot(&entry.slot)?;
        if by_slot.insert(entry.slot.clone(), entry).is_some() {
            return Err(PrepareIndexError::DuplicateSlot);
        }
    }

    let empty = encode_index(Vec::new())?;
    if by_slot.len() > MAX_INDEX_ENTRIES {
        return prepared_unavailable(input_len);
    }

    let mut measured = Vec::with_capacity(by_slot.len());
    let mut baseline_len = empty.len();
    for entry in by_slot.into_values() {
        let baseline = if matches!(entry.attribution, IndexedAttribution::Public { .. }) {
            entry.as_unavailable()
        } else {
            entry.clone()
        };
        let unavailable_len = serde_json::to_vec(&baseline)
            .map_err(|_| PrepareIndexError::Serialize)?
            .len();
        let public_len = if matches!(entry.attribution, IndexedAttribution::Public { .. }) {
            Some(
                serde_json::to_vec(&entry)
                    .map_err(|_| PrepareIndexError::Serialize)?
                    .len(),
            )
        } else {
            None
        };
        baseline_len = baseline_len
            .checked_add(unavailable_len)
            .and_then(|length| {
                if measured.is_empty() {
                    Some(length)
                } else {
                    length.checked_add(1)
                }
            })
            .ok_or(PrepareIndexError::SizeAccounting)?;
        measured.push(MeasuredEntry {
            entry,
            unavailable_len,
            public_len,
        });
    }

    if baseline_len > MAX_INDEX_BYTES {
        return prepared_unavailable(input_len);
    }

    let mut selected_len = baseline_len;
    let mut unavailable_public_entries = 0;
    let mut selected = Vec::with_capacity(measured.len());
    for measured in measured {
        let MeasuredEntry {
            mut entry,
            unavailable_len,
            public_len,
        } = measured;
        if let Some(public_len) = public_len {
            let candidate_len = selected_len
                .checked_sub(unavailable_len)
                .and_then(|length| length.checked_add(public_len))
                .ok_or(PrepareIndexError::SizeAccounting)?;
            if candidate_len <= MAX_INDEX_BYTES {
                selected_len = candidate_len;
            } else {
                entry.attribution = IndexedAttribution::Unavailable;
                unavailable_public_entries += 1;
            }
        }
        selected.push(entry);
    }

    let encoded = encode_index(selected)?;
    debug_assert_eq!(
        encoded.len(),
        selected_len,
        "BUG: installation-index size accounting must match final encoding"
    );
    if encoded.len() != selected_len || encoded.len() > MAX_INDEX_BYTES {
        return Err(PrepareIndexError::SizeAccounting);
    }

    Ok(PreparedInstallationIndex {
        retained_entries: input_len,
        encoded: encoded.into_boxed_slice(),
        unavailable: false,
        unavailable_public_entries,
        omitted_entries: 0,
    })
}

fn encode_index(entries: Vec<InstallationIndexEntry>) -> Result<Vec<u8>, PrepareIndexError> {
    serde_json::to_vec(&IndexDocument {
        version: INDEX_VERSION,
        availability: IndexAvailability::Complete,
        entries,
    })
    .map_err(|_| PrepareIndexError::Serialize)
}

fn prepared_unavailable(
    omitted_entries: usize,
) -> Result<PreparedInstallationIndex, PrepareIndexError> {
    let encoded = serde_json::to_vec(&IndexDocument {
        version: INDEX_VERSION,
        availability: IndexAvailability::Unavailable,
        entries: Vec::new(),
    })
    .map_err(|_| PrepareIndexError::Serialize)?;
    Ok(PreparedInstallationIndex {
        encoded: encoded.into_boxed_slice(),
        unavailable: true,
        retained_entries: 0,
        unavailable_public_entries: 0,
        omitted_entries,
    })
}

/// Prepare a document-level unavailable index when inventory inspection was
/// incomplete and no complete replacement may be claimed.
pub(crate) fn prepare_unavailable_index() -> Result<PreparedInstallationIndex, PrepareIndexError> {
    prepared_unavailable(0)
}

fn validate_slot(slot: &str) -> Result<(), PrepareIndexError> {
    if !crate::skills::is_safe_dir_name(slot) {
        return Err(PrepareIndexError::UnsafeSlot);
    }
    if is_reserved_skill_slot(slot) {
        return Err(PrepareIndexError::ReservedSlot);
    }
    Ok(())
}

#[must_use]
pub(crate) fn is_reserved_skill_slot(slot: &str) -> bool {
    // Reserve every ASCII case on all platforms so a project remains valid
    // when moved onto a case-insensitive filesystem.
    slot.eq_ignore_ascii_case(INDEX_DIRECTORY_NAME)
}

/// Content-free write stage for one complete index replacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IndexWriteError {
    CreateDirectory(io::ErrorKind),
    WriteIgnore(io::ErrorKind),
    CreateTemporary(io::ErrorKind),
    WriteTemporary(io::ErrorKind),
    Persist(io::ErrorKind),
}

impl fmt::Display for IndexWriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CreateDirectory(_) => {
                formatter.write_str("failed to create installation index directory")
            }
            Self::WriteIgnore(_) => {
                formatter.write_str("failed to write installation index ignore file")
            }
            Self::CreateTemporary(_) => {
                formatter.write_str("failed to create temporary installation index")
            }
            Self::WriteTemporary(_) => {
                formatter.write_str("failed to write temporary installation index")
            }
            Self::Persist(_) => formatter.write_str("failed to replace installation index"),
        }
    }
}

impl Error for IndexWriteError {}

/// Publication detail from making one parent index current.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IndexWriteReport {
    pub(crate) index_replaced: bool,
    pub(crate) unavailable: bool,
    pub(crate) retained_entries: usize,
    pub(crate) unavailable_public_entries: usize,
    pub(crate) omitted_entries: usize,
    pub(crate) temporary_inspection_failures: u64,
    pub(crate) temporary_removal_failures: u64,
}

/// Publish fully prepared bytes for one parent index.
///
/// Canonical bytes that already match the destination are left untouched, so
/// a fully debounced sync does not rewrite the index or its ignore file.
///
/// Concurrent writers are last-writer-wins at the complete-document boundary.
/// A lost race can only make a later binding comparison fail closed to
/// `not_indexed`; the next sync rebuilds the document.
///
/// # Errors
///
/// Returns the content-free filesystem stage that prevented replacement.
pub(crate) fn replace_index(
    parent: &LockedParent<'_>,
    prepared: &PreparedInstallationIndex,
) -> Result<IndexWriteReport, IndexWriteError> {
    replace_index_at(parent.path(), prepared)
}

fn replace_index_at(
    skills_parent: &Path,
    prepared: &PreparedInstallationIndex,
) -> Result<IndexWriteReport, IndexWriteError> {
    let directory = skills_parent.join(INDEX_DIRECTORY_NAME);
    match ensure_real_directory_or_absent(&directory) {
        Ok(true) => {}
        Ok(false) => fs::create_dir(&directory)
            .map_err(|error| IndexWriteError::CreateDirectory(error.kind()))?,
        Err(error) => return Err(IndexWriteError::CreateDirectory(error.kind())),
    }
    let ignore_file = directory.join(INDEX_IGNORE_FILE_NAME);
    if !file_has_contents(&ignore_file, INDEX_IGNORE_CONTENTS) {
        ensure_real_file_or_absent(&ignore_file).map_err(|error| {
            IndexWriteError::WriteIgnore(match error {
                RealFileError::Io(kind) => kind,
                RealFileError::Unsafe => io::ErrorKind::InvalidData,
            })
        })?;
        fs::write(ignore_file, INDEX_IGNORE_CONTENTS)
            .map_err(|error| IndexWriteError::WriteIgnore(error.kind()))?;
    }

    let cleanup = cleanup_temporary_files(&directory);
    let index_file = directory.join(INDEX_FILE_NAME);
    let index_replaced = !file_has_contents(&index_file, &prepared.encoded);
    if index_replaced {
        ensure_real_file_or_absent(&index_file).map_err(|error| {
            IndexWriteError::Persist(match error {
                RealFileError::Io(kind) => kind,
                RealFileError::Unsafe => io::ErrorKind::InvalidData,
            })
        })?;
        let mut builder = tempfile::Builder::new();
        builder.prefix(INDEX_TEMPORARY_PREFIX);
        let mut temporary = builder
            .tempfile_in(&directory)
            .map_err(|error| IndexWriteError::CreateTemporary(error.kind()))?;
        temporary
            .write_all(&prepared.encoded)
            .map_err(|error| IndexWriteError::WriteTemporary(error.kind()))?;
        temporary
            .persist(index_file)
            .map_err(|error| IndexWriteError::Persist(error.error.kind()))?;
    }

    Ok(IndexWriteReport {
        index_replaced,
        unavailable: prepared.unavailable,
        retained_entries: prepared.retained_entries,
        unavailable_public_entries: prepared.unavailable_public_entries,
        omitted_entries: prepared.omitted_entries,
        temporary_inspection_failures: cleanup.inspection_failures,
        temporary_removal_failures: cleanup.removal_failures,
    })
}

fn file_has_contents(path: &Path, expected: &[u8]) -> bool {
    matches!(
        read_bounded_regular(path, expected.len()),
        Ok(Some(actual)) if actual.as_slice() == expected
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct TemporaryCleanup {
    inspection_failures: u64,
    removal_failures: u64,
}

fn cleanup_temporary_files(directory: &Path) -> TemporaryCleanup {
    let mut outcome = TemporaryCleanup::default();
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return outcome,
        Err(_) => {
            outcome.inspection_failures += 1;
            return outcome;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                outcome.inspection_failures += 1;
                continue;
            }
        };
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !name.starts_with(INDEX_TEMPORARY_PREFIX) {
            continue;
        }
        match entry.file_type() {
            Ok(kind) if kind.is_file() => {
                if fs::remove_file(entry.path()).is_err() {
                    outcome.removal_failures += 1;
                }
            }
            Ok(_) => {}
            Err(_) => outcome.inspection_failures += 1,
        }
    }
    outcome
}

/// Best-effort result of removing the exact index files Symposium owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct IndexCleanupReport {
    pub(crate) inspection_failures: u64,
    pub(crate) removal_failures: u64,
    pub(crate) retained_unknown_content: bool,
}

/// Remove only files owned by the installed-attribution index.
///
/// This never recursively removes the directory. Unknown files, directories,
/// and symlinks survive, and consequently keep the directory itself alive.
#[must_use]
pub(crate) fn remove_index(parent: &LockedParent<'_>) -> IndexCleanupReport {
    remove_index_at(parent.path())
}

fn remove_index_at(skills_parent: &Path) -> IndexCleanupReport {
    let directory = skills_parent.join(INDEX_DIRECTORY_NAME);
    let mut report = IndexCleanupReport::default();
    match ensure_real_directory_or_absent(&directory) {
        Ok(true) => {}
        Ok(false) => return report,
        Err(_) => {
            report.inspection_failures += 1;
            report.retained_unknown_content = true;
            return report;
        }
    }
    remove_owned_file(&directory.join(INDEX_FILE_NAME), &mut report);
    remove_owned_file(&directory.join(INDEX_IGNORE_FILE_NAME), &mut report);
    let temporary = cleanup_temporary_files(&directory);
    report.inspection_failures += temporary.inspection_failures;
    report.removal_failures += temporary.removal_failures;

    let mut entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return report,
        Err(_) => {
            report.inspection_failures += 1;
            return report;
        }
    };
    match entries.next() {
        Some(Ok(_)) => report.retained_unknown_content = true,
        Some(Err(_)) => report.inspection_failures += 1,
        None => {
            if let Err(error) = fs::remove_dir(&directory) {
                if error.kind() == io::ErrorKind::DirectoryNotEmpty {
                    report.retained_unknown_content = true;
                } else if error.kind() != io::ErrorKind::NotFound {
                    report.removal_failures += 1;
                }
            }
        }
    }
    report
}

fn remove_owned_file(path: &Path, report: &mut IndexCleanupReport) {
    match ensure_real_file_or_absent(path) {
        Ok(false) => return,
        Err(RealFileError::Unsafe) => {
            report.retained_unknown_content = true;
            return;
        }
        Err(RealFileError::Io(_)) => {
            report.inspection_failures += 1;
            return;
        }
        Ok(true) => {}
    }
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => report.removal_failures += 1,
    }
}

/// Closed attribution result for one exact agent-provided slot.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the hook attribution commit consumes the lookup result"
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::telemetry) enum InstalledSkillAttribution {
    Public(SafeSkillAttribution),
    Ineligible,
    NotIndexed,
    AttributionUnavailable,
    Ambiguous,
}

/// Look up one exact installed slot across candidate skills parents.
///
/// Missing index metadata and missing markers are definite non-matches.
/// Present metadata that cannot be read or validated is an unavailable source
/// and escalates a single otherwise-valid match, because another match cannot
/// be ruled out. Two proven matches are ambiguous even if another parent is
/// unavailable.
///
/// Every existing candidate parent is planned before metadata absence is
/// decided. Successfully acquired shared locks remain live through result
/// combination, while a failed candidate is remembered as unavailable so it
/// cannot hide a conflict proven by other parents.
#[must_use]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the hook attribution commit consumes the lookup entry point"
    )
)]
pub(in crate::telemetry) fn lookup_installed_skill(
    sym: &crate::config::Symposium,
    skills_parents: impl IntoIterator<Item = PathBuf>,
    slot: &str,
) -> InstalledSkillAttribution {
    if validate_slot(slot).is_err() {
        return InstalledSkillAttribution::NotIndexed;
    }

    let mut unavailable = false;
    let parents = skills_parents.into_iter().collect::<BTreeSet<_>>();
    let guards = locks::acquire_read_guards(sym, parents);

    let mut matches = Vec::new();
    for candidate in guards.candidates() {
        let parent = match candidate {
            locks::ReadParentCandidate::Locked(parent) => parent,
            locks::ReadParentCandidate::Unlocked(parent) => {
                // No compliant writer has published through this parent
                // without first creating its permanent physical lock. Check
                // absence only; present metadata cannot be read unlocked.
                match ensure_real_directory_or_absent(&parent.join(INDEX_DIRECTORY_NAME)) {
                    Ok(false) => {}
                    Ok(true) | Err(_) => unavailable = true,
                }
                continue;
            }
            locks::ReadParentCandidate::Unavailable => {
                unavailable = true;
                continue;
            }
        };
        let entries = match read_index(parent.path()) {
            Ok(Some(DecodedIndex::Complete(entries))) => entries,
            Ok(Some(DecodedIndex::Unavailable)) => {
                unavailable = true;
                continue;
            }
            Ok(None) => continue,
            Err(_) => {
                unavailable = true;
                continue;
            }
        };
        let Some(entry) = entries.into_iter().find(|entry| entry.slot == slot) else {
            continue;
        };
        let skill_dir = parent.path().join(slot);
        match ensure_real_directory_or_absent(&skill_dir) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(_) => {
                unavailable = true;
                continue;
            }
        }
        let marker = skill_dir.join(MARKER_FILE_NAME);
        match read_marker(&marker) {
            Ok(Some(binding)) if binding == entry.binding => match entry.attribution {
                IndexedAttribution::Unavailable => unavailable = true,
                attribution if !matches.contains(&attribution) => matches.push(attribution),
                _ => {}
            },
            Ok(Some(_) | None) => {}
            Err(_) => unavailable = true,
        }
    }

    // A proven conflict remains ambiguous even if another candidate was
    // unreadable. Unavailable metadata cannot erase evidence already seen.
    if matches.len() > 1 {
        return InstalledSkillAttribution::Ambiguous;
    }
    if unavailable {
        return InstalledSkillAttribution::AttributionUnavailable;
    }
    match matches.pop() {
        Some(IndexedAttribution::Public { attribution }) => {
            InstalledSkillAttribution::Public(attribution)
        }
        Some(IndexedAttribution::Ineligible) => InstalledSkillAttribution::Ineligible,
        Some(IndexedAttribution::Unavailable) => {
            unreachable!("unavailable entries are recorded separately")
        }
        None => InstalledSkillAttribution::NotIndexed,
    }
}

/// Content-free failure while loading a present index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndexReadError {
    Io(io::ErrorKind),
    TooLarge { maximum: usize },
    Malformed { line: usize, column: usize },
    UnsupportedVersion(u64),
    TooManyEntries { maximum: usize },
    UnsafeSlot,
    ReservedSlot,
    DuplicateSlot,
    UnavailableWithEntries,
    Unsafe,
}

enum DecodedIndex {
    Complete(Vec<InstallationIndexEntry>),
    Unavailable,
}

fn read_index(skills_parent: &Path) -> Result<Option<DecodedIndex>, IndexReadError> {
    let path = skills_parent
        .join(INDEX_DIRECTORY_NAME)
        .join(INDEX_FILE_NAME);
    let Some(bytes) =
        read_bounded_regular(&path, MAX_INDEX_BYTES).map_err(|error| match error {
            BoundedReadError::Io(kind) => IndexReadError::Io(kind),
            BoundedReadError::TooLarge { maximum } => IndexReadError::TooLarge { maximum },
            BoundedReadError::Unsafe => IndexReadError::Unsafe,
        })?
    else {
        return Ok(None);
    };
    decode_index(&bytes).map(Some)
}

fn decode_index(bytes: &[u8]) -> Result<DecodedIndex, IndexReadError> {
    if bytes.len() > MAX_INDEX_BYTES {
        return Err(IndexReadError::TooLarge {
            maximum: MAX_INDEX_BYTES,
        });
    }
    let envelope: VersionEnvelope = parse_json(bytes).map_err(IndexReadError::from)?;
    if envelope.version != INDEX_VERSION {
        return Err(IndexReadError::UnsupportedVersion(envelope.version));
    }
    let document: IndexDocument = parse_json(bytes).map_err(IndexReadError::from)?;
    let entries = match document.availability {
        IndexAvailability::Complete => document.entries,
        IndexAvailability::Unavailable if document.entries.is_empty() => {
            return Ok(DecodedIndex::Unavailable);
        }
        IndexAvailability::Unavailable => return Err(IndexReadError::UnavailableWithEntries),
    };
    if entries.len() > MAX_INDEX_ENTRIES {
        return Err(IndexReadError::TooManyEntries {
            maximum: MAX_INDEX_ENTRIES,
        });
    }
    let mut slots = BTreeSet::new();
    for entry in &entries {
        match validate_slot(&entry.slot) {
            Ok(()) => {}
            Err(PrepareIndexError::UnsafeSlot) => return Err(IndexReadError::UnsafeSlot),
            Err(PrepareIndexError::ReservedSlot) => return Err(IndexReadError::ReservedSlot),
            Err(
                PrepareIndexError::DuplicateSlot
                | PrepareIndexError::Serialize
                | PrepareIndexError::SizeAccounting,
            ) => unreachable!("slot validation returns only slot errors"),
        }
        if !slots.insert(entry.slot.as_str()) {
            return Err(IndexReadError::DuplicateSlot);
        }
    }
    Ok(DecodedIndex::Complete(entries))
}

impl From<JsonContentError> for IndexReadError {
    fn from(error: JsonContentError) -> Self {
        Self::Malformed {
            line: error.line,
            column: error.column,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct JsonContentError {
    line: usize,
    column: usize,
}

fn parse_json<'de, T>(bytes: &'de [u8]) -> Result<T, JsonContentError>
where
    T: Deserialize<'de>,
{
    serde_json::from_slice(bytes).map_err(|error| JsonContentError {
        line: error.line(),
        column: error.column(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoundedReadError {
    Io(io::ErrorKind),
    TooLarge { maximum: usize },
    Unsafe,
}

fn read_bounded_regular(path: &Path, maximum: usize) -> Result<Option<Vec<u8>>, BoundedReadError> {
    match ensure_real_file_or_absent(path) {
        Ok(true) => {}
        Ok(false) => return Ok(None),
        Err(RealFileError::Io(kind)) => return Err(BoundedReadError::Io(kind)),
        Err(RealFileError::Unsafe) => return Err(BoundedReadError::Unsafe),
    }
    let file = File::open(path).map_err(|error| BoundedReadError::Io(error.kind()))?;
    let limit = u64::try_from(maximum)
        .expect("BUG: usize safety ceiling must fit in u64")
        .checked_add(1)
        .expect("BUG: bounded read ceiling leaves room for one byte");
    let mut bytes = Vec::new();
    file.take(limit)
        .read_to_end(&mut bytes)
        .map_err(|error| BoundedReadError::Io(error.kind()))?;
    if bytes.len() > maximum {
        return Err(BoundedReadError::TooLarge { maximum });
    }
    Ok(Some(bytes))
}

#[derive(Debug, Clone, Copy)]
enum RealFileError {
    Io(io::ErrorKind),
    Unsafe,
}

fn ensure_real_file_or_absent(path: &Path) -> Result<bool, RealFileError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(RealFileError::Io(error.kind())),
    };
    if metadata.file_type().is_file()
        && !metadata.file_type().is_symlink()
        && !has_reparse_point(&metadata)
    {
        Ok(true)
    } else {
        Err(RealFileError::Unsafe)
    }
}

fn ensure_real_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_dir()
        && !metadata.file_type().is_symlink()
        && !has_reparse_point(&metadata)
    {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "path is not a real directory",
        ))
    }
}

fn ensure_real_directory_or_absent(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => ensure_real_directory(path).map(|()| true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
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
    use std::{
        fs::{self, FileTimes, OpenOptions},
        path::{Path, PathBuf},
        time::{Duration, SystemTime},
    };

    use super::*;
    use crate::config::Symposium;

    fn binding(byte: u8) -> InstallationBinding {
        InstallationBinding([byte; BINDING_BYTES])
    }

    fn attribution(name: &str) -> SafeSkillAttribution {
        serde_json::from_str(&format!(
            r#"{{"target":{{"type":"skill","source":"symposium-recommendations","name":"{name}"}},"path":[{{"type":"opaque","reason":"non_package_predicate"}}]}}"#
        ))
        .unwrap()
    }

    fn public_entry(slot: &str, byte: u8) -> InstallationIndexEntry {
        InstallationIndexEntry::public(slot.to_owned(), binding(byte), attribution(slot))
    }

    fn ineligible_entry(slot: &str, byte: u8) -> InstallationIndexEntry {
        InstallationIndexEntry::ineligible(slot.to_owned(), binding(byte))
    }

    fn index_path(parent: &Path) -> PathBuf {
        parent.join(INDEX_DIRECTORY_NAME).join(INDEX_FILE_NAME)
    }

    fn with_locked_parent<R>(
        sym: &Symposium,
        parent: &Path,
        f: impl FnOnce(&LockedParent<'_>) -> R,
    ) -> R {
        let config = acquire_config_guard(sym).unwrap();
        let acquisition = acquire_write_guards(
            sym,
            &config,
            [ParentLockRequest::desired(parent.to_path_buf())],
        )
        .unwrap();
        assert!(acquisition.excluded().is_empty());
        let locked = acquisition.guards().parent(parent).unwrap();
        f(&locked)
    }

    fn publish_entry(
        sym: &Symposium,
        parent: &Path,
        slot: &str,
        attribution: IndexedAttribution,
    ) -> InstallationBinding {
        fs::create_dir_all(parent.join(slot)).unwrap();
        with_locked_parent(sym, parent, |locked| {
            let marker = reconcile_marker(locked, slot, MarkerAction::Rotate).unwrap();
            let entry = InstallationIndexEntry {
                slot: slot.to_owned(),
                binding: marker.binding(),
                attribution,
            };
            let prepared = prepare_index(vec![entry]).unwrap();
            replace_index(locked, &prepared).unwrap();
            marker.binding()
        })
    }

    fn decode_complete(bytes: &[u8]) -> Vec<InstallationIndexEntry> {
        match decode_index(bytes).unwrap() {
            DecodedIndex::Complete(entries) => entries,
            DecodedIndex::Unavailable => panic!("expected a complete index"),
        }
    }

    #[test]
    fn installation_bindings_use_exact_lowercase_hex() {
        let value = InstallationBinding([
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x10, 0x32, 0x54, 0x76, 0x98, 0xba,
            0xdc, 0xfe,
        ]);

        let encoded = serde_json::to_string(&value).unwrap();
        let decoded: InstallationBinding = serde_json::from_str(&encoded).unwrap();

        assert_eq!(encoded, r#""0123456789abcdef1032547698badcfe""#);
        assert!(decoded == value);
        for invalid in [
            r#""0123456789ABCDEF1032547698BADCFE""#,
            r#""0123456789abcdef""#,
            r#""0123456789abcdef1032547698badcg0""#,
        ] {
            assert!(serde_json::from_str::<InstallationBinding>(invalid).is_err());
        }
    }

    #[test]
    fn installation_binding_never_enters_serialized_attribution() {
        let local_binding = binding(0xab);
        let encoded_binding = serde_json::to_string(&local_binding).unwrap();
        let encoded_attribution = serde_json::to_string(&attribution("debugging")).unwrap();

        assert!(!encoded_attribution.contains(encoded_binding.trim_matches('"')));
    }

    #[test]
    fn ensure_preserves_marker_mtime_while_refresh_advances_it() {
        let root = tempfile::tempdir().unwrap();
        let skill_dir = root.path().join("debugging");
        fs::create_dir(&skill_dir).unwrap();
        let value = binding(1);
        write_marker(&skill_dir.join(MARKER_FILE_NAME), value).unwrap();
        let old_time = SystemTime::UNIX_EPOCH + Duration::from_secs(10);
        OpenOptions::new()
            .write(true)
            .open(skill_dir.join(MARKER_FILE_NAME))
            .unwrap()
            .set_times(FileTimes::new().set_modified(old_time))
            .unwrap();

        let ensured = reconcile_marker_at_with(&skill_dir, MarkerAction::Ensure, || {
            panic!("a valid marker must not mint a new binding")
        })
        .unwrap();
        let after_ensure = fs::metadata(skill_dir.join(MARKER_FILE_NAME))
            .unwrap()
            .modified()
            .unwrap();
        let refreshed = reconcile_marker_at_with(&skill_dir, MarkerAction::Refresh, || {
            panic!("a valid marker must not mint a new binding")
        })
        .unwrap();
        let after_refresh = fs::metadata(skill_dir.join(MARKER_FILE_NAME))
            .unwrap()
            .modified()
            .unwrap();

        assert!(ensured.binding() == value);
        assert_eq!(ensured.change(), MarkerChange::Preserved);
        assert_eq!(after_ensure, old_time);
        assert!(refreshed.binding() == value);
        assert_eq!(refreshed.change(), MarkerChange::Refreshed);
        assert!(after_refresh > after_ensure);
    }

    #[test]
    fn malformed_owned_marker_is_repaired_without_rewriting_skill_content() {
        let root = tempfile::tempdir().unwrap();
        let skill_dir = root.path().join("debugging");
        fs::create_dir(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "content").unwrap();
        fs::write(skill_dir.join(MARKER_FILE_NAME), "{broken").unwrap();

        let repaired =
            reconcile_marker_at_with(&skill_dir, MarkerAction::Ensure, || Ok(binding(7))).unwrap();

        assert_eq!(repaired.change(), MarkerChange::Repaired);
        assert!(repaired.binding() == binding(7));
        assert_eq!(
            fs::read_to_string(skill_dir.join("SKILL.md")).unwrap(),
            "content"
        );
        assert!(read_marker(&skill_dir.join(MARKER_FILE_NAME)).unwrap() == Some(binding(7)));
    }

    #[test]
    fn ensure_never_claims_a_slot_without_an_ownership_marker() {
        let root = tempfile::tempdir().unwrap();
        let skill_dir = root.path().join("debugging");
        fs::create_dir(&skill_dir).unwrap();

        assert!(matches!(
            reconcile_marker_at_with(&skill_dir, MarkerAction::Ensure, || Ok(binding(7))),
            Err(MarkerReconcileError::Missing)
        ));
        assert!(!skill_dir.join(MARKER_FILE_NAME).exists());
    }

    #[test]
    fn rotate_mints_a_new_attribution_generation() {
        let root = tempfile::tempdir().unwrap();
        let skill_dir = root.path().join("debugging");
        fs::create_dir(&skill_dir).unwrap();
        write_marker(&skill_dir.join(MARKER_FILE_NAME), binding(1)).unwrap();

        let rotated =
            reconcile_marker_at_with(&skill_dir, MarkerAction::Rotate, || Ok(binding(2))).unwrap();

        assert_eq!(rotated.change(), MarkerChange::Rotated);
        assert!(rotated.binding() == binding(2));
        assert!(read_marker(&skill_dir.join(MARKER_FILE_NAME)).unwrap() == Some(binding(2)));
    }

    #[test]
    fn marker_codec_rejects_unknown_fields_and_versions() {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join(MARKER_FILE_NAME);
        fs::write(
            &marker,
            r#"{"version":1,"binding":"01010101010101010101010101010101","extra":true}"#,
        )
        .unwrap();
        assert!(matches!(
            read_marker(&marker),
            Err(MarkerReadError::Malformed { .. })
        ));

        fs::write(
            &marker,
            r#"{"version":2,"binding":"01010101010101010101010101010101"}"#,
        )
        .unwrap();
        assert!(matches!(
            read_marker(&marker),
            Err(MarkerReadError::UnsupportedVersion(2))
        ));
    }

    #[test]
    fn complete_index_is_sorted_and_round_trips_strictly() {
        let prepared = prepare_index(vec![
            ineligible_entry("z-last", 2),
            public_entry("a-first", 1),
        ])
        .unwrap();
        let decoded = decode_complete(&prepared.encoded);

        assert!(!prepared.unavailable);
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].slot, "a-first");
        assert_eq!(decoded[1].slot, "z-last");
        assert!(matches!(
            decoded[0].attribution,
            IndexedAttribution::Public { .. }
        ));
        assert!(matches!(
            decoded[1].attribution,
            IndexedAttribution::Ineligible
        ));

        let mut document: serde_json::Value = serde_json::from_slice(&prepared.encoded).unwrap();
        document["extra"] = serde_json::Value::Bool(true);
        assert!(decode_index(&serde_json::to_vec(&document).unwrap()).is_err());
    }

    #[test]
    fn unsafe_reserved_and_duplicate_slots_are_rejected() {
        assert!(matches!(
            prepare_index(vec![ineligible_entry("../escape", 1)]),
            Err(PrepareIndexError::UnsafeSlot)
        ));
        assert!(matches!(
            prepare_index(vec![ineligible_entry(INDEX_DIRECTORY_NAME, 1)]),
            Err(PrepareIndexError::ReservedSlot)
        ));
        assert!(matches!(
            prepare_index(vec![ineligible_entry(".SYMPOSIUM-INDEX", 1)]),
            Err(PrepareIndexError::ReservedSlot)
        ));
        assert!(matches!(
            prepare_index(vec![
                ineligible_entry("same", 1),
                ineligible_entry("same", 2),
            ]),
            Err(PrepareIndexError::DuplicateSlot)
        ));
    }

    #[test]
    fn oversized_inventory_is_explicitly_unavailable_not_empty() {
        let entries = (0..=MAX_INDEX_ENTRIES)
            .map(|index| ineligible_entry(&format!("skill-{index:04}"), 1))
            .collect();

        let prepared = prepare_index(entries).unwrap();

        assert!(prepared.unavailable);
        assert_eq!(prepared.retained_entries, 0);
        assert_eq!(prepared.omitted_entries, MAX_INDEX_ENTRIES + 1);
        assert!(matches!(
            decode_index(&prepared.encoded).unwrap(),
            DecodedIndex::Unavailable
        ));
    }

    #[test]
    fn byte_budget_exhaustion_makes_only_omitted_public_entries_unavailable() {
        let attribution = attribution("debugging");
        let entries = (0..3000)
            .map(|index| {
                InstallationIndexEntry::public(
                    format!("skill-{index:04}"),
                    binding(1),
                    attribution.clone(),
                )
            })
            .collect();

        let prepared = prepare_index(entries).unwrap();
        let decoded = decode_complete(&prepared.encoded);
        let public = decoded
            .iter()
            .filter(|entry| matches!(entry.attribution, IndexedAttribution::Public { .. }))
            .count();
        let unavailable = decoded
            .iter()
            .filter(|entry| matches!(entry.attribution, IndexedAttribution::Unavailable))
            .count();

        assert!(!prepared.unavailable);
        assert_eq!(prepared.retained_entries, 3000);
        assert_eq!(prepared.unavailable_public_entries, unavailable);
        assert!(public > 0);
        assert!(unavailable > 0);
        assert_eq!(public + unavailable, decoded.len());
        assert!(prepared.encoded.len() <= MAX_INDEX_BYTES);
    }

    #[test]
    fn entry_unavailable_is_distinct_from_ineligible() {
        let entry = InstallationIndexEntry::unavailable("debugging".into(), binding(1));
        let prepared = prepare_index(vec![entry]).unwrap();
        let decoded = decode_complete(&prepared.encoded);

        assert!(matches!(
            decoded[0].attribution,
            IndexedAttribution::Unavailable
        ));
    }

    #[test]
    fn absent_resolution_evidence_projects_to_ineligible() {
        let entry =
            InstallationIndexEntry::from_skill_evidence("debugging".into(), binding(1), None);

        assert!(matches!(entry.attribution, IndexedAttribution::Ineligible));
    }

    #[test]
    fn incomplete_inventory_has_an_explicit_unavailable_document() {
        let prepared = prepare_unavailable_index().unwrap();

        assert!(prepared.unavailable);
        assert!(matches!(
            decode_index(&prepared.encoded).unwrap(),
            DecodedIndex::Unavailable
        ));
    }

    #[test]
    fn attribution_equality_excludes_slot_and_binding() {
        let first = public_entry("one", 1);
        let same = InstallationIndexEntry::public("two".into(), binding(2), attribution("one"));
        let different = public_entry("other", 3);

        assert!(first.has_same_attribution(&same));
        assert!(!first.has_same_attribution(&different));
    }

    #[test]
    fn guarded_publication_is_atomic_and_avoids_identical_rewrites() {
        let root = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(root.path());
        let parent = root.path().join("project/.agents/skills");
        let prepared = prepare_index(vec![ineligible_entry("debugging", 1)]).unwrap();

        with_locked_parent(&sym, &parent, |locked| {
            let first = replace_index(locked, &prepared).unwrap();
            assert!(first.index_replaced);
            assert!(!first.unavailable);
            let before = fs::metadata(index_path(locked.path()))
                .unwrap()
                .modified()
                .unwrap();

            let second = replace_index(locked, &prepared).unwrap();
            let after = fs::metadata(index_path(locked.path()))
                .unwrap()
                .modified()
                .unwrap();

            assert!(!second.index_replaced);
            assert_eq!(before, after);
            assert_eq!(
                fs::read(locked.path().join(INDEX_DIRECTORY_NAME).join(".gitignore")).unwrap(),
                b"*\n"
            );
        });
    }

    #[test]
    fn cleanup_removes_only_owned_regular_files() {
        let root = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(root.path());
        let parent = root.path().join("project/.agents/skills");
        let directory = parent.join(INDEX_DIRECTORY_NAME);

        with_locked_parent(&sym, &parent, |locked| {
            let prepared = prepare_index(vec![ineligible_entry("debugging", 1)]).unwrap();
            replace_index(locked, &prepared).unwrap();
            fs::write(directory.join("user-note"), "keep").unwrap();

            let report = remove_index(locked);

            assert!(report.retained_unknown_content);
            assert!(!index_path(&parent).exists());
            assert!(!directory.join(".gitignore").exists());
            assert_eq!(
                fs::read_to_string(directory.join("user-note")).unwrap(),
                "keep"
            );
        });
    }

    #[test]
    fn lookup_returns_public_attribution_for_matching_binding() {
        let root = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(root.path());
        let parent = root.path().join("project/.agents/skills");
        publish_entry(
            &sym,
            &parent,
            "debugging",
            IndexedAttribution::Public {
                attribution: attribution("debugging"),
            },
        );

        assert!(matches!(
            lookup_installed_skill(&sym, [parent], "debugging"),
            InstalledSkillAttribution::Public(_)
        ));
    }

    #[test]
    fn missing_index_is_a_definite_non_match_and_creates_no_lock_root() {
        let root = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(root.path());
        let parent = root.path().join("project/.agents/skills");
        fs::create_dir_all(&parent).unwrap();

        assert_eq!(
            lookup_installed_skill(&sym, [parent], "debugging"),
            InstalledSkillAttribution::NotIndexed
        );
        assert!(!root.path().join(".symposium/installation-locks").exists());

        assert_eq!(
            lookup_installed_skill(&sym, [root.path().join("project/.agents/skills")], "../bad"),
            InstalledSkillAttribution::NotIndexed
        );
        assert!(!root.path().join(".symposium/installation-locks").exists());
    }

    #[test]
    fn metadata_absence_is_decided_only_after_the_parent_lock_attempt() {
        let root = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(root.path());
        let parent = root.path().join("project/.agents/skills");
        let config = acquire_config_guard(&sym).unwrap();
        let held =
            acquire_write_guards(&sym, &config, [ParentLockRequest::desired(parent.clone())])
                .unwrap();

        assert_eq!(
            lookup_installed_skill(&sym, [parent], "debugging"),
            InstalledSkillAttribution::AttributionUnavailable
        );
        drop(held);
    }

    #[test]
    fn stale_binding_is_not_indexed() {
        let root = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(root.path());
        let parent = root.path().join("project/.agents/skills");
        publish_entry(&sym, &parent, "debugging", IndexedAttribution::Ineligible);
        write_marker(&parent.join("debugging").join(MARKER_FILE_NAME), binding(9)).unwrap();

        assert_eq!(
            lookup_installed_skill(&sym, [parent], "debugging"),
            InstalledSkillAttribution::NotIndexed
        );
    }

    #[test]
    fn identical_matches_across_parents_are_not_ambiguous() {
        let root = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(root.path());
        let first = root.path().join("project/.agents/skills");
        let second = root.path().join("project/.claude/skills");
        for parent in [&first, &second] {
            publish_entry(
                &sym,
                parent,
                "debugging",
                IndexedAttribution::Public {
                    attribution: attribution("debugging"),
                },
            );
        }

        assert!(matches!(
            lookup_installed_skill(&sym, [first, second], "debugging"),
            InstalledSkillAttribution::Public(_)
        ));
    }

    #[test]
    fn conflicting_valid_matches_are_ambiguous_before_unavailable_metadata() {
        let root = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(root.path());
        let first = root.path().join("project/.agents/skills");
        let second = root.path().join("project/.claude/skills");
        let unavailable = root.path().join("project/.codex/skills");
        publish_entry(
            &sym,
            &first,
            "debugging",
            IndexedAttribution::Public {
                attribution: attribution("debugging"),
            },
        );
        publish_entry(
            &sym,
            &second,
            "debugging",
            IndexedAttribution::Public {
                attribution: attribution("other"),
            },
        );
        publish_entry(
            &sym,
            &unavailable,
            "debugging",
            IndexedAttribution::Unavailable,
        );

        assert_eq!(
            lookup_installed_skill(&sym, [first, second, unavailable], "debugging"),
            InstalledSkillAttribution::Ambiguous
        );
    }

    #[test]
    fn conflicting_readable_parents_win_over_a_third_contended_parent() {
        let root = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(root.path());
        let first = root.path().join("project/.agents/skills");
        let second = root.path().join("project/.claude/skills");
        let contended = root.path().join("project/.codex/skills");
        publish_entry(
            &sym,
            &first,
            "debugging",
            IndexedAttribution::Public {
                attribution: attribution("debugging"),
            },
        );
        publish_entry(
            &sym,
            &second,
            "debugging",
            IndexedAttribution::Public {
                attribution: attribution("other"),
            },
        );
        publish_entry(
            &sym,
            &contended,
            "debugging",
            IndexedAttribution::Ineligible,
        );
        let config = acquire_config_guard(&sym).unwrap();
        let held = acquire_write_guards(
            &sym,
            &config,
            [ParentLockRequest::existing(contended.clone())],
        )
        .unwrap();

        assert_eq!(
            lookup_installed_skill(&sym, [first, second, contended], "debugging"),
            InstalledSkillAttribution::Ambiguous
        );
        drop(held);
    }

    #[test]
    fn unavailable_candidate_escalates_one_otherwise_valid_match() {
        let root = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(root.path());
        let valid = root.path().join("project/.agents/skills");
        let unavailable = root.path().join("project/.claude/skills");
        publish_entry(&sym, &valid, "debugging", IndexedAttribution::Ineligible);
        publish_entry(
            &sym,
            &unavailable,
            "debugging",
            IndexedAttribution::Unavailable,
        );

        assert_eq!(
            lookup_installed_skill(&sym, [valid, unavailable], "debugging"),
            InstalledSkillAttribution::AttributionUnavailable
        );
    }

    #[test]
    fn malformed_present_index_is_unavailable() {
        let root = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(root.path());
        let parent = root.path().join("project/.agents/skills");

        with_locked_parent(&sym, &parent, |locked| {
            let directory = locked.path().join(INDEX_DIRECTORY_NAME);
            fs::create_dir(&directory).unwrap();
            fs::write(directory.join(INDEX_FILE_NAME), "{broken").unwrap();
        });

        assert_eq!(
            lookup_installed_skill(&sym, [parent], "debugging"),
            InstalledSkillAttribution::AttributionUnavailable
        );
    }

    #[cfg(unix)]
    #[test]
    fn marker_and_index_symlinks_fail_closed() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(root.path());
        let parent = root.path().join("project/.agents/skills");
        let external = root.path().join("external");
        fs::create_dir_all(parent.join("debugging")).unwrap();
        fs::write(&external, "{}").unwrap();

        with_locked_parent(&sym, &parent, |locked| {
            let directory = locked.path().join(INDEX_DIRECTORY_NAME);
            fs::create_dir(&directory).unwrap();
            symlink(&external, directory.join(INDEX_FILE_NAME)).unwrap();
        });
        assert_eq!(
            lookup_installed_skill(&sym, [parent.clone()], "debugging"),
            InstalledSkillAttribution::AttributionUnavailable
        );

        fs::remove_file(index_path(&parent)).unwrap();
        with_locked_parent(&sym, &parent, |locked| {
            let prepared = prepare_index(vec![ineligible_entry("debugging", 1)]).unwrap();
            replace_index(locked, &prepared).unwrap();
            symlink(
                &external,
                locked.path().join("debugging").join(MARKER_FILE_NAME),
            )
            .unwrap();
        });
        assert_eq!(
            lookup_installed_skill(&sym, [parent], "debugging"),
            InstalledSkillAttribution::AttributionUnavailable
        );
    }
}
