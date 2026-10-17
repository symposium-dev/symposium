//! Public naming policy for package and extension telemetry.
//!
//! Package managers report source facts; this module alone decides whether
//! those facts may become a frozen public telemetry label. Raw provenance and
//! URLs never cross into a row. Source rejection takes precedence over
//! coordinate validation, so malformed private coordinates retain their
//! source reason instead of becoming `invalid_coordinate`.
//!
//! Packages retain one unnamed reason because resolution summaries count the
//! mutually exclusive source and coordinate failures. Extensions do not: the
//! invocation vocabulary also covers indexing and signal failures that this
//! source policy cannot observe, so this layer reports only public or unnamed.
//! The recommendations Git source is consequently public for extensions but
//! remains `git` for package-resolution accounting.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "resolution and extension producers consume this policy in later slices"
    )
)]

use crate::{
    config::BUILTIN_RECOMMENDATIONS_URL,
    pm::{CARGO_PM, CRATES_IO_REGISTRY_URL, PackageProvenance, SourcedPackage},
};

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

/// Select the frozen public extension source for one provenance fact.
///
/// Commands use this source decision directly because their coordinate carries
/// both a plugin name and a command name. Plugin and skill naming goes through
/// [`extension`], which also validates the extension name.
/// Unlike package naming, this label makes no package-ecosystem claim, so it
/// deliberately does not inspect the package-manager routing name.
#[must_use]
pub(in crate::telemetry) fn extension_source(
    provenance: &PackageProvenance,
) -> Option<PublicExtensionSource> {
    match provenance {
        PackageProvenance::Registry { url } if url == CRATES_IO_REGISTRY_URL => {
            Some(PublicExtensionSource::CratesIo)
        }
        PackageProvenance::Git { url } if url == BUILTIN_RECOMMENDATIONS_URL => {
            Some(PublicExtensionSource::SymposiumRecommendations)
        }
        PackageProvenance::Registry { .. }
        | PackageProvenance::Git { .. }
        | PackageProvenance::Path
        | PackageProvenance::Workspace
        | PackageProvenance::Unknown => None,
    }
}

/// Apply the version 1 public package policy to one sourced package.
#[must_use]
pub(in crate::telemetry) fn package(package: &SourcedPackage) -> PackageNaming {
    let ecosystem = match &package.provenance {
        PackageProvenance::Registry { url }
            if url == CRATES_IO_REGISTRY_URL && package.id.pm == CARGO_PM =>
        {
            PackageEcosystem::Cargo
        }
        PackageProvenance::Registry { url } if url == CRATES_IO_REGISTRY_URL => {
            // The registry is recognized, but an unfamiliar PM cannot justify
            // the Cargo ecosystem. Version 1 has no ecosystem-mismatch reason,
            // and `private_registry` would make a false source claim.
            return PackageNaming::Unnamed(UnnamedPackageReason::UnknownSource);
        }
        PackageProvenance::Registry { .. } => {
            return PackageNaming::Unnamed(UnnamedPackageReason::PrivateRegistry);
        }
        PackageProvenance::Git { .. } => {
            return PackageNaming::Unnamed(UnnamedPackageReason::Git);
        }
        PackageProvenance::Path => {
            return PackageNaming::Unnamed(UnnamedPackageReason::Path);
        }
        PackageProvenance::Workspace => {
            return PackageNaming::Unnamed(UnnamedPackageReason::Workspace);
        }
        PackageProvenance::Unknown => {
            return PackageNaming::Unnamed(UnnamedPackageReason::UnknownSource);
        }
    };

    match PublicPackageCoordinate::try_new(ecosystem, &package.id.name, &package.id.version) {
        Ok(coordinate) => PackageNaming::Public(coordinate),
        Err(_) => PackageNaming::Unnamed(UnnamedPackageReason::InvalidCoordinate),
    }
}

/// Apply the version 1 public extension policy to one plugin or skill name.
#[must_use]
pub(in crate::telemetry) fn extension(
    provenance: &PackageProvenance,
    kind: ExtensionKind,
    name: &str,
) -> ExtensionNaming {
    let Some(source) = extension_source(provenance) else {
        return ExtensionNaming::Unnamed;
    };

    match PublicExtensionCoordinate::try_new(kind, source, name) {
        Ok(coordinate) => ExtensionNaming::Public(coordinate),
        Err(_) => ExtensionNaming::Unnamed,
    }
}

#[cfg(test)]
mod tests {
    use cargo_metadata::Source;
    use symposium_sdk::pm::PackageId;

    use super::*;
    use crate::pm::classify_package_source;

    fn sourced(
        pm: &str,
        name: &str,
        version: &str,
        provenance: PackageProvenance,
    ) -> SourcedPackage {
        SourcedPackage::new(PackageId::new(pm, name, version), provenance)
    }

    fn crates_io() -> PackageProvenance {
        PackageProvenance::Registry {
            url: CRATES_IO_REGISTRY_URL.to_owned(),
        }
    }

    fn recommendations() -> PackageProvenance {
        PackageProvenance::Git {
            url: BUILTIN_RECOMMENDATIONS_URL.to_owned(),
        }
    }

    #[test]
    fn crates_io_cargo_packages_receive_public_coordinates() {
        let decision = package(&sourced(CARGO_PM, "example-tools", "1.2.3", crates_io()));

        let PackageNaming::Public(coordinate) = decision else {
            panic!("expected a public package coordinate");
        };
        assert_eq!(coordinate.ecosystem(), PackageEcosystem::Cargo);
        assert_eq!(coordinate.name().as_str(), "example-tools");
        assert_eq!(coordinate.version().to_string(), "1.2.3");
    }

    #[test]
    fn public_package_coordinates_require_valid_names_and_exact_versions() {
        for candidate in [
            sourced(CARGO_PM, "private/package", "1.2.3", crates_io()),
            sourced(CARGO_PM, "example-tools", "*", crates_io()),
        ] {
            assert_eq!(
                package(&candidate),
                PackageNaming::Unnamed(UnnamedPackageReason::InvalidCoordinate)
            );
        }
    }

    #[test]
    fn package_source_reasons_take_precedence_over_invalid_coordinates() {
        let cases = [
            (
                PackageProvenance::Registry {
                    url: "https://registry.example.invalid/index".to_owned(),
                },
                UnnamedPackageReason::PrivateRegistry,
            ),
            (
                PackageProvenance::Git {
                    url: BUILTIN_RECOMMENDATIONS_URL.to_owned(),
                },
                UnnamedPackageReason::Git,
            ),
            (PackageProvenance::Path, UnnamedPackageReason::Path),
            (
                PackageProvenance::Workspace,
                UnnamedPackageReason::Workspace,
            ),
            (
                PackageProvenance::Unknown,
                UnnamedPackageReason::UnknownSource,
            ),
        ];

        for (provenance, expected) in cases {
            let invalid = sourced(CARGO_PM, "private/package", "*", provenance);
            assert_eq!(package(&invalid), PackageNaming::Unnamed(expected));
        }
    }

    #[test]
    fn crates_io_provenance_requires_the_cargo_package_manager() {
        let candidate = sourced("other", "private/package", "*", crates_io());

        assert_eq!(
            package(&candidate),
            PackageNaming::Unnamed(UnnamedPackageReason::UnknownSource)
        );
    }

    #[test]
    fn extension_sources_are_allowlisted_by_exact_provenance() {
        let cases = [
            (crates_io(), Some(PublicExtensionSource::CratesIo)),
            (
                recommendations(),
                Some(PublicExtensionSource::SymposiumRecommendations),
            ),
            (
                PackageProvenance::Registry {
                    url: format!("{CRATES_IO_REGISTRY_URL}/"),
                },
                None,
            ),
            (
                PackageProvenance::Git {
                    url: format!("{BUILTIN_RECOMMENDATIONS_URL}/"),
                },
                None,
            ),
            (PackageProvenance::Path, None),
            (PackageProvenance::Workspace, None),
            (PackageProvenance::Unknown, None),
        ];

        for (provenance, expected) in cases {
            assert_eq!(extension_source(&provenance), expected);
        }
    }

    #[test]
    fn allowlisted_sources_name_plugins_and_skills() {
        let cases = [
            (
                crates_io(),
                ExtensionKind::Plugin,
                PublicExtensionSource::CratesIo,
            ),
            (
                recommendations(),
                ExtensionKind::Skill,
                PublicExtensionSource::SymposiumRecommendations,
            ),
        ];

        for (provenance, kind, expected_source) in cases {
            let ExtensionNaming::Public(coordinate) = extension(&provenance, kind, "example-tools")
            else {
                panic!("expected a public extension coordinate");
            };
            assert_eq!(coordinate.kind(), kind);
            assert_eq!(coordinate.source(), expected_source);
            assert_eq!(coordinate.name().as_str(), "example-tools");
        }
    }

    #[test]
    fn invalid_or_private_extension_names_remain_unnamed() {
        let cases = [
            (crates_io(), "private/plugin"),
            (PackageProvenance::Path, "valid-name"),
            (
                PackageProvenance::Registry {
                    url: "https://registry.example.invalid/index".to_owned(),
                },
                "valid-name",
            ),
        ];

        for (provenance, name) in cases {
            assert_eq!(
                extension(&provenance, ExtensionKind::Plugin, name),
                ExtensionNaming::Unnamed
            );
        }
    }

    #[test]
    fn recommendations_are_public_extensions_but_unnamed_packages() {
        assert_eq!(
            extension_source(&recommendations()),
            Some(PublicExtensionSource::SymposiumRecommendations)
        );
        assert_eq!(
            package(&sourced(
                "symposium-recommendations",
                "example-tools",
                "*",
                recommendations(),
            )),
            PackageNaming::Unnamed(UnnamedPackageReason::Git)
        );
    }

    #[tokio::test]
    async fn builtin_user_plugins_remain_unnamed() {
        let temp = tempfile::tempdir().unwrap();
        let entry = temp.path().join("plugins/example-tools");
        std::fs::create_dir_all(&entry).unwrap();
        std::fs::write(entry.join("Symposium.toml"), "name = \"example-tools\"").unwrap();
        let symposium = crate::config::Symposium::from_dir(temp.path());
        let user_plugins = symposium
            .registry_instances()
            .into_iter()
            .find(|instance| instance.name == "user-plugins")
            .expect("the default config includes the builtin user-plugins registry");
        let offered = user_plugins.pm.active_plugins(&[]).await;
        let plugin = offered
            .first()
            .expect("the builtin user-plugins registry should offer the fixture");

        assert_eq!(plugin.package.id.pm, "user-plugins");
        assert_eq!(plugin.package.provenance, PackageProvenance::Path);
        assert_eq!(extension_source(&plugin.package.provenance), None);
        assert_eq!(
            extension(
                &plugin.package.provenance,
                ExtensionKind::Plugin,
                "example-tools",
            ),
            ExtensionNaming::Unnamed
        );
    }

    #[test]
    fn cargo_classifier_output_flows_into_the_public_policy() {
        let source = Source {
            repr: "sparse+https://index.crates.io/".to_owned(),
        };
        let provenance = classify_package_source(Some(&source), false);

        assert_eq!(
            provenance,
            PackageProvenance::Registry {
                url: CRATES_IO_REGISTRY_URL.to_owned()
            }
        );
        assert_eq!(
            extension_source(&provenance),
            Some(PublicExtensionSource::CratesIo)
        );
        assert!(matches!(
            package(&sourced(CARGO_PM, "example-tools", "1.2.3", provenance)),
            PackageNaming::Public(_)
        ));
    }
}
