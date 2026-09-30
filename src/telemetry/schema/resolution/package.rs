//! Public package coordinates used by resolution telemetry.

use std::{fmt, str::FromStr};

use semver::Version;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use super::super::{
    EventId, RowKind, SchemaVersion, SymposiumVersion, UtcDay, deserialize_version_one,
    name::{InitialByteRule, validated_string_newtype},
};
use crate::telemetry::identity::{
    DimensionWriter, IdentityDimension, PackageDomain, PackageSubject,
};
use crate::telemetry::state::BoundRecordingObservation;

const MAX_PUBLIC_PACKAGE_NAME_BYTES: usize = 64;

/// Public package ecosystem approved for version 1 telemetry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::telemetry) enum PackageEcosystem {
    Cargo,
}

impl PackageEcosystem {
    #[must_use]
    const fn as_str(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
        }
    }
}

/// Kind of extension content contributed by one public package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::telemetry) enum ExtensionMatch {
    Public,
    UnnamedOnly,
    None,
}

validated_string_newtype! {
    /// Public package name accepted by the version 1 telemetry contract.
    pub(in crate::telemetry) struct PublicPackageName {
        error = PublicPackageNameError;
        maximum_bytes = MAX_PUBLIC_PACKAGE_NAME_BYTES;
        initial_byte_rule = InitialByteRule::Alphabetic;
        invalid_initial = NonAlphabeticFirstCharacter;
        noun = "public package name";
        as_str_doc = "Return the validated package name.";
    }
}

/// Exact semantic version attached to a public package coordinate.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::telemetry) struct ExactPackageVersion(Version);

impl ExactPackageVersion {
    /// Return the validated semantic version.
    #[must_use]
    pub(in crate::telemetry) fn as_version(&self) -> &Version {
        &self.0
    }
}

impl From<Version> for ExactPackageVersion {
    fn from(version: Version) -> Self {
        Self(version)
    }
}

impl FromStr for ExactPackageVersion {
    type Err = InvalidExactPackageVersion;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Version::parse(value)
            .map(Self)
            .map_err(|_| InvalidExactPackageVersion)
    }
}

impl TryFrom<String> for ExactPackageVersion {
    type Error = InvalidExactPackageVersion;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl fmt::Display for ExactPackageVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Serialize for ExactPackageVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ExactPackageVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .try_into()
            .map_err(D::Error::custom)
    }
}

/// Error returned when a package version is not an exact semantic version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) struct InvalidExactPackageVersion;

impl fmt::Display for InvalidExactPackageVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("package version must be an exact semantic version")
    }
}

impl std::error::Error for InvalidExactPackageVersion {}

/// Public package coordinate safe to place in telemetry.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::telemetry) struct PublicPackageCoordinate {
    ecosystem: PackageEcosystem,
    name: PublicPackageName,
    version: ExactPackageVersion,
}

impl PublicPackageCoordinate {
    /// Combine validated components into one public coordinate.
    ///
    /// The name and version must come from the package manager's resolved
    /// package identity. In particular, `name` must not be a dependency alias
    /// or the spelling from an unresolved request.
    #[must_use]
    pub(in crate::telemetry) fn new(
        ecosystem: PackageEcosystem,
        name: PublicPackageName,
        version: ExactPackageVersion,
    ) -> Self {
        Self {
            ecosystem,
            name,
            version,
        }
    }

    /// Validate raw resolved components and combine them into one coordinate.
    ///
    /// # Errors
    ///
    /// Returns an error when the package name is outside the version 1 grammar
    /// or the version is not an exact semantic version.
    pub(in crate::telemetry) fn try_new(
        ecosystem: PackageEcosystem,
        name: &str,
        version: &str,
    ) -> Result<Self, InvalidPublicPackageCoordinate> {
        Ok(Self::new(ecosystem, name.parse()?, version.parse()?))
    }

    /// Return the public ecosystem.
    #[must_use]
    pub(in crate::telemetry) fn ecosystem(&self) -> PackageEcosystem {
        self.ecosystem
    }

    /// Return the validated package name.
    #[must_use]
    pub(in crate::telemetry) fn name(&self) -> &PublicPackageName {
        &self.name
    }

    /// Return the exact package version.
    #[must_use]
    pub(in crate::telemetry) fn version(&self) -> &ExactPackageVersion {
        &self.version
    }

    /// Write package coordinate fields in version 1 identity order.
    pub(super) fn write_identity_fields(&self, writer: &mut DimensionWriter<'_>) {
        let version = self.version.to_string();
        writer.field(self.ecosystem.as_str().as_bytes());
        writer.field(self.name.as_str().as_bytes());
        writer.field(version.as_bytes());
    }
}

impl IdentityDimension for PublicPackageCoordinate {
    type Domain = PackageDomain;

    /// Write the version 1 `package_subject` fields in contract order.
    fn write(&self, writer: &mut DimensionWriter<'_>) {
        self.write_identity_fields(writer);
    }
}

/// Error returned when a public package coordinate has an invalid component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum InvalidPublicPackageCoordinate {
    Name(PublicPackageNameError),
    Version(InvalidExactPackageVersion),
}

impl From<PublicPackageNameError> for InvalidPublicPackageCoordinate {
    fn from(error: PublicPackageNameError) -> Self {
        Self::Name(error)
    }
}

impl From<InvalidExactPackageVersion> for InvalidPublicPackageCoordinate {
    fn from(error: InvalidExactPackageVersion) -> Self {
        Self::Version(error)
    }
}

impl fmt::Display for InvalidPublicPackageCoordinate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name(error) => write!(formatter, "invalid public package name: {error}"),
            Self::Version(error) => write!(formatter, "invalid public package version: {error}"),
        }
    }
}

impl std::error::Error for InvalidPublicPackageCoordinate {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Name(error) => Some(error),
            Self::Version(error) => Some(error),
        }
    }
}

/// Version 1 record of one eligible public package used during resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::telemetry) struct PackageResolutionV1 {
    #[serde(rename = "v", deserialize_with = "deserialize_version_one")]
    version: SchemaVersion,
    kind: RowKind,
    event_id: EventId,
    day: UtcDay,
    symposium: SymposiumVersion,
    package: PublicPackageCoordinate,
    extension_match: ExtensionMatch,
    package_subject: PackageSubject,
}

impl PackageResolutionV1 {
    /// Create a record for one eligible public resolution-input package.
    #[must_use]
    pub(in crate::telemetry) fn new(
        observation: &BoundRecordingObservation<'_>,
        package: PublicPackageCoordinate,
        extension_match: ExtensionMatch,
    ) -> Self {
        let package_subject = observation.identifier_window_scope().derive(&package);

        Self {
            version: SchemaVersion::V1,
            kind: RowKind::PackageResolution,
            event_id: EventId::new(),
            day: observation.day(),
            symposium: SymposiumVersion::current(),
            package,
            extension_match,
            package_subject,
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::super::super::{
        IDENTIFIER_WINDOW_TEST_STATE, assert_contract_names, assert_contract_names_with_labels,
        recording_observation,
    };
    use super::*;
    use crate::telemetry::{identity::encode_dimension_for_test, state::TelemetryStateV1};

    fn package_name(value: &str) -> PublicPackageName {
        value.parse().unwrap()
    }

    fn package_version(value: &str) -> ExactPackageVersion {
        value.parse().unwrap()
    }

    fn package_resolution() -> PackageResolutionV1 {
        package_resolution_for("example-runtime")
    }

    fn package_resolution_for(package_name: &str) -> PackageResolutionV1 {
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let observation = recording_observation(&mut state);

        PackageResolutionV1::new(
            &observation,
            PublicPackageCoordinate::try_new(PackageEcosystem::Cargo, package_name, "1.2.3")
                .unwrap(),
            ExtensionMatch::Public,
        )
    }

    #[test]
    fn package_ecosystems_round_trip_with_contract_names() {
        let cases = [(PackageEcosystem::Cargo, "cargo")];

        assert_contract_names_with_labels(&cases, PackageEcosystem::as_str);
    }

    #[test]
    fn extension_matches_round_trip_with_contract_names() {
        let cases = [
            (ExtensionMatch::Public, "public"),
            (ExtensionMatch::UnnamedOnly, "unnamed_only"),
            (ExtensionMatch::None, "none"),
        ];

        assert_contract_names(&cases);
    }

    #[test]
    fn package_vocabulary_rejects_unknown_contract_names() {
        let unknown = r#""future_value""#;

        let ecosystem = serde_json::from_str::<PackageEcosystem>(unknown);
        let extension_match = serde_json::from_str::<ExtensionMatch>(unknown);

        assert!(ecosystem.is_err());
        assert!(extension_match.is_err());
    }

    #[test]
    fn public_package_names_accept_the_contract_grammar() {
        let cases = ["a", "A1", "example-runtime", "example_runtime"];

        for value in cases {
            let name = value.parse::<PublicPackageName>().unwrap();
            let json = serde_json::to_string(&name).unwrap();
            let decoded = serde_json::from_str::<PublicPackageName>(&json).unwrap();

            assert_eq!(name.as_str(), value);
            assert_eq!(json, format!(r#""{value}""#));
            assert_eq!(decoded, name);
        }

        let maximum_length = format!("a{}", "0".repeat(63));
        assert_eq!(
            maximum_length
                .parse::<PublicPackageName>()
                .unwrap()
                .as_str(),
            maximum_length
        );
    }

    #[test]
    fn public_package_names_reject_invalid_length() {
        let too_long = format!("a{}", "0".repeat(64));

        let empty = "".parse::<PublicPackageName>();
        let oversized = too_long.parse::<PublicPackageName>();

        assert_eq!(empty, Err(PublicPackageNameError::Empty));
        assert_eq!(oversized, Err(PublicPackageNameError::TooLong));
    }

    #[test]
    fn public_package_names_require_an_ascii_letter_first() {
        let cases = ["1crate", "-crate", "_crate", "écrate"];

        for value in cases {
            assert_eq!(
                value.parse::<PublicPackageName>(),
                Err(PublicPackageNameError::NonAlphabeticFirstCharacter)
            );
        }
    }

    #[test]
    fn public_package_names_reject_unsupported_characters() {
        let cases = ["crate.name", "crate/name", "crate name", "craté"];

        for value in cases {
            assert_eq!(
                value.parse::<PublicPackageName>(),
                Err(PublicPackageNameError::UnsupportedCharacter)
            );
        }
    }

    #[test]
    fn exact_package_versions_round_trip_without_losing_semver_parts() {
        let cases = ["1.2.3", "1.2.3-alpha.1+build.5"];

        for value in cases {
            let version = value.parse::<ExactPackageVersion>().unwrap();
            let json = serde_json::to_string(&version).unwrap();
            let decoded = serde_json::from_str::<ExactPackageVersion>(&json).unwrap();

            assert_eq!(version.to_string(), value);
            assert_eq!(json, format!(r#""{value}""#));
            assert_eq!(decoded, version);
        }
    }

    #[test]
    fn exact_package_versions_reject_missing_ranges_and_wildcards() {
        let cases = ["", "*", "^1.2.3", "1.2", "01.2.3", "1.2.3.4"];

        for value in cases {
            assert_eq!(
                value.parse::<ExactPackageVersion>(),
                Err(InvalidExactPackageVersion)
            );
        }
    }

    #[test]
    fn public_package_coordinate_validates_raw_components() {
        let coordinate =
            PublicPackageCoordinate::try_new(PackageEcosystem::Cargo, "example-runtime", "1.2.3")
                .unwrap();
        let invalid_name =
            PublicPackageCoordinate::try_new(PackageEcosystem::Cargo, "private/package", "1.2.3");
        let invalid_version =
            PublicPackageCoordinate::try_new(PackageEcosystem::Cargo, "example-runtime", "*");

        assert_eq!(coordinate.name().as_str(), "example-runtime");
        assert_eq!(coordinate.version().to_string(), "1.2.3");
        assert_eq!(
            invalid_name,
            Err(InvalidPublicPackageCoordinate::Name(
                PublicPackageNameError::UnsupportedCharacter
            ))
        );
        assert_eq!(
            invalid_version,
            Err(InvalidPublicPackageCoordinate::Version(
                InvalidExactPackageVersion
            ))
        );
    }

    #[test]
    fn package_subject_dimension_uses_contract_field_order() {
        let coordinate = PublicPackageCoordinate::try_new(
            PackageEcosystem::Cargo,
            "example-runtime",
            "1.2.3-alpha.1+build.5",
        )
        .unwrap();
        let ecosystem_length = 5_u64.to_be_bytes();
        let name_length = 15_u64.to_be_bytes();
        let version_length = 21_u64.to_be_bytes();
        let expected = [
            ecosystem_length.as_slice(),
            b"cargo".as_slice(),
            name_length.as_slice(),
            b"example-runtime".as_slice(),
            version_length.as_slice(),
            b"1.2.3-alpha.1+build.5".as_slice(),
        ]
        .concat();

        let encoded = encode_dimension_for_test(&coordinate);

        assert_eq!(encoded, expected);
    }

    #[test]
    fn public_package_coordinate_round_trips_in_contract_order() {
        let coordinate = PublicPackageCoordinate::new(
            PackageEcosystem::Cargo,
            package_name("Example-runtime"),
            package_version("1.2.3-alpha.1+build.5"),
        );

        let json = serde_json::to_string(&coordinate).unwrap();
        let decoded = serde_json::from_str::<PublicPackageCoordinate>(&json).unwrap();

        assert_eq!(
            json,
            r#"{"ecosystem":"cargo","name":"Example-runtime","version":"1.2.3-alpha.1+build.5"}"#
        );
        assert_eq!(decoded, coordinate);
        assert_eq!(coordinate.ecosystem(), PackageEcosystem::Cargo);
        assert_eq!(coordinate.name().as_str(), "Example-runtime");
        assert_eq!(
            coordinate.version().as_version(),
            &Version::parse("1.2.3-alpha.1+build.5").unwrap()
        );
    }

    #[test]
    fn public_package_coordinate_rejects_unknown_fields() {
        let json = r#"{"ecosystem":"cargo","name":"example-runtime","version":"1.2.3","source":"registry"}"#;

        let result = serde_json::from_str::<PublicPackageCoordinate>(json);

        assert!(result.is_err());
    }

    #[test]
    fn public_package_coordinate_validates_nested_name_and_version() {
        let invalid_name = r#"{"ecosystem":"cargo","name":"private/package","version":"1.2.3"}"#;
        let invalid_version = r#"{"ecosystem":"cargo","name":"example-runtime","version":"*"}"#;

        let name_result = serde_json::from_str::<PublicPackageCoordinate>(invalid_name);
        let version_result = serde_json::from_str::<PublicPackageCoordinate>(invalid_version);

        assert!(name_result.is_err());
        assert!(version_result.is_err());
    }

    #[test]
    fn new_package_resolution_derives_subject_from_its_coordinate() {
        let expected_day = UtcDay::from_date(NaiveDate::from_ymd_opt(2026, 8, 3).unwrap());
        // Cross-checked with .NET's HMACSHA256 over the contract header,
        // identifier window, ecosystem, published name, and exact version. The
        // complete digest is a7907f7a5ae0de9ae55469f276ba73b2fe68e9b97c4bdd1867dd0685acd297ee.
        let expected_subject = "pkg_a7907f7a5ae0de9ae55469f276ba73b2".parse().unwrap();

        let row = package_resolution();

        assert_eq!(row.version, SchemaVersion::V1);
        assert_eq!(row.kind, RowKind::PackageResolution);
        assert_eq!(row.event_id.0.get_version(), Some(uuid::Version::Random));
        assert_eq!(row.day, expected_day);
        assert_eq!(row.symposium, SymposiumVersion::current());
        assert_eq!(row.package.ecosystem(), PackageEcosystem::Cargo);
        assert_eq!(row.package.name().as_str(), "example-runtime");
        assert_eq!(row.package.version().to_string(), "1.2.3");
        assert_eq!(row.extension_match, ExtensionMatch::Public);
        assert_eq!(row.package_subject, expected_subject);
    }

    #[test]
    fn package_subject_changes_with_the_source_coordinate() {
        let first = package_resolution_for("example-runtime");
        let second = package_resolution_for("example-tools");

        assert_ne!(first.package_subject, second.package_subject);
    }

    #[test]
    fn package_resolution_rejects_future_version() {
        let json = serde_json::to_string(&package_resolution()).unwrap();
        let future = json.replacen(r#""v":1"#, r#""v":2"#, 1);

        let result = serde_json::from_str::<PackageResolutionV1>(&future);

        assert!(result.is_err());
    }

    #[test]
    fn package_resolution_rejects_unknown_fields() {
        let json = serde_json::to_string(&package_resolution()).unwrap();
        let unknown = json.replacen(r#""package""#, r#""future_field":true,"package""#, 1);

        let result = serde_json::from_str::<PackageResolutionV1>(&unknown);

        assert!(result.is_err());
    }

    #[test]
    fn package_resolution_requires_every_field() {
        let json = serde_json::to_string(&package_resolution()).unwrap();
        let missing = json.replacen(r#","extension_match":"public""#, "", 1);

        let result = serde_json::from_str::<PackageResolutionV1>(&missing);

        assert!(result.is_err());
    }
}
