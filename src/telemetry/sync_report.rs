//! Safe projection of one full resolution and synchronization operation.
//!
//! The sync layer supplies private package provenance and one-pass resolution
//! evidence. This module applies the public naming policy and returns only
//! bounded, content-safe values. Filesystem layout never crosses this boundary.

use std::time::Duration;

use crate::pm::SourcedPackage;

use super::{
    policy::{self, PackageNaming},
    resolution_witness::{ResolutionEvidence, SafeExtensionResolution},
    schema::{ExtensionMatch, PublicPackageCoordinate, ResolutionOutcome, UnnamedPackageReasons},
};

/// One public package and the strongest extension relationship it produced.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SafePackageResolution {
    package: PublicPackageCoordinate,
    extension_match: ExtensionMatch,
}

/// Telemetry-safe result of one full resolution and sync.
///
/// This type may derive `Debug`: public naming policy has already removed raw
/// provenance, private package names, and filesystem paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyncReport {
    outcome: ResolutionOutcome,
    duration_ms: u64,
    packages: Vec<SafePackageResolution>,
    unnamed_package_reasons: UnnamedPackageReasons,
    plugins: u64,
    skills: u64,
    installed: u64,
    updated: u64,
    reaped: u64,
    extensions: Vec<SafeExtensionResolution>,
}

/// Private source values from which [`SyncReport`] is projected.
pub(crate) struct SyncReportInput<'a> {
    pub(crate) outcome: ResolutionOutcome,
    pub(crate) duration: Duration,
    pub(crate) packages: &'a [SourcedPackage],
    pub(crate) extensions: Vec<&'a ResolutionEvidence>,
    pub(crate) plugins: usize,
    pub(crate) skills: usize,
    pub(crate) installed: usize,
    pub(crate) updated: usize,
    pub(crate) reaped: usize,
}

impl SyncReport {
    /// Apply public naming policy and discard every private input.
    #[must_use]
    pub(crate) fn project(input: SyncReportInput<'_>) -> Self {
        let mut packages = Vec::new();
        let mut unnamed_package_reasons = UnnamedPackageReasons::default();

        for package in input.packages {
            match policy::package(package) {
                PackageNaming::Public(coordinate) => {
                    let extension_match = extension_match(package, &input.extensions);
                    packages.push(SafePackageResolution {
                        package: coordinate,
                        extension_match,
                    });
                }
                PackageNaming::Unnamed(reason) => unnamed_package_reasons
                    .checked_record(reason)
                    .expect("BUG: the in-memory package count must fit in u64"),
            }
        }

        let mut extensions = Vec::new();
        for evidence in &input.extensions {
            if let Some(resolution) = evidence.safe_resolution()
                && !extensions.contains(&resolution)
            {
                extensions.push(resolution);
            }
        }

        Self {
            outcome: input.outcome,
            duration_ms: u64::try_from(input.duration.as_millis()).unwrap_or(u64::MAX),
            packages,
            unnamed_package_reasons,
            plugins: count(input.plugins),
            skills: count(input.skills),
            installed: count(input.installed),
            updated: count(input.updated),
            reaped: count(input.reaped),
            extensions,
        }
    }

    #[cfg(test)]
    pub(crate) fn outcome(&self) -> ResolutionOutcome {
        self.outcome
    }
}

fn extension_match(package: &SourcedPackage, extensions: &[&ResolutionEvidence]) -> ExtensionMatch {
    let mut matched_unnamed = false;
    for evidence in extensions {
        if !evidence.contributes(package) {
            continue;
        }
        if evidence.names_public_extension() {
            return ExtensionMatch::Public;
        }
        matched_unnamed = true;
    }

    if matched_unnamed {
        ExtensionMatch::UnnamedOnly
    } else {
        ExtensionMatch::None
    }
}

fn count(value: usize) -> u64 {
    u64::try_from(value).expect("BUG: usize must fit in u64 on supported targets")
}

#[cfg(test)]
mod tests {
    use symposium_sdk::pm::PackageId;

    use super::*;
    use crate::{
        config::BUILTIN_RECOMMENDATIONS_URL,
        pm::{ANY_VERSION, CARGO_PM, CRATES_IO_REGISTRY_URL, PackageProvenance},
        telemetry::{OptionalWitnessMode, evaluate_resolution_predicates},
    };
    use symposium_sdk::predicate::syntax::{Predicate, PredicateSet};

    fn package_with(
        pm: &str,
        name: &str,
        version: &str,
        provenance: PackageProvenance,
    ) -> SourcedPackage {
        SourcedPackage::new(PackageId::new(pm, name, version), provenance)
    }

    fn package(name: &str, provenance: PackageProvenance) -> SourcedPackage {
        package_with(CARGO_PM, name, "1.2.3", provenance)
    }

    fn evidence_for(
        package: &SourcedPackage,
        extension_provenance: PackageProvenance,
        extension_name: &str,
    ) -> ResolutionEvidence {
        let packages = vec![package.clone()];
        let mut context = crate::predicate::SourcedPredicateContext::new(&packages);
        let predicates = PredicateSet {
            predicates: vec![Predicate::DependsOn(package.id.name.clone(), None)],
        };
        let evidence = evaluate_resolution_predicates(
            &predicates,
            &mut context,
            &mut OptionalWitnessMode::new(true),
        )
        .unwrap()
        .unwrap();

        super::super::resolution_witness::skill(
            Some(evidence),
            None,
            None,
            &extension_provenance,
            extension_name,
        )
        .unwrap()
    }

    #[test]
    fn projection_counts_each_unnamed_reason_and_extension_match() {
        let public = package(
            "public-input",
            PackageProvenance::Registry {
                url: CRATES_IO_REGISTRY_URL.to_owned(),
            },
        );
        let unnamed_only = package(
            "unnamed-content",
            PackageProvenance::Registry {
                url: CRATES_IO_REGISTRY_URL.to_owned(),
            },
        );
        let unmatched = package(
            "unmatched",
            PackageProvenance::Registry {
                url: CRATES_IO_REGISTRY_URL.to_owned(),
            },
        );
        let unnamed = [
            package(
                "private-registry",
                PackageProvenance::Registry {
                    url: "https://example.invalid/index".to_owned(),
                },
            ),
            package(
                "git-input",
                PackageProvenance::Git {
                    url: "https://example.invalid/repository".to_owned(),
                },
            ),
            package("path-input", PackageProvenance::Path),
            package("workspace-input", PackageProvenance::Workspace),
            package("unknown-input", PackageProvenance::Unknown),
            package_with(
                CARGO_PM,
                "invalid-coordinate",
                ANY_VERSION,
                PackageProvenance::Registry {
                    url: CRATES_IO_REGISTRY_URL.to_owned(),
                },
            ),
        ];
        let public_evidence = evidence_for(
            &public,
            PackageProvenance::Git {
                url: BUILTIN_RECOMMENDATIONS_URL.to_owned(),
            },
            "public-skill",
        );
        let unnamed_evidence = evidence_for(
            &unnamed_only,
            PackageProvenance::Git {
                url: "https://example.invalid/private".to_owned(),
            },
            "private-skill",
        );
        let packages = [public, unnamed_only, unmatched]
            .into_iter()
            .chain(unnamed)
            .collect::<Vec<_>>();

        let report = SyncReport::project(SyncReportInput {
            outcome: ResolutionOutcome::Partial,
            duration: Duration::from_millis(42),
            packages: &packages,
            extensions: vec![&public_evidence, &unnamed_evidence],
            plugins: 2,
            skills: 3,
            installed: 1,
            updated: 1,
            reaped: 1,
        });

        assert_eq!(report.outcome, ResolutionOutcome::Partial);
        assert_eq!(report.duration_ms, 42);
        assert_eq!(report.packages.len(), 3);
        assert_eq!(report.packages[0].extension_match, ExtensionMatch::Public);
        assert_eq!(
            report.packages[1].extension_match,
            ExtensionMatch::UnnamedOnly
        );
        assert_eq!(report.packages[2].extension_match, ExtensionMatch::None);
        assert_eq!(
            serde_json::to_value(report.unnamed_package_reasons).unwrap(),
            serde_json::json!({
                "private_registry": 1,
                "git": 1,
                "path": 1,
                "workspace": 1,
                "unknown_source": 1,
                "invalid_coordinate": 1,
            })
        );
        assert_eq!(report.extensions.len(), 1);
        assert_eq!((report.plugins, report.skills), (2, 3));
        assert_eq!((report.installed, report.updated, report.reaped), (1, 1, 1));
    }
}
