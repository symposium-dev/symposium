//! Supplier-scoped admission for package-manager resolution facts.
//!
//! Wire values can claim a coordinate and source class, but they cannot grant
//! themselves naming authority. This module binds each accepted value to the
//! package-manager instance that supplied it and replaces its sensitive wire
//! key with a session-local handle before the value leaves `pm`.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use symposium_sdk::pm::{PackageId, PackageKey, PackageResolution, PackageSourceClaim};

static NEXT_SUPPLIER_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct SupplierId(u64);

/// A session-local identity for one package admitted from one supplier.
///
/// Handles are deliberately opaque. In particular, they expose neither the
/// supplier's key nor a representation that can be serialized or logged.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct PackageHandle {
    supplier: SupplierId,
    slot: usize,
}

/// Core-owned authority attached while a package-manager answer is admitted.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PackageAuthority {
    BuiltinCargo,
    Recommendations,
    Remote,
}

/// Core-owned authority visible to the public-naming policy.
///
/// Constructing this value does not grant authority: the policy accepts it
/// only as part of a [`PackageNamingView`] minted from an admitted package.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "resolution producers activate every public-naming authority in a later slice"
    )
)]
pub(crate) enum PackageNamingAuthority {
    BuiltinCargo,
    Recommendations,
    Remote,
}

/// Borrowed package facts available to the public-naming policy.
///
/// The private constructor keeps supplier keys, handles, content roots, and
/// raw source representations outside the policy boundary. The type has no
/// diagnostic or serialization implementation because its admitted coordinate
/// may still be private.
#[derive(Clone, Copy)]
pub(crate) struct PackageNamingView<'a> {
    id: &'a PackageId,
    source: PackageSourceClaim,
    authority: PackageNamingAuthority,
}

impl<'a> PackageNamingView<'a> {
    pub(crate) fn id(self) -> &'a PackageId {
        self.id
    }

    pub(crate) fn source(self) -> PackageSourceClaim {
        self.source
    }

    pub(crate) fn authority(self) -> PackageNamingAuthority {
        self.authority
    }

    #[cfg(test)]
    pub(crate) const fn for_test(
        id: &'a PackageId,
        source: PackageSourceClaim,
        authority: PackageNamingAuthority,
    ) -> Self {
        Self {
            id,
            source,
            authority,
        }
    }
}

/// The source-sensitive package fact core has admitted for this session.
///
/// This type has no serialization and its deliberate `Debug` view omits the
/// coordinate. The sensitive supplier key remains in the admission ledger and
/// is represented here only by [`PackageHandle`].
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ResolvedPackage {
    handle: PackageHandle,
    id: PackageId,
    source: PackageSourceClaim,
    authority: PackageAuthority,
}

impl std::fmt::Debug for ResolvedPackage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ResolvedPackage")
            .field("source", &self.source)
            .field(
                "authority",
                &match self.authority {
                    PackageAuthority::BuiltinCargo => "builtin_cargo",
                    PackageAuthority::Recommendations => "recommendations",
                    PackageAuthority::Remote => "remote",
                },
            )
            .finish_non_exhaustive()
    }
}

impl ResolvedPackage {
    pub(crate) fn handle(&self) -> PackageHandle {
        self.handle
    }

    pub(crate) fn id(&self) -> &PackageId {
        &self.id
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "resolution producers activate the public-naming policy in a later slice"
        )
    )]
    pub(crate) fn naming_view(&self) -> PackageNamingView<'_> {
        PackageNamingView {
            id: &self.id,
            source: self.source,
            authority: match self.authority {
                PackageAuthority::BuiltinCargo => PackageNamingAuthority::BuiltinCargo,
                PackageAuthority::Recommendations => PackageNamingAuthority::Recommendations,
                PackageAuthority::Remote => PackageNamingAuthority::Remote,
            },
        }
    }
}

/// One exact package selected from a supplier's dependency snapshot.
///
/// The newtype prevents dependency-inspection APIs from accepting a coordinate
/// that has lost the supplier and key which selected it.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ResolvedDependency(ResolvedPackage);

impl ResolvedDependency {
    pub(crate) fn id(&self) -> &PackageId {
        self.0.id()
    }

    pub(crate) fn into_package(self) -> ResolvedPackage {
        self.0
    }
}

/// Resolved package content after its wire key has been confined to the
/// supplier ledger.
#[derive(Clone)]
pub struct ResolvedContent {
    package: ResolvedPackage,
    root: PathBuf,
}

impl std::fmt::Debug for ResolvedContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedContent")
            .field("package", &self.package)
            .finish_non_exhaustive()
    }
}

impl ResolvedContent {
    /// The admitted package's exact coordinate.
    pub fn id(&self) -> &PackageId {
        self.package.id()
    }

    /// The canonical directory containing the resolved package.
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn package(&self) -> &ResolvedPackage {
        &self.package
    }
}

/// Why a supplier-provided resolution could not be admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AdmissionError {
    ForeignPackageManager,
    DuplicateKey,
    ConflictingKey,
    ContentRootUnavailable,
    ConflictingContentRoot,
    StaleHandle,
}

impl std::fmt::Display for AdmissionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ForeignPackageManager => {
                formatter.write_str("package manager answered for another supplier")
            }
            Self::DuplicateKey => formatter
                .write_str("package manager repeated a package key in one dependency listing"),
            Self::ConflictingKey => formatter
                .write_str("package manager reused one package key for conflicting resolutions"),
            Self::ContentRootUnavailable => {
                formatter.write_str("package manager supplied an unavailable content root")
            }
            Self::ConflictingContentRoot => formatter
                .write_str("package manager reused one package key for another content root"),
            Self::StaleHandle => {
                formatter.write_str("package handle does not belong to this supplier session")
            }
        }
    }
}

impl std::error::Error for AdmissionError {}

struct AdmittedPackage {
    resolution: PackageResolution,
    canonical_root: Option<PathBuf>,
}

#[derive(Default)]
struct Ledger {
    records: Vec<AdmittedPackage>,
    by_key: HashMap<PackageKey, usize>,
    dependency_slots: Option<Vec<usize>>,
}

impl Ledger {
    fn admit_dependencies(
        &mut self,
        resolutions: Vec<PackageResolution>,
    ) -> Result<Vec<usize>, AdmissionError> {
        self.check_existing(&resolutions)?;

        let mut slots = Vec::with_capacity(resolutions.len());
        for resolution in resolutions {
            let slot = match self.by_key.get(resolution.key()).copied() {
                Some(slot) => slot,
                None => {
                    let slot = self.records.len();
                    self.by_key.insert(resolution.key().clone(), slot);
                    self.records.push(AdmittedPackage {
                        resolution,
                        canonical_root: None,
                    });
                    slot
                }
            };
            slots.push(slot);
        }
        self.dependency_slots = Some(slots.clone());
        Ok(slots)
    }

    fn admit_content(
        &mut self,
        resolution: PackageResolution,
        canonical_root: &Path,
    ) -> Result<usize, AdmissionError> {
        self.check_existing(std::slice::from_ref(&resolution))?;

        match self.by_key.get(resolution.key()).copied() {
            Some(slot) => {
                let record = &mut self.records[slot];
                match &record.canonical_root {
                    Some(existing) if existing != canonical_root => {
                        Err(AdmissionError::ConflictingContentRoot)
                    }
                    Some(_) => Ok(slot),
                    None => {
                        record.canonical_root = Some(canonical_root.to_owned());
                        Ok(slot)
                    }
                }
            }
            None => {
                let slot = self.records.len();
                self.by_key.insert(resolution.key().clone(), slot);
                self.records.push(AdmittedPackage {
                    resolution,
                    canonical_root: Some(canonical_root.to_owned()),
                });
                Ok(slot)
            }
        }
    }

    fn check_existing(&self, resolutions: &[PackageResolution]) -> Result<(), AdmissionError> {
        for resolution in resolutions {
            if let Some(&slot) = self.by_key.get(resolution.key()) {
                let record = self.record(slot);
                if &record.resolution != resolution {
                    return Err(AdmissionError::ConflictingKey);
                }
            }
        }
        Ok(())
    }

    fn record(&self, slot: usize) -> &AdmittedPackage {
        self.records
            .get(slot)
            .expect("BUG: ledger slot must name an admitted package")
    }
}

/// One package-manager instance's current resolution session.
pub(super) struct PackageAdmission {
    supplier: SupplierId,
    owner: String,
    authority: PackageAuthority,
    ledger: Mutex<Ledger>,
}

impl PackageAdmission {
    pub(super) fn new(owner: String, authority: PackageAuthority) -> Self {
        Self {
            supplier: SupplierId(NEXT_SUPPLIER_ID.fetch_add(1, Ordering::Relaxed)),
            owner,
            authority,
            ledger: Mutex::new(Ledger::default()),
        }
    }

    /// Admit a complete dependency listing transactionally.
    ///
    /// Every coordinate, key, and existing ledger entry is checked before any
    /// new record is published. Any key repeated within the listing rejects
    /// the complete dependency snapshot, even when both entries agree.
    pub(super) fn admit_dependencies(
        &self,
        resolutions: Vec<PackageResolution>,
    ) -> Result<Vec<ResolvedDependency>, AdmissionError> {
        self.preflight_dependencies(&resolutions)?;
        let mut ledger = self.ledger.lock().expect("package admission lock poisoned");
        let slots = ledger.admit_dependencies(resolutions)?;
        Ok(slots
            .into_iter()
            .map(|slot| ResolvedDependency(self.lower(slot, &ledger.record(slot).resolution)))
            .collect())
    }

    /// Admit one content-bearing result without invalidating its valid siblings.
    pub(super) fn admit_content(
        &self,
        resolution: PackageResolution,
        root: &Path,
    ) -> Result<ResolvedContent, AdmissionError> {
        self.check_owner(&resolution)?;
        let canonical_root = dunce::canonicalize(root)
            .ok()
            .filter(|root| root.is_dir())
            .ok_or(AdmissionError::ContentRootUnavailable)?;
        let mut ledger = self.ledger.lock().expect("package admission lock poisoned");
        let slot = ledger.admit_content(resolution, &canonical_root)?;
        let package = self.lower(slot, &ledger.record(slot).resolution);
        Ok(ResolvedContent {
            package,
            root: canonical_root,
        })
    }

    fn preflight_dependencies(
        &self,
        resolutions: &[PackageResolution],
    ) -> Result<(), AdmissionError> {
        let mut seen = HashMap::<&PackageKey, &PackageResolution>::with_capacity(resolutions.len());

        for resolution in resolutions {
            self.check_owner(resolution)?;
            if let Some(previous) = seen.insert(resolution.key(), resolution) {
                if previous != resolution {
                    return Err(AdmissionError::ConflictingKey);
                }
                return Err(AdmissionError::DuplicateKey);
            }
        }
        Ok(())
    }

    fn check_owner(&self, resolution: &PackageResolution) -> Result<(), AdmissionError> {
        if resolution.id().pm == self.owner {
            Ok(())
        } else {
            Err(AdmissionError::ForeignPackageManager)
        }
    }

    /// Project an admitted package back to the wire only for its supplier.
    pub(super) fn project(
        &self,
        handle: PackageHandle,
    ) -> Result<PackageResolution, AdmissionError> {
        if handle.supplier != self.supplier {
            return Err(AdmissionError::StaleHandle);
        }
        self.ledger
            .lock()
            .expect("package admission lock poisoned")
            .records
            .get(handle.slot)
            .map(|record| record.resolution.clone())
            .ok_or(AdmissionError::StaleHandle)
    }

    /// The supplier's exact dependency records for an ID-only predicate set.
    ///
    /// Equal coordinates may name several supplier keys; all are retained so
    /// dependency inspection never chooses one by ordering.
    pub(super) fn project_dependencies(&self, ids: &[PackageId]) -> Vec<PackageResolution> {
        let ledger = self.ledger.lock().expect("package admission lock poisoned");
        ledger
            .dependency_slots
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|&slot| ledger.record(slot))
            .map(|record| &record.resolution)
            .filter(|resolution| ids.contains(resolution.id()))
            .cloned()
            .collect()
    }

    /// The admitted dependency handles matching an ID-only predicate set.
    ///
    /// Equal coordinates remain separate handles; callers must not select one
    /// by ordering.
    pub(super) fn resolved_dependencies(&self, ids: &[PackageId]) -> Vec<ResolvedDependency> {
        let ledger = self.ledger.lock().expect("package admission lock poisoned");
        ledger
            .dependency_slots
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter_map(|&slot| {
                let record = ledger.record(slot);
                ids.contains(record.resolution.id())
                    .then(|| ResolvedDependency(self.lower(slot, &record.resolution)))
            })
            .collect()
    }

    pub(super) fn has_dependency_snapshot(&self) -> bool {
        self.ledger
            .lock()
            .expect("package admission lock poisoned")
            .dependency_slots
            .is_some()
    }

    pub(super) fn invalidate_dependency_snapshot(&self) {
        self.ledger
            .lock()
            .expect("package admission lock poisoned")
            .dependency_slots = None;
    }

    fn lower(&self, slot: usize, resolution: &PackageResolution) -> ResolvedPackage {
        ResolvedPackage {
            handle: PackageHandle {
                supplier: self.supplier,
                slot,
            },
            id: resolution.id().clone(),
            source: resolution.source(),
            authority: self.authority,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolution(key: &str, name: &str, source: PackageSourceClaim) -> PackageResolution {
        PackageResolution::new(
            PackageId::new("cargo", name, "1.0.0"),
            source,
            PackageKey::new(key).unwrap(),
        )
    }

    #[test]
    fn dependency_admission_is_transactional() {
        let admission = PackageAdmission::new("cargo".into(), PackageAuthority::BuiltinCargo);
        let error = admission
            .admit_dependencies(vec![
                resolution("same", "first", PackageSourceClaim::CratesIo),
                resolution("same", "second", PackageSourceClaim::CratesIo),
            ])
            .err();
        assert_eq!(error, Some(AdmissionError::ConflictingKey));

        let admitted = admission
            .admit_dependencies(vec![resolution(
                "same",
                "second",
                PackageSourceClaim::CratesIo,
            )])
            .unwrap();
        assert_eq!(admitted[0].id().name, "second");
    }

    #[test]
    fn failed_dependency_refresh_preserves_the_ledger() {
        let admission = PackageAdmission::new("cargo".into(), PackageAuthority::BuiltinCargo);
        let stable = resolution("stable", "existing", PackageSourceClaim::CratesIo);
        let stable_handle = admission
            .admit_dependencies(vec![stable.clone()])
            .unwrap()
            .pop()
            .unwrap()
            .into_package()
            .handle();

        let error = admission
            .admit_dependencies(vec![
                resolution("new", "staged", PackageSourceClaim::CratesIo),
                resolution("stable", "conflict", PackageSourceClaim::CratesIo),
            ])
            .err();
        assert_eq!(error, Some(AdmissionError::ConflictingKey));
        assert!(admission.project(stable_handle).unwrap() == stable);

        // The first entry in the failed refresh was not published either.
        let replacement = admission.admit_dependencies(vec![resolution(
            "new",
            "replacement",
            PackageSourceClaim::Registry,
        )]);
        assert!(replacement.is_ok());
    }

    #[test]
    fn repeated_keys_must_keep_the_same_fact() {
        let root = tempfile::tempdir().unwrap();
        let admission = PackageAdmission::new("cargo".into(), PackageAuthority::BuiltinCargo);
        let first = admission
            .admit_content(
                resolution("stable", "widget", PackageSourceClaim::CratesIo),
                root.path(),
            )
            .unwrap();
        let repeated = admission
            .admit_content(
                resolution("stable", "widget", PackageSourceClaim::CratesIo),
                root.path(),
            )
            .unwrap();
        assert!(first.package.handle() == repeated.package.handle());

        let error = admission
            .admit_content(
                resolution("stable", "widget", PackageSourceClaim::Registry),
                root.path(),
            )
            .err();
        assert_eq!(error, Some(AdmissionError::ConflictingKey));
    }

    #[test]
    fn repeated_dependency_keys_are_rejected_even_when_they_agree() {
        let admission = PackageAdmission::new("cargo".into(), PackageAuthority::BuiltinCargo);
        let package = resolution("same", "widget", PackageSourceClaim::CratesIo);

        assert_eq!(
            admission
                .admit_dependencies(vec![package.clone(), package])
                .err(),
            Some(AdmissionError::DuplicateKey)
        );
    }

    #[test]
    fn one_package_key_cannot_change_its_canonical_content_root() {
        let first_root = tempfile::tempdir().unwrap();
        let second_root = tempfile::tempdir().unwrap();
        let admission = PackageAdmission::new("cargo".into(), PackageAuthority::BuiltinCargo);
        let package = resolution("stable", "widget", PackageSourceClaim::CratesIo);
        let admitted = admission
            .admit_content(package.clone(), first_root.path())
            .unwrap();
        assert_eq!(
            admitted.root(),
            dunce::canonicalize(first_root.path()).unwrap()
        );

        assert_eq!(
            admission.admit_content(package, second_root.path()).err(),
            Some(AdmissionError::ConflictingContentRoot)
        );
    }

    #[test]
    fn handles_project_only_through_their_supplier() {
        let cargo = PackageAdmission::new("cargo".into(), PackageAuthority::BuiltinCargo);
        let remote = PackageAdmission::new("cargo".into(), PackageAuthority::Remote);
        let root = tempfile::tempdir().unwrap();
        let content = cargo
            .admit_content(
                resolution("private-key", "widget", PackageSourceClaim::CratesIo),
                root.path(),
            )
            .unwrap();
        let package = content.package;

        let naming = package.naming_view();
        assert_eq!(naming.source(), PackageSourceClaim::CratesIo);
        assert!(matches!(
            naming.authority(),
            PackageNamingAuthority::BuiltinCargo
        ));
        let diagnostic = format!("{package:?}");
        assert!(!diagnostic.contains("widget"));
        assert!(!diagnostic.contains("private-key"));

        assert_eq!(
            remote.project(package.handle()).err(),
            Some(AdmissionError::StaleHandle)
        );
        assert_eq!(cargo.project(package.handle()).unwrap().id(), package.id());
    }
}
