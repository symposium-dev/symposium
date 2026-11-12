//! Public naming policy for package and extension telemetry.
//!
//! Package managers supply coordinates and source classifications, while core
//! attaches authority during admission. This module is the only place those
//! two facts may become frozen public telemetry labels. Supplier keys, handles,
//! paths, and raw source representations never enter this boundary.
//!
//! Source rejection precedes coordinate validation. Packages retain one
//! bounded unnamed reason for resolution accounting. Extensions do not: their
//! broader attribution vocabulary also contains indexing and signal failures
//! this policy cannot observe, so this layer reports only public or unnamed.
//! Recommendations are consequently public for extensions but remain `git`
//! for package-resolution accounting.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "resolution and extension producers consume this policy in later slices"
    )
)]

use crate::{
    pm::{CARGO_PM, PackageNamingAuthority, PackageNamingView},
    skills::SkillNamingView,
};
use symposium_sdk::pm::PackageSourceClaim;

use super::schema::{
    ExtensionKind, PackageEcosystem, PublicExtensionCoordinate, PublicExtensionSource,
    PublicPackageCoordinate, UnnamedPackageReason,
};

/// Naming decision for one resolved package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::telemetry) enum PackageNaming {
    /// The package has a validated coordinate from an allowlisted ecosystem.
    Public(PublicPackageCoordinate),
    /// The package remains unnamed for exactly one contract reason.
    Unnamed(UnnamedPackageReason),
}

/// Naming decision for one plugin or skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::telemetry) enum ExtensionNaming {
    /// The extension has a validated coordinate from an allowlisted source.
    Public(PublicExtensionCoordinate),
    /// The extension remains usable but unnamed in telemetry.
    Unnamed,
}

/// Source eligibility shared by package and extension naming.
///
/// Recommendations may name extensions, but retain the Git rejection reason
/// when projected as packages.
enum SourceDecision {
    PublicCargo,
    PublicRecommendations,
    Unnamed(UnnamedPackageReason),
}

fn source_decision(view: PackageNamingView<'_>) -> SourceDecision {
    use PackageNamingAuthority as Authority;
    use PackageSourceClaim as Source;
    use SourceDecision as Decision;
    use UnnamedPackageReason as Reason;

    let (cargo, recommendations) = match view.authority() {
        Authority::BuiltinCargo => (true, false),
        Authority::Recommendations => (false, true),
        Authority::Remote => (false, false),
    };

    match view.source() {
        Source::CratesIo if cargo => Decision::PublicCargo,
        Source::CratesIo => Decision::Unnamed(Reason::UnknownSource),
        Source::Git if recommendations => Decision::PublicRecommendations,
        Source::Git => Decision::Unnamed(Reason::Git),
        Source::Workspace if cargo => Decision::Unnamed(Reason::Workspace),
        Source::Workspace => Decision::Unnamed(Reason::UnknownSource),
        Source::Registry => Decision::Unnamed(Reason::PrivateRegistry),
        Source::Path => Decision::Unnamed(Reason::Path),
        Source::Unknown => Decision::Unnamed(Reason::UnknownSource),
    }
}

/// Select the frozen public extension source for one admitted package.
///
/// Commands consume this source decision directly because their coordinate
/// carries both a plugin name and a command name. Plugin and skill naming use
/// the dedicated entry points below, which also validate the extension name.
#[must_use]
pub(in crate::telemetry) fn extension_source(
    package: PackageNamingView<'_>,
) -> Option<PublicExtensionSource> {
    match source_decision(package) {
        SourceDecision::PublicCargo => Some(PublicExtensionSource::CratesIo),
        SourceDecision::PublicRecommendations => {
            Some(PublicExtensionSource::SymposiumRecommendations)
        }
        SourceDecision::Unnamed(_) => None,
    }
}

/// Apply the version 1 public package policy to one admitted package.
#[must_use]
pub(in crate::telemetry) fn package(package: PackageNamingView<'_>) -> PackageNaming {
    match source_decision(package) {
        SourceDecision::PublicCargo => {
            if package.id().pm != CARGO_PM {
                return PackageNaming::Unnamed(UnnamedPackageReason::UnknownSource);
            }

            match PublicPackageCoordinate::try_new(
                PackageEcosystem::Cargo,
                &package.id().name,
                &package.id().version,
            ) {
                Ok(coordinate) => PackageNaming::Public(coordinate),
                Err(_) => PackageNaming::Unnamed(UnnamedPackageReason::InvalidCoordinate),
            }
        }
        SourceDecision::PublicRecommendations => PackageNaming::Unnamed(UnnamedPackageReason::Git),
        SourceDecision::Unnamed(reason) => PackageNaming::Unnamed(reason),
    }
}

/// Apply the version 1 public extension policy to one plugin.
#[must_use]
pub(in crate::telemetry) fn plugin(package: PackageNamingView<'_>, name: &str) -> ExtensionNaming {
    extension(package, ExtensionKind::Plugin, name)
}

/// Apply the version 1 public extension policy to one discovered skill.
///
/// The skill view carries the containment proof selected during discovery. A
/// detached skill therefore cannot borrow its parent plugin's package view.
#[must_use]
pub(in crate::telemetry) fn skill(skill: SkillNamingView<'_>) -> ExtensionNaming {
    let Some(package) = skill.package() else {
        return ExtensionNaming::Unnamed;
    };

    extension(package, ExtensionKind::Skill, skill.name())
}

fn extension(package: PackageNamingView<'_>, kind: ExtensionKind, name: &str) -> ExtensionNaming {
    let Some(source) = extension_source(package) else {
        return ExtensionNaming::Unnamed;
    };

    match PublicExtensionCoordinate::try_new(kind, source, name) {
        Ok(coordinate) => ExtensionNaming::Public(coordinate),
        Err(_) => ExtensionNaming::Unnamed,
    }
}

#[cfg(test)]
mod tests {
    use symposium_sdk::pm::{PackageId, PackageSourceClaim};

    use super::*;

    fn view<'a>(
        id: &'a PackageId,
        authority: PackageNamingAuthority,
        source: PackageSourceClaim,
    ) -> PackageNamingView<'a> {
        PackageNamingView::for_test(id, source, authority)
    }

    #[test]
    fn builtin_cargo_crates_io_packages_receive_public_coordinates() {
        let id = PackageId::new("cargo", "example-tools", "1.2.3");

        let PackageNaming::Public(coordinate) = package(view(
            &id,
            PackageNamingAuthority::BuiltinCargo,
            PackageSourceClaim::CratesIo,
        )) else {
            panic!("expected a public package coordinate");
        };

        assert_eq!(coordinate.ecosystem(), PackageEcosystem::Cargo);
        assert_eq!(coordinate.name().as_str(), "example-tools");
        assert_eq!(coordinate.version().to_string(), "1.2.3");
    }

    #[test]
    fn builtin_cargo_authority_does_not_name_another_package_ecosystem() {
        let id = PackageId::new("another-pm", "example-tools", "1.2.3");

        assert_eq!(
            package(view(
                &id,
                PackageNamingAuthority::BuiltinCargo,
                PackageSourceClaim::CratesIo,
            )),
            PackageNaming::Unnamed(UnnamedPackageReason::UnknownSource)
        );
    }

    #[test]
    fn allowlisted_package_sources_still_require_valid_exact_coordinates() {
        for id in [
            PackageId::new("cargo", "private/package", "1.2.3"),
            PackageId::new("cargo", "example-tools", "*"),
        ] {
            assert_eq!(
                package(view(
                    &id,
                    PackageNamingAuthority::BuiltinCargo,
                    PackageSourceClaim::CratesIo,
                )),
                PackageNaming::Unnamed(UnnamedPackageReason::InvalidCoordinate)
            );
        }
    }

    struct SourceCase {
        authority: PackageNamingAuthority,
        source: PackageSourceClaim,
        package_reason: Option<UnnamedPackageReason>,
        extension_source: Option<PublicExtensionSource>,
    }

    fn source_cases() -> [SourceCase; 18] {
        use PackageNamingAuthority as Authority;
        use PackageSourceClaim as Source;
        use PublicExtensionSource as Extension;
        use UnnamedPackageReason as Reason;

        // Expected decisions are explicit and independent of source_decision.
        // No package reason means that source permits coordinate validation.
        [
            (
                Authority::BuiltinCargo,
                Source::CratesIo,
                None,
                Some(Extension::CratesIo),
            ),
            (
                Authority::BuiltinCargo,
                Source::Registry,
                Some(Reason::PrivateRegistry),
                None,
            ),
            (
                Authority::BuiltinCargo,
                Source::Git,
                Some(Reason::Git),
                None,
            ),
            (
                Authority::BuiltinCargo,
                Source::Path,
                Some(Reason::Path),
                None,
            ),
            (
                Authority::BuiltinCargo,
                Source::Workspace,
                Some(Reason::Workspace),
                None,
            ),
            (
                Authority::BuiltinCargo,
                Source::Unknown,
                Some(Reason::UnknownSource),
                None,
            ),
            (
                Authority::Recommendations,
                Source::CratesIo,
                Some(Reason::UnknownSource),
                None,
            ),
            (
                Authority::Recommendations,
                Source::Registry,
                Some(Reason::PrivateRegistry),
                None,
            ),
            (
                Authority::Recommendations,
                Source::Git,
                Some(Reason::Git),
                Some(Extension::SymposiumRecommendations),
            ),
            (
                Authority::Recommendations,
                Source::Path,
                Some(Reason::Path),
                None,
            ),
            (
                Authority::Recommendations,
                Source::Workspace,
                Some(Reason::UnknownSource),
                None,
            ),
            (
                Authority::Recommendations,
                Source::Unknown,
                Some(Reason::UnknownSource),
                None,
            ),
            (
                Authority::Remote,
                Source::CratesIo,
                Some(Reason::UnknownSource),
                None,
            ),
            (
                Authority::Remote,
                Source::Registry,
                Some(Reason::PrivateRegistry),
                None,
            ),
            (Authority::Remote, Source::Git, Some(Reason::Git), None),
            (Authority::Remote, Source::Path, Some(Reason::Path), None),
            (
                Authority::Remote,
                Source::Workspace,
                Some(Reason::UnknownSource),
                None,
            ),
            (
                Authority::Remote,
                Source::Unknown,
                Some(Reason::UnknownSource),
                None,
            ),
        ]
        .map(
            |(authority, source, package_reason, extension_source)| SourceCase {
                authority,
                source,
                package_reason,
                extension_source,
            },
        )
    }

    #[test]
    fn package_source_reasons_precede_coordinate_validation() {
        let invalid = PackageId::new("cargo", "private/package", "*");

        for (index, case) in source_cases().into_iter().enumerate() {
            let expected = case
                .package_reason
                .unwrap_or(UnnamedPackageReason::InvalidCoordinate);
            assert_eq!(
                package(view(&invalid, case.authority, case.source)),
                PackageNaming::Unnamed(expected),
                "source case {index}"
            );
        }
    }

    #[test]
    fn extension_sources_require_exact_authority_and_source_pairs() {
        let id = PackageId::new("supplier", "example-tools", "1.2.3");

        for (index, case) in source_cases().into_iter().enumerate() {
            assert_eq!(
                extension_source(view(&id, case.authority, case.source)),
                case.extension_source,
                "source case {index}"
            );
        }
    }

    #[test]
    fn plugins_are_named_only_after_source_and_name_validation() {
        let cargo = PackageId::new("cargo", "example-tools", "1.2.3");
        let recommendations = PackageId::new("recommendations", "example-tools", "1.2.3");
        let cases = [
            (
                plugin(
                    view(
                        &cargo,
                        PackageNamingAuthority::BuiltinCargo,
                        PackageSourceClaim::CratesIo,
                    ),
                    "example-tools",
                ),
                PublicExtensionSource::CratesIo,
            ),
            (
                plugin(
                    view(
                        &recommendations,
                        PackageNamingAuthority::Recommendations,
                        PackageSourceClaim::Git,
                    ),
                    "example-tools",
                ),
                PublicExtensionSource::SymposiumRecommendations,
            ),
        ];

        for (decision, expected_source) in cases {
            let ExtensionNaming::Public(coordinate) = decision else {
                panic!("expected a public plugin coordinate");
            };
            assert_eq!(coordinate.kind(), ExtensionKind::Plugin);
            assert_eq!(coordinate.source(), expected_source);
            assert_eq!(coordinate.name().as_str(), "example-tools");
        }

        assert_eq!(
            plugin(
                view(
                    &cargo,
                    PackageNamingAuthority::BuiltinCargo,
                    PackageSourceClaim::CratesIo,
                ),
                "private/plugin",
            ),
            ExtensionNaming::Unnamed
        );
        assert_eq!(
            plugin(
                view(
                    &cargo,
                    PackageNamingAuthority::BuiltinCargo,
                    PackageSourceClaim::Path,
                ),
                "example-tools",
            ),
            ExtensionNaming::Unnamed
        );
    }

    #[test]
    fn skills_require_their_own_contained_package_proof() {
        let cargo = PackageId::new("cargo", "example-tools", "1.2.3");
        let recommendations = PackageId::new("recommendations", "example-tools", "1.2.3");

        assert_eq!(
            skill(SkillNamingView::for_test("example-tools", None)),
            ExtensionNaming::Unnamed
        );

        for (package, expected_source) in [
            (
                view(
                    &cargo,
                    PackageNamingAuthority::BuiltinCargo,
                    PackageSourceClaim::CratesIo,
                ),
                PublicExtensionSource::CratesIo,
            ),
            (
                view(
                    &recommendations,
                    PackageNamingAuthority::Recommendations,
                    PackageSourceClaim::Git,
                ),
                PublicExtensionSource::SymposiumRecommendations,
            ),
        ] {
            let ExtensionNaming::Public(coordinate) =
                skill(SkillNamingView::for_test("example-tools", Some(package)))
            else {
                panic!("expected a public skill coordinate");
            };
            assert_eq!(coordinate.kind(), ExtensionKind::Skill);
            assert_eq!(coordinate.source(), expected_source);
            assert_eq!(coordinate.name().as_str(), "example-tools");
        }

        assert_eq!(
            skill(SkillNamingView::for_test(
                "private/skill",
                Some(view(
                    &cargo,
                    PackageNamingAuthority::BuiltinCargo,
                    PackageSourceClaim::CratesIo,
                )),
            )),
            ExtensionNaming::Unnamed
        );
    }

    #[test]
    fn recommendations_are_public_extensions_but_git_packages() {
        let id = PackageId::new("recommendations", "example-tools", "1.2.3");
        let package = view(
            &id,
            PackageNamingAuthority::Recommendations,
            PackageSourceClaim::Git,
        );

        assert_eq!(
            extension_source(package),
            Some(PublicExtensionSource::SymposiumRecommendations)
        );
        assert_eq!(
            super::package(package),
            PackageNaming::Unnamed(UnnamedPackageReason::Git)
        );
    }
}
