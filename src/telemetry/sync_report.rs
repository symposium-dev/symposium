//! Safe projection of one full resolution and synchronization operation.
//!
//! Sync supplies admitted packages and private one-pass evidence. This module
//! applies public naming policy and retains only bounded, content-safe values.
//! Filesystem layout and supplier-local package identity never cross this
//! boundary.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "resolution row staging consumes the safe report in the next slice"
    )
)]

use std::time::Duration;

use crate::pm::{PackageHandle, ResolvedDependency};

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
/// package coordinates, supplier keys, filesystem paths, and private names.
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

/// Private source values consumed while projecting a [`SyncReport`].
pub(crate) struct SyncReportInput<'a> {
    pub(crate) outcome: ResolutionOutcome,
    pub(crate) duration: Duration,
    pub(crate) packages: &'a [ResolvedDependency],
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
            match policy::package(package.naming_view()) {
                PackageNaming::Public(coordinate) => {
                    packages.push(SafePackageResolution {
                        package: coordinate,
                        extension_match: extension_match(package.handle(), &input.extensions),
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

    #[cfg(test)]
    pub(crate) fn artifact_counts(&self) -> (u64, u64, u64) {
        (self.installed, self.updated, self.reaped)
    }
}

fn extension_match(package: PackageHandle, extensions: &[&ResolutionEvidence]) -> ExtensionMatch {
    let mut matched_unnamed = false;
    for evidence in extensions {
        debug_assert!(
            evidence.is_complete_extension(),
            "BUG: a sync report must receive complete extension evidence"
        );
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
    use symposium_sdk::{
        pm::{PackageId, PackageSourceClaim},
        predicate::syntax::{Dependency, Predicate, PredicateSet},
    };

    use super::*;
    use crate::{
        pm::{CARGO_PM, PackageNamingAuthority},
        predicate::ResolvedPredicateContext,
        skills::SkillNamingView,
        telemetry::{
            OptionalWitnessMode, evaluate_resolution_predicates, skill_resolution_evidence,
        },
    };

    fn dependency(
        slot: usize,
        name: &str,
        version: &str,
        source: PackageSourceClaim,
        authority: PackageNamingAuthority,
    ) -> ResolvedDependency {
        ResolvedDependency::for_test(
            slot,
            PackageId::new(CARGO_PM, name, version),
            source,
            authority,
        )
    }

    fn crates_io(slot: usize, name: &str) -> ResolvedDependency {
        dependency(
            slot,
            name,
            "1.2.3",
            PackageSourceClaim::CratesIo,
            PackageNamingAuthority::BuiltinCargo,
        )
    }

    fn evidence_for(
        dependency: &ResolvedDependency,
        extension_source: PackageSourceClaim,
        extension_authority: PackageNamingAuthority,
        extension_name: &str,
    ) -> ResolutionEvidence {
        let dependencies = [dependency.clone()];
        let mut context = ResolvedPredicateContext::new(&dependencies);
        let predicates = PredicateSet {
            predicates: vec![Predicate::DependsOn(Dependency::named(
                dependency.id().name.clone(),
            ))],
        };
        let predicates = evaluate_resolution_predicates(
            &predicates,
            &mut context,
            &mut OptionalWitnessMode::new(true),
        )
        .unwrap()
        .unwrap();
        let extension_id = PackageId::new(CARGO_PM, extension_name, "1.2.3");
        let extension = crate::pm::PackageNamingView::for_test(
            &extension_id,
            extension_source,
            extension_authority,
        );

        skill_resolution_evidence(
            Some(predicates),
            None,
            None,
            SkillNamingView::for_test(extension_name, Some(extension)),
        )
        .unwrap()
    }

    #[test]
    fn projection_counts_unnamed_reasons_and_extension_matches() {
        let public = crates_io(0, "public-input");
        let unnamed_only = crates_io(1, "unnamed-content");
        let unmatched = crates_io(2, "unmatched");
        let unnamed = [
            dependency(
                3,
                "private-registry",
                "1.2.3",
                PackageSourceClaim::Registry,
                PackageNamingAuthority::Remote,
            ),
            dependency(
                4,
                "git-input",
                "1.2.3",
                PackageSourceClaim::Git,
                PackageNamingAuthority::BuiltinCargo,
            ),
            dependency(
                5,
                "path-input",
                "1.2.3",
                PackageSourceClaim::Path,
                PackageNamingAuthority::BuiltinCargo,
            ),
            dependency(
                6,
                "workspace-input",
                "1.2.3",
                PackageSourceClaim::Workspace,
                PackageNamingAuthority::BuiltinCargo,
            ),
            dependency(
                7,
                "unknown-input",
                "1.2.3",
                PackageSourceClaim::Unknown,
                PackageNamingAuthority::BuiltinCargo,
            ),
            dependency(
                8,
                "invalid-coordinate",
                "*",
                PackageSourceClaim::CratesIo,
                PackageNamingAuthority::BuiltinCargo,
            ),
        ];
        let public_evidence = evidence_for(
            &public,
            PackageSourceClaim::CratesIo,
            PackageNamingAuthority::BuiltinCargo,
            "public-skill",
        );
        let unnamed_evidence = evidence_for(
            &unnamed_only,
            PackageSourceClaim::Path,
            PackageNamingAuthority::BuiltinCargo,
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
