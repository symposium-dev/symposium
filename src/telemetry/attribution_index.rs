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
    Created,
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
    Read(io::ErrorKind),
    Generate,
    Encode,
    Write(io::ErrorKind),
}

impl fmt::Display for MarkerReconcileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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
    skill_dir: &Path,
    action: MarkerAction,
) -> Result<MarkerReconciliation, MarkerReconcileError> {
    reconcile_marker_with(skill_dir, action, || {
        InstallationBinding::generate().map_err(|_| MarkerReconcileError::Generate)
    })
}

fn reconcile_marker_with(
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
        Ok(None) => (generate()?, Some(MarkerChange::Created)),
        Err(MarkerReadError::Io(kind)) => return Err(MarkerReconcileError::Read(kind)),
        Err(
            MarkerReadError::TooLarge { .. }
            | MarkerReadError::Malformed { .. }
            | MarkerReadError::UnsupportedVersion(_),
        ) => (generate()?, Some(MarkerChange::Repaired)),
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
    fs::write(marker_path, encoded).map_err(|error| MarkerReconcileError::Write(error.kind()))
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
    TooLarge { maximum: usize },
    Malformed { line: usize, column: usize },
    UnsupportedVersion(u64),
}

fn read_marker(path: &Path) -> Result<Option<InstallationBinding>, MarkerReadError> {
    let Some(bytes) = read_bounded(path, MAX_MARKER_BYTES).map_err(|error| match error {
        BoundedReadError::Io(kind) => MarkerReadError::Io(kind),
        BoundedReadError::TooLarge { maximum } => MarkerReadError::TooLarge { maximum },
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

    fn as_ineligible(&self) -> Self {
        Self::ineligible(self.slot.clone(), self.binding)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexDocument {
    version: u64,
    entries: Vec<InstallationIndexEntry>,
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
    retained_entries: usize,
    demoted_public_entries: usize,
    omitted_entries: usize,
}

struct MeasuredEntry {
    entry: InstallationIndexEntry,
    ineligible_len: usize,
    public_len: Option<usize>,
}

/// Validate, order, and encode one complete parent index.
///
/// Each entry shape is measured once. Public attribution is then admitted in
/// canonical slot order against a running total, and the selected document is
/// encoded once. If even the all-ineligible form cannot fit, a valid empty
/// index replaces stale attribution.
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
        return Ok(PreparedInstallationIndex {
            encoded: empty.into_boxed_slice(),
            retained_entries: 0,
            demoted_public_entries: 0,
            omitted_entries: input_len,
        });
    }

    let mut measured = Vec::with_capacity(by_slot.len());
    let mut baseline_len = empty.len();
    for entry in by_slot.into_values() {
        let ineligible_len = serde_json::to_vec(&entry.as_ineligible())
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
            .checked_add(ineligible_len)
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
            ineligible_len,
            public_len,
        });
    }

    if baseline_len > MAX_INDEX_BYTES {
        return Ok(PreparedInstallationIndex {
            encoded: empty.into_boxed_slice(),
            retained_entries: 0,
            demoted_public_entries: 0,
            omitted_entries: input_len,
        });
    }

    let mut selected_len = baseline_len;
    let mut demoted_public_entries = 0;
    let mut selected = Vec::with_capacity(measured.len());
    for measured in measured {
        let MeasuredEntry {
            mut entry,
            ineligible_len,
            public_len,
        } = measured;
        if let Some(public_len) = public_len {
            let candidate_len = selected_len
                .checked_sub(ineligible_len)
                .and_then(|length| length.checked_add(public_len))
                .ok_or(PrepareIndexError::SizeAccounting)?;
            if candidate_len <= MAX_INDEX_BYTES {
                selected_len = candidate_len;
            } else {
                entry.attribution = IndexedAttribution::Ineligible;
                demoted_public_entries += 1;
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
        demoted_public_entries,
        omitted_entries: 0,
    })
}

fn encode_index(entries: Vec<InstallationIndexEntry>) -> Result<Vec<u8>, PrepareIndexError> {
    serde_json::to_vec(&IndexDocument {
        version: INDEX_VERSION,
        entries,
    })
    .map_err(|_| PrepareIndexError::Serialize)
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
    pub(crate) retained_entries: usize,
    pub(crate) demoted_public_entries: usize,
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
    skills_parent: &Path,
    prepared: &PreparedInstallationIndex,
) -> Result<IndexWriteReport, IndexWriteError> {
    let directory = skills_parent.join(INDEX_DIRECTORY_NAME);
    fs::create_dir_all(&directory)
        .map_err(|error| IndexWriteError::CreateDirectory(error.kind()))?;
    let ignore_file = directory.join(INDEX_IGNORE_FILE_NAME);
    if !file_has_contents(&ignore_file, INDEX_IGNORE_CONTENTS) {
        fs::write(ignore_file, INDEX_IGNORE_CONTENTS)
            .map_err(|error| IndexWriteError::WriteIgnore(error.kind()))?;
    }

    let cleanup = cleanup_temporary_files(&directory);
    let index_file = directory.join(INDEX_FILE_NAME);
    let index_replaced = !file_has_contents(&index_file, &prepared.encoded);
    if index_replaced {
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
        retained_entries: prepared.retained_entries,
        demoted_public_entries: prepared.demoted_public_entries,
        omitted_entries: prepared.omitted_entries,
        temporary_inspection_failures: cleanup.inspection_failures,
        temporary_removal_failures: cleanup.removal_failures,
    })
}

fn file_has_contents(path: &Path, expected: &[u8]) -> bool {
    matches!(
        read_bounded(path, expected.len()),
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
pub(crate) fn remove_index(skills_parent: &Path) -> IndexCleanupReport {
    let directory = skills_parent.join(INDEX_DIRECTORY_NAME);
    let mut report = IndexCleanupReport::default();
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
/// Candidate parents are deduplicated by exact path spelling. Current callers
/// derive every parent from one project root; any future caller that accepts
/// aliases must prevent equivalent paths from becoming distinct candidates.
#[must_use]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the hook attribution commit consumes the lookup entry point"
    )
)]
pub(in crate::telemetry) fn lookup_installed_skill(
    skills_parents: impl IntoIterator<Item = PathBuf>,
    slot: &str,
) -> InstalledSkillAttribution {
    if validate_slot(slot).is_err() {
        return InstalledSkillAttribution::NotIndexed;
    }

    let mut matches = Vec::new();
    let mut unavailable = false;
    let parents = skills_parents.into_iter().collect::<BTreeSet<_>>();
    for parent in parents {
        let entries = match read_index(&parent) {
            Ok(Some(entries)) => entries,
            Ok(None) => continue,
            Err(_) => {
                unavailable = true;
                continue;
            }
        };
        let Some(entry) = entries.into_iter().find(|entry| entry.slot == slot) else {
            continue;
        };
        let marker = parent.join(slot).join(MARKER_FILE_NAME);
        match read_marker(&marker) {
            Ok(Some(binding)) if binding == entry.binding => matches.push(entry.attribution),
            Ok(Some(_) | None) => {}
            Err(_) => unavailable = true,
        }
    }

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
}

fn read_index(skills_parent: &Path) -> Result<Option<Vec<InstallationIndexEntry>>, IndexReadError> {
    let path = skills_parent
        .join(INDEX_DIRECTORY_NAME)
        .join(INDEX_FILE_NAME);
    let Some(bytes) = read_bounded(&path, MAX_INDEX_BYTES).map_err(|error| match error {
        BoundedReadError::Io(kind) => IndexReadError::Io(kind),
        BoundedReadError::TooLarge { maximum } => IndexReadError::TooLarge { maximum },
    })?
    else {
        return Ok(None);
    };
    decode_index(&bytes).map(Some)
}

fn decode_index(bytes: &[u8]) -> Result<Vec<InstallationIndexEntry>, IndexReadError> {
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
    if document.entries.len() > MAX_INDEX_ENTRIES {
        return Err(IndexReadError::TooManyEntries {
            maximum: MAX_INDEX_ENTRIES,
        });
    }
    let mut slots = BTreeSet::new();
    for entry in &document.entries {
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
    Ok(document.entries)
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
}

fn read_bounded(path: &Path, maximum: usize) -> Result<Option<Vec<u8>>, BoundedReadError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(BoundedReadError::Io(error.kind())),
    };
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

#[cfg(test)]
mod tests {
    use std::{
        fs::{self, FileTimes, OpenOptions},
        time::{Duration, SystemTime},
    };

    use super::*;

    fn binding(byte: u8) -> InstallationBinding {
        InstallationBinding([byte; BINDING_BYTES])
    }

    fn attribution(name: &str) -> SafeSkillAttribution {
        serde_json::from_str(&format!(
            r#"{{"target":{{"type":"skill","source":"symposium-recommendations","name":"{name}"}},"path":[{{"type":"all","children":[{{"type":"any","child":{{"type":"opaque","reason":"non_package_predicate"}}}},{{"type":"not"}}]}}]}}"#
        ))
        .unwrap()
    }

    fn public_entry(slot: &str, byte: u8) -> InstallationIndexEntry {
        InstallationIndexEntry::public(slot.to_owned(), binding(byte), attribution(slot))
    }

    fn ineligible_entry(slot: &str, byte: u8) -> InstallationIndexEntry {
        InstallationIndexEntry::ineligible(slot.to_owned(), binding(byte))
    }

    fn reconcile_with_binding(
        skill_dir: &Path,
        action: MarkerAction,
        value: InstallationBinding,
    ) -> Result<MarkerReconciliation, MarkerReconcileError> {
        reconcile_marker_with(skill_dir, action, || Ok(value))
    }

    fn write_marker_binding(skill_dir: &Path, value: InstallationBinding) {
        fs::create_dir_all(skill_dir).unwrap();
        write_marker(&skill_dir.join(MARKER_FILE_NAME), value).unwrap();
    }

    fn replace_entries(skills_parent: &Path, entries: Vec<InstallationIndexEntry>) {
        fs::create_dir_all(skills_parent).unwrap();
        let prepared = prepare_index(entries).unwrap();
        replace_index(skills_parent, &prepared).unwrap();
    }

    fn index_path(skills_parent: &Path) -> PathBuf {
        skills_parent
            .join(INDEX_DIRECTORY_NAME)
            .join(INDEX_FILE_NAME)
    }

    fn assert_index_error(bytes: &[u8], expected: IndexReadError) {
        match decode_index(bytes) {
            Ok(_) => panic!("expected the installation index to be rejected"),
            Err(error) => assert_eq!(error, expected),
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
    fn ensure_preserves_marker_mtime_while_refresh_advances_it() {
        let root = tempfile::tempdir().unwrap();
        let skill_dir = root.path().join("debugging");
        let value = binding(1);
        write_marker_binding(&skill_dir, value);
        let old_time = SystemTime::UNIX_EPOCH + Duration::from_secs(10);
        OpenOptions::new()
            .write(true)
            .open(skill_dir.join(MARKER_FILE_NAME))
            .unwrap()
            .set_times(FileTimes::new().set_modified(old_time))
            .unwrap();

        let ensured = reconcile_marker_with(&skill_dir, MarkerAction::Ensure, || {
            panic!("a valid marker must not mint a new binding")
        })
        .unwrap();
        let after_ensure = fs::metadata(skill_dir.join(MARKER_FILE_NAME))
            .unwrap()
            .modified()
            .unwrap();
        let refreshed = reconcile_marker_with(&skill_dir, MarkerAction::Refresh, || {
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
    fn marker_actions_preserve_or_rotate_the_binding_as_declared() {
        let root = tempfile::tempdir().unwrap();
        let skill_dir = root.path().join("debugging");
        fs::create_dir(&skill_dir).unwrap();

        let created = reconcile_with_binding(&skill_dir, MarkerAction::Ensure, binding(1)).unwrap();
        let refreshed =
            reconcile_with_binding(&skill_dir, MarkerAction::Refresh, binding(2)).unwrap();
        let rotated = reconcile_with_binding(&skill_dir, MarkerAction::Rotate, binding(3)).unwrap();

        assert!(created.binding() == binding(1));
        assert_eq!(created.change(), MarkerChange::Created);
        assert!(refreshed.binding() == binding(1));
        assert_eq!(refreshed.change(), MarkerChange::Refreshed);
        assert!(rotated.binding() == binding(3));
        assert_eq!(rotated.change(), MarkerChange::Rotated);
        assert!(read_marker(&skill_dir.join(MARKER_FILE_NAME)).unwrap() == Some(binding(3)));
    }

    #[test]
    fn production_marker_entry_point_mints_a_random_binding() {
        let root = tempfile::tempdir().unwrap();
        let skill_dir = root.path().join("debugging");
        fs::create_dir(&skill_dir).unwrap();

        let result = reconcile_marker(&skill_dir, MarkerAction::Ensure).unwrap();

        assert_eq!(result.change(), MarkerChange::Created);
        assert!(read_marker(&skill_dir.join(MARKER_FILE_NAME)).unwrap() == Some(result.binding()));
    }

    #[test]
    fn non_skill_evidence_degrades_to_an_ineligible_index_entry() {
        use crate::{
            pm::{CARGO_PM, CRATES_IO_REGISTRY_URL, PackageId, PackageProvenance, SourcedPackage},
            predicate::{PredicateSet, SourcedPredicateContext},
        };

        let package = SourcedPackage::new(
            PackageId::new(CARGO_PM, "example", "1.2.3"),
            PackageProvenance::Registry {
                url: CRATES_IO_REGISTRY_URL.to_owned(),
            },
        );
        let mut context = SourcedPredicateContext::new(&[]);
        let route = crate::telemetry::evaluate_resolution_predicates(
            &PredicateSet::default(),
            &mut context,
            &mut crate::telemetry::OptionalWitnessMode::new(true),
        )
        .unwrap();
        let evidence =
            crate::telemetry::plugin_resolution_evidence(route, None, None, &package, "example")
                .unwrap();

        let entry = InstallationIndexEntry::from_skill_evidence(
            "example".to_owned(),
            binding(1),
            Some(&evidence),
        );

        assert!(matches!(entry.attribution, IndexedAttribution::Ineligible));
    }

    #[test]
    fn sync_marker_and_index_reader_bind_the_same_installed_content() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let parent = root.path().join("project/.agents/skills");
        let destination = parent.join("debugging");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "first").unwrap();

        crate::sync::sync_skill_dir_for_attribution_test(
            &source,
            &destination,
            root.path(),
            Duration::ZERO,
        )
        .unwrap();
        let first = read_marker(&destination.join(MARKER_FILE_NAME))
            .unwrap()
            .unwrap();
        replace_entries(
            &parent,
            vec![InstallationIndexEntry::public(
                "debugging".to_owned(),
                first,
                attribution("debugging"),
            )],
        );
        assert!(matches!(
            lookup_installed_skill([parent.clone()], "debugging"),
            InstalledSkillAttribution::Public(_)
        ));

        fs::write(source.join("SKILL.md"), "second").unwrap();
        crate::sync::sync_skill_dir_for_attribution_test(
            &source,
            &destination,
            root.path(),
            Duration::ZERO,
        )
        .unwrap();
        let second = read_marker(&destination.join(MARKER_FILE_NAME))
            .unwrap()
            .unwrap();

        assert!(first != second);
        assert_eq!(
            lookup_installed_skill([parent.clone()], "debugging"),
            InstalledSkillAttribution::NotIndexed,
            "rotating the sync-owned marker must invalidate the old index"
        );

        replace_entries(
            &parent,
            vec![InstallationIndexEntry::public(
                "debugging".to_owned(),
                second,
                attribution("debugging"),
            )],
        );
        assert!(matches!(
            lookup_installed_skill([parent], "debugging"),
            InstalledSkillAttribution::Public(_)
        ));
    }

    #[test]
    fn malformed_legacy_and_unknown_markers_are_repaired() {
        let cases: [&[u8]; 3] = [
            b"",
            br#"{"version":1}"#,
            br#"{"version":2,"binding":"00000000000000000000000000000000"}"#,
        ];

        for (index, contents) in cases.into_iter().enumerate() {
            let root = tempfile::tempdir().unwrap();
            let skill_dir = root.path().join("debugging");
            fs::create_dir(&skill_dir).unwrap();
            fs::write(skill_dir.join(MARKER_FILE_NAME), contents).unwrap();
            let expected = binding(u8::try_from(index + 1).unwrap());

            let result =
                reconcile_with_binding(&skill_dir, MarkerAction::Ensure, expected).unwrap();

            assert!(result.binding() == expected);
            assert_eq!(result.change(), MarkerChange::Repaired);
            assert!(
                fs::metadata(skill_dir.join(MARKER_FILE_NAME))
                    .unwrap()
                    .len()
                    > 0
            );
            assert!(read_marker(&skill_dir.join(MARKER_FILE_NAME)).unwrap() == Some(expected));
        }
    }

    #[test]
    fn oversized_marker_is_unavailable_to_lookup_and_repaired_by_sync() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("skills");
        let skill_dir = parent.join("debugging");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join(MARKER_FILE_NAME),
            vec![b'x'; MAX_MARKER_BYTES + 1],
        )
        .unwrap();
        replace_entries(&parent, vec![public_entry("debugging", 1)]);

        assert_eq!(
            lookup_installed_skill(vec![parent], "debugging"),
            InstalledSkillAttribution::AttributionUnavailable
        );

        let repaired =
            reconcile_with_binding(&skill_dir, MarkerAction::Ensure, binding(1)).unwrap();
        assert_eq!(repaired.change(), MarkerChange::Repaired);
        assert!(repaired.binding() == binding(1));
    }

    #[test]
    fn marker_parse_failures_retain_no_input_content() {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join(MARKER_FILE_NAME);
        let recognizable = "recognizable-private-binding-text";
        fs::write(
            &marker,
            format!(r#"{{"version":1,"binding":"{recognizable}"}}"#),
        )
        .unwrap();

        let error = match read_marker(&marker) {
            Ok(_) => panic!("expected the marker to be rejected"),
            Err(error) => error,
        };
        let diagnostic = format!("{error:?}");

        assert!(!diagnostic.contains(recognizable));
        assert!(matches!(error, MarkerReadError::Malformed { .. }));
    }

    #[test]
    fn binding_never_enters_safe_attribution_serialization() {
        let value = binding(0xab);
        let encoded_binding = serde_json::to_string(&value).unwrap();
        let encoded_attribution = serde_json::to_string(&attribution("debugging")).unwrap();

        assert!(!encoded_attribution.contains(encoded_binding.trim_matches('"')));
    }

    #[test]
    fn public_and_ineligible_entries_round_trip_in_canonical_order() {
        let prepared = prepare_index(vec![
            ineligible_entry("z-last", 2),
            public_entry("a-first", 1),
        ])
        .unwrap();

        let decoded = decode_index(&prepared.encoded).unwrap();

        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].slot, "a-first");
        assert!(decoded[0].binding == binding(1));
        assert!(matches!(
            decoded[0].attribution,
            IndexedAttribution::Public { .. }
        ));
        assert_eq!(decoded[1].slot, "z-last");
        assert!(matches!(
            decoded[1].attribution,
            IndexedAttribution::Ineligible
        ));
        assert_eq!(prepared.retained_entries, 2);
        assert_eq!(prepared.demoted_public_entries, 0);
        assert_eq!(prepared.omitted_entries, 0);
    }

    #[test]
    fn nested_attribution_revalidates_and_reencodes_identically() {
        let prepared = prepare_index(vec![public_entry("debugging", 1)]).unwrap();
        let decoded = decode_index(&prepared.encoded).unwrap();
        let reencoded = prepare_index(decoded).unwrap();

        assert_eq!(prepared.encoded, reencoded.encoded);

        let too_deep = format!(
            r#"{{"version":1,"entries":[{{"slot":"debugging","binding":"{}","attribution":{{"eligibility":"public","attribution":{{"target":{{"type":"skill","source":"symposium-recommendations","name":"debugging"}},"path":[{}]}}}}}}]}}"#,
            "00".repeat(BINDING_BYTES),
            nested_any_node(9)
        );
        assert!(matches!(
            decode_index(too_deep.as_bytes()),
            Err(IndexReadError::Malformed { .. })
        ));
    }

    fn nested_any_node(depth: usize) -> String {
        let mut node = r#"{"type":"not"}"#.to_owned();
        for _ in 0..depth {
            node = format!(r#"{{"type":"any","child":{node}}}"#);
        }
        node
    }

    #[test]
    fn index_decoder_rejects_version_slots_duplicates_and_limits() {
        let binding = "00".repeat(BINDING_BYTES);
        let cases = [
            (
                r#"{"version":2,"entries":[]}"#.to_owned(),
                IndexReadError::UnsupportedVersion(2),
            ),
            (
                format!(
                    r#"{{"version":1,"entries":[{{"slot":"../escape","binding":"{binding}","attribution":{{"eligibility":"ineligible"}}}}]}}"#
                ),
                IndexReadError::UnsafeSlot,
            ),
            (
                format!(
                    r#"{{"version":1,"entries":[{{"slot":"{INDEX_DIRECTORY_NAME}","binding":"{binding}","attribution":{{"eligibility":"ineligible"}}}}]}}"#
                ),
                IndexReadError::ReservedSlot,
            ),
            (
                format!(
                    r#"{{"version":1,"entries":[{{"slot":"same","binding":"{binding}","attribution":{{"eligibility":"ineligible"}}}},{{"slot":"same","binding":"{binding}","attribution":{{"eligibility":"ineligible"}}}}]}}"#
                ),
                IndexReadError::DuplicateSlot,
            ),
        ];
        for (json, expected) in cases {
            assert_index_error(json.as_bytes(), expected);
        }
        assert!(matches!(
            decode_index(br#"{"version":1,"entries":[],"private_field":"must fail"}"#),
            Err(IndexReadError::Malformed { .. })
        ));
        assert_index_error(
            &vec![b' '; MAX_INDEX_BYTES + 1],
            IndexReadError::TooLarge {
                maximum: MAX_INDEX_BYTES,
            },
        );

        let entries = (0..=MAX_INDEX_ENTRIES)
            .map(|index| ineligible_entry(&format!("skill-{index}"), 1))
            .collect();
        let oversized_count = serde_json::to_vec(&IndexDocument {
            version: INDEX_VERSION,
            entries,
        })
        .unwrap();
        assert!(oversized_count.len() <= MAX_INDEX_BYTES);
        assert_index_error(
            &oversized_count,
            IndexReadError::TooManyEntries {
                maximum: MAX_INDEX_ENTRIES,
            },
        );
    }

    #[test]
    fn public_entries_are_demoted_by_canonical_entry_deltas() {
        let entries = (0..MAX_INDEX_ENTRIES)
            .map(|index| public_entry(&format!("skill-{index:04}"), 1))
            .collect();

        let prepared = prepare_index(entries).unwrap();
        let decoded = decode_index(&prepared.encoded).unwrap();
        let public_count = decoded
            .iter()
            .filter(|entry| matches!(entry.attribution, IndexedAttribution::Public { .. }))
            .count();

        assert_eq!(decoded.len(), MAX_INDEX_ENTRIES);
        assert!(prepared.encoded.len() <= MAX_INDEX_BYTES);
        assert!(public_count > 0);
        assert!(prepared.demoted_public_entries > 0);
        assert_eq!(
            public_count + prepared.demoted_public_entries,
            MAX_INDEX_ENTRIES
        );
    }

    #[test]
    fn irreducible_entry_count_or_byte_size_installs_an_empty_index() {
        let too_many = (0..=MAX_INDEX_ENTRIES)
            .map(|index| ineligible_entry(&format!("skill-{index}"), 1))
            .collect::<Vec<_>>();
        let too_wide = (0..3000)
            .map(|index| ineligible_entry(&format!("skill-{index:04}-{}", "x".repeat(180)), 1))
            .collect::<Vec<_>>();

        for entries in [too_many, too_wide] {
            let input_len = entries.len();
            let prepared = prepare_index(entries).unwrap();
            let decoded = decode_index(&prepared.encoded).unwrap();

            assert!(decoded.is_empty());
            assert_eq!(prepared.retained_entries, 0);
            assert_eq!(prepared.omitted_entries, input_len);
        }
    }

    #[test]
    fn index_encoding_is_deterministic_across_input_order() {
        let forward = prepare_index(vec![public_entry("a", 1), ineligible_entry("b", 2)]).unwrap();
        let reverse = prepare_index(vec![ineligible_entry("b", 2), public_entry("a", 1)]).unwrap();

        assert_eq!(forward.encoded, reverse.encoded);
    }

    #[test]
    fn replacement_removes_stale_temporaries_and_writes_complete_bytes() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("skills");
        let directory = parent.join(INDEX_DIRECTORY_NAME);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join(format!("{INDEX_TEMPORARY_PREFIX}stale")),
            b"old",
        )
        .unwrap();
        let prepared = prepare_index(vec![public_entry("debugging", 1)]).unwrap();

        let report = replace_index(&parent, &prepared).unwrap();

        assert_eq!(fs::read(index_path(&parent)).unwrap(), &*prepared.encoded);
        assert_eq!(
            fs::read(directory.join(INDEX_IGNORE_FILE_NAME)).unwrap(),
            b"*\n"
        );
        assert!(
            !directory
                .join(format!("{INDEX_TEMPORARY_PREFIX}stale"))
                .exists()
        );
        assert!(report.index_replaced);
        assert_eq!(report.retained_entries, 1);
        assert_eq!(report.demoted_public_entries, 0);
        assert_eq!(report.omitted_entries, 0);
        assert_eq!(report.temporary_inspection_failures, 0);
        assert_eq!(report.temporary_removal_failures, 0);
    }

    #[test]
    fn identical_index_and_ignore_file_are_not_rewritten() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("skills");
        let prepared = prepare_index(vec![public_entry("debugging", 1)]).unwrap();
        let initial = replace_index(&parent, &prepared).unwrap();
        assert!(initial.index_replaced);

        let directory = parent.join(INDEX_DIRECTORY_NAME);
        let index = index_path(&parent);
        let ignore = directory.join(INDEX_IGNORE_FILE_NAME);
        let old_time = SystemTime::now()
            .checked_sub(Duration::from_secs(60))
            .unwrap();
        for path in [&index, &ignore] {
            OpenOptions::new()
                .write(true)
                .open(path)
                .unwrap()
                .set_times(FileTimes::new().set_modified(old_time))
                .unwrap();
        }

        let unchanged = replace_index(&parent, &prepared).unwrap();

        assert!(!unchanged.index_replaced);
        assert_eq!(fs::metadata(&index).unwrap().modified().unwrap(), old_time);
        assert_eq!(fs::metadata(&ignore).unwrap().modified().unwrap(), old_time);

        fs::write(&ignore, b"tracked\n").unwrap();
        let repaired_ignore = replace_index(&parent, &prepared).unwrap();

        assert!(!repaired_ignore.index_replaced);
        assert_eq!(fs::read(&ignore).unwrap(), INDEX_IGNORE_CONTENTS);
        assert_eq!(fs::metadata(&index).unwrap().modified().unwrap(), old_time);
    }

    #[test]
    fn failed_atomic_persistence_keeps_the_existing_destination() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("skills");
        let target = index_path(&parent);
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("sentinel"), b"old").unwrap();
        let prepared = prepare_index(vec![public_entry("debugging", 1)]).unwrap();

        let error = replace_index(&parent, &prepared).unwrap_err();

        assert!(matches!(error, IndexWriteError::Persist(_)));
        assert_eq!(fs::read(target.join("sentinel")).unwrap(), b"old");
        assert!(
            fs::read_dir(parent.join(INDEX_DIRECTORY_NAME))
                .unwrap()
                .all(|entry| !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(INDEX_TEMPORARY_PREFIX))
        );
    }

    #[test]
    fn cleanup_removes_only_owned_regular_files_without_recursing() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("skills");
        let directory = parent.join(INDEX_DIRECTORY_NAME);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join(INDEX_FILE_NAME), b"index").unwrap();
        fs::write(directory.join(INDEX_IGNORE_FILE_NAME), b"*\n").unwrap();
        fs::write(
            directory.join(format!("{INDEX_TEMPORARY_PREFIX}stale")),
            b"temp",
        )
        .unwrap();
        fs::create_dir(directory.join(format!("{INDEX_TEMPORARY_PREFIX}directory"))).unwrap();
        fs::write(directory.join("user-note"), b"keep").unwrap();

        let report = remove_index(&parent);

        assert!(!directory.join(INDEX_FILE_NAME).exists());
        assert!(!directory.join(INDEX_IGNORE_FILE_NAME).exists());
        assert!(
            !directory
                .join(format!("{INDEX_TEMPORARY_PREFIX}stale"))
                .exists()
        );
        assert!(
            directory
                .join(format!("{INDEX_TEMPORARY_PREFIX}directory"))
                .is_dir()
        );
        assert!(directory.join("user-note").is_file());
        assert!(directory.is_dir());
        assert_eq!(report.inspection_failures, 0);
        assert_eq!(report.removal_failures, 0);
        assert!(report.retained_unknown_content);
        assert_eq!(
            remove_index(&root.path().join("missing")),
            IndexCleanupReport::default()
        );
    }

    #[test]
    fn complete_replacement_drops_slots_omitted_from_the_next_index() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("skills");
        replace_entries(
            &parent,
            vec![public_entry("kept", 1), public_entry("reaped", 2)],
        );

        replace_entries(&parent, vec![public_entry("kept", 1)]);
        let entries = read_index(&parent).unwrap().unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].slot, "kept");
    }

    #[test]
    fn lookup_returns_exact_public_and_ineligible_attribution() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("skills");
        write_marker_binding(&parent.join("public"), binding(1));
        write_marker_binding(&parent.join("private"), binding(2));
        replace_entries(
            &parent,
            vec![public_entry("public", 1), ineligible_entry("private", 2)],
        );

        let public = lookup_installed_skill(vec![parent.clone()], "public");
        let ineligible = lookup_installed_skill(vec![parent], "private");

        assert!(matches!(public, InstalledSkillAttribution::Public(_)));
        assert_eq!(ineligible, InstalledSkillAttribution::Ineligible);
    }

    #[test]
    fn absent_metadata_missing_markers_and_stale_bindings_are_not_indexed() {
        let root = tempfile::tempdir().unwrap();
        let absent = root.path().join("absent");
        let missing_marker = root.path().join("missing-marker");
        let stale = root.path().join("stale");
        replace_entries(&missing_marker, vec![public_entry("debugging", 1)]);
        write_marker_binding(&stale.join("debugging"), binding(2));
        replace_entries(&stale, vec![public_entry("debugging", 1)]);

        assert_eq!(
            lookup_installed_skill(vec![absent], "debugging"),
            InstalledSkillAttribution::NotIndexed
        );
        assert_eq!(
            lookup_installed_skill(vec![missing_marker], "debugging"),
            InstalledSkillAttribution::NotIndexed
        );
        assert_eq!(
            lookup_installed_skill(vec![stale], "debugging"),
            InstalledSkillAttribution::NotIndexed
        );
    }

    #[test]
    fn unsafe_and_reserved_agent_slots_are_refused_before_joining() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("skills");
        fs::create_dir_all(&parent).unwrap();

        for slot in [
            "",
            "..",
            "../escape",
            r"..\escape",
            INDEX_DIRECTORY_NAME,
            ".SYMPOSIUM-INDEX",
        ] {
            assert_eq!(
                lookup_installed_skill(vec![parent.clone()], slot),
                InstalledSkillAttribution::NotIndexed
            );
        }
    }

    #[test]
    fn corrupt_present_metadata_is_attribution_unavailable() {
        let root = tempfile::tempdir().unwrap();
        let malformed_index = root.path().join("malformed-index");
        let malformed_marker = root.path().join("malformed-marker");
        fs::create_dir_all(malformed_index.join(INDEX_DIRECTORY_NAME)).unwrap();
        fs::write(index_path(&malformed_index), b"private malformed index").unwrap();
        replace_entries(&malformed_marker, vec![public_entry("debugging", 1)]);
        fs::create_dir_all(malformed_marker.join("debugging")).unwrap();
        fs::write(
            malformed_marker.join("debugging").join(MARKER_FILE_NAME),
            b"private malformed marker",
        )
        .unwrap();

        assert_eq!(
            lookup_installed_skill(vec![malformed_index], "debugging"),
            InstalledSkillAttribution::AttributionUnavailable
        );
        assert_eq!(
            lookup_installed_skill(vec![malformed_marker], "debugging"),
            InstalledSkillAttribution::AttributionUnavailable
        );
    }

    #[test]
    fn duplicate_slots_in_a_present_index_are_attribution_unavailable() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("skills");
        let binding = "00".repeat(BINDING_BYTES);
        fs::create_dir_all(parent.join(INDEX_DIRECTORY_NAME)).unwrap();
        fs::write(
            index_path(&parent),
            format!(r#"{{"version":1,"entries":[{{"slot":"debugging","binding":"{binding}","attribution":{{"eligibility":"ineligible"}}}},{{"slot":"debugging","binding":"{binding}","attribution":{{"eligibility":"ineligible"}}}}]}}"#),
        )
        .unwrap();

        assert_eq!(
            lookup_installed_skill(vec![parent], "debugging"),
            InstalledSkillAttribution::AttributionUnavailable
        );
    }

    #[test]
    fn lookup_escalates_corruption_and_multiple_proven_matches_correctly() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first");
        let second = root.path().join("second");
        let corrupt = root.path().join("corrupt");
        for (parent, byte) in [(&first, 1), (&second, 2)] {
            write_marker_binding(&parent.join("debugging"), binding(byte));
            replace_entries(parent, vec![public_entry("debugging", byte)]);
        }
        fs::create_dir_all(corrupt.join(INDEX_DIRECTORY_NAME)).unwrap();
        fs::write(index_path(&corrupt), b"malformed").unwrap();

        assert_eq!(
            lookup_installed_skill(vec![first.clone(), corrupt.clone()], "debugging"),
            InstalledSkillAttribution::AttributionUnavailable
        );
        assert_eq!(
            lookup_installed_skill(vec![first, second, corrupt], "debugging"),
            InstalledSkillAttribution::Ambiguous
        );
    }

    #[test]
    fn duplicate_parent_paths_do_not_create_false_ambiguity() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("skills");
        write_marker_binding(&parent.join("debugging"), binding(1));
        replace_entries(&parent, vec![public_entry("debugging", 1)]);

        assert!(matches!(
            lookup_installed_skill(vec![parent.clone(), parent], "debugging"),
            InstalledSkillAttribution::Public(_)
        ));
    }
}
