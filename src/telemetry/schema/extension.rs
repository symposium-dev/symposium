//! Public extension vocabulary shared by telemetry rows.

use serde::{Deserialize, Serialize};

use super::name::{InitialByteRule, validated_string_newtype};

const MAX_PUBLIC_EXTENSION_NAME_BYTES: usize = 64;

/// Plugin or skill named by eligible public telemetry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::telemetry) enum ExtensionKind {
    Plugin,
    Skill,
}

impl ExtensionKind {
    /// Return the frozen version 1 wire label.
    #[must_use]
    pub(in crate::telemetry) const fn as_str(self) -> &'static str {
        match self {
            Self::Plugin => "plugin",
            Self::Skill => "skill",
        }
    }
}

/// Public source approved for version 1 extension telemetry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(in crate::telemetry) enum PublicExtensionSource {
    SymposiumRecommendations,
    CratesIo,
}

impl PublicExtensionSource {
    /// Return the frozen version 1 wire label.
    #[must_use]
    pub(in crate::telemetry) const fn as_str(self) -> &'static str {
        match self {
            Self::SymposiumRecommendations => "symposium-recommendations",
            Self::CratesIo => "crates-io",
        }
    }
}

validated_string_newtype! {
    /// Public plugin or skill name accepted by the version 1 telemetry contract.
    pub(in crate::telemetry) struct PublicExtensionName {
        error = PublicExtensionNameError;
        maximum_bytes = MAX_PUBLIC_EXTENSION_NAME_BYTES;
        initial_byte_rule = InitialByteRule::Alphanumeric;
        invalid_initial = NonAlphanumericFirstCharacter;
        noun = "public extension name";
        as_str_doc = "Return the validated extension name without changing its spelling.";
    }
}

/// Public plugin or skill coordinate safe to place in telemetry.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::telemetry) struct PublicExtensionCoordinate {
    #[serde(rename = "type")]
    kind: ExtensionKind,
    source: PublicExtensionSource,
    name: PublicExtensionName,
}

impl PublicExtensionCoordinate {
    /// Combine validated components into one public extension coordinate.
    #[must_use]
    pub(in crate::telemetry) const fn new(
        kind: ExtensionKind,
        source: PublicExtensionSource,
        name: PublicExtensionName,
    ) -> Self {
        Self { kind, source, name }
    }

    /// Validate a raw public name and combine it with its typed coordinate.
    ///
    /// # Errors
    ///
    /// Returns an error when `name` is outside the version 1 public extension
    /// grammar.
    pub(in crate::telemetry) fn try_new(
        kind: ExtensionKind,
        source: PublicExtensionSource,
        name: &str,
    ) -> Result<Self, PublicExtensionNameError> {
        Ok(Self::new(kind, source, name.parse()?))
    }

    /// Return the public plugin or skill kind.
    #[must_use]
    pub(in crate::telemetry) const fn kind(&self) -> ExtensionKind {
        self.kind
    }

    /// Return the allowlisted public source.
    #[must_use]
    pub(in crate::telemetry) const fn source(&self) -> PublicExtensionSource {
        self.source
    }

    /// Return the validated public extension name.
    #[must_use]
    pub(in crate::telemetry) const fn name(&self) -> &PublicExtensionName {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::super::assert_contract_names_with_labels;
    use super::*;

    #[test]
    fn extension_kinds_round_trip_with_contract_names() {
        let cases = [
            (ExtensionKind::Plugin, "plugin"),
            (ExtensionKind::Skill, "skill"),
        ];

        assert_contract_names_with_labels(&cases, ExtensionKind::as_str);
    }

    #[test]
    fn public_extension_sources_round_trip_with_contract_names() {
        let cases = [
            (
                PublicExtensionSource::SymposiumRecommendations,
                "symposium-recommendations",
            ),
            (PublicExtensionSource::CratesIo, "crates-io"),
        ];

        assert_contract_names_with_labels(&cases, PublicExtensionSource::as_str);
    }

    #[test]
    fn extension_vocabulary_rejects_unknown_contract_names() {
        let kind = serde_json::from_str::<ExtensionKind>(r#""command""#);
        let source = serde_json::from_str::<PublicExtensionSource>(r#""user-plugins""#);

        assert!(kind.is_err());
        assert!(source.is_err());
    }

    #[test]
    fn public_extension_names_accept_the_contract_grammar() {
        for value in ["0", "Example-runtime_2", &"a".repeat(64)] {
            let name = value.parse::<PublicExtensionName>().unwrap();

            assert_eq!(name.as_str(), value);
        }
    }

    #[test]
    fn public_extension_names_reject_invalid_length() {
        let empty = "".parse::<PublicExtensionName>();
        let too_long = "a".repeat(65).parse::<PublicExtensionName>();

        assert_eq!(empty.unwrap_err(), PublicExtensionNameError::Empty);
        assert_eq!(too_long.unwrap_err(), PublicExtensionNameError::TooLong);
    }

    #[test]
    fn public_extension_names_require_an_ascii_alphanumeric_first_byte() {
        for value in ["-extension", "_extension", "\u{e9}xtension"] {
            let result = value.parse::<PublicExtensionName>();

            assert_eq!(
                result.unwrap_err(),
                PublicExtensionNameError::NonAlphanumericFirstCharacter
            );
        }
    }

    #[test]
    fn public_extension_names_reject_unsupported_characters() {
        for value in [
            "extension.name",
            "extension name",
            "extension/name",
            "a\u{e9}",
        ] {
            let result = value.parse::<PublicExtensionName>();

            assert_eq!(
                result.unwrap_err(),
                PublicExtensionNameError::UnsupportedCharacter
            );
        }
    }

    #[test]
    fn public_extension_names_round_trip_without_normalization() {
        let name = "Example-runtime_2".parse::<PublicExtensionName>().unwrap();

        let encoded = serde_json::to_string(&name).unwrap();
        let decoded = serde_json::from_str::<PublicExtensionName>(&encoded).unwrap();

        assert_eq!(encoded, r#""Example-runtime_2""#);
        assert_eq!(decoded, name);
    }

    #[test]
    fn public_extension_name_validation_runs_during_deserialization() {
        let invalid = serde_json::from_str::<PublicExtensionName>(r#""extension.name""#);

        assert!(invalid.is_err());
    }

    #[test]
    fn public_extension_coordinate_round_trips_in_contract_order() {
        let coordinate = PublicExtensionCoordinate::try_new(
            ExtensionKind::Skill,
            PublicExtensionSource::SymposiumRecommendations,
            "Example-debugging_2",
        )
        .unwrap();

        let json = serde_json::to_string(&coordinate).unwrap();
        let decoded = serde_json::from_str::<PublicExtensionCoordinate>(&json).unwrap();

        assert_eq!(
            json,
            r#"{"type":"skill","source":"symposium-recommendations","name":"Example-debugging_2"}"#
        );
        assert_eq!(decoded, coordinate);
    }

    #[test]
    fn public_extension_coordinate_exposes_its_validated_components() {
        let name = "example-tools".parse::<PublicExtensionName>().unwrap();
        let coordinate = PublicExtensionCoordinate::new(
            ExtensionKind::Plugin,
            PublicExtensionSource::CratesIo,
            name.clone(),
        );

        assert_eq!(coordinate.kind(), ExtensionKind::Plugin);
        assert_eq!(coordinate.source(), PublicExtensionSource::CratesIo);
        assert_eq!(coordinate.name(), &name);
    }

    #[test]
    fn public_extension_coordinate_rejects_unknown_fields() {
        let json =
            r#"{"type":"plugin","source":"crates-io","name":"example-tools","path":"private"}"#;

        let result = serde_json::from_str::<PublicExtensionCoordinate>(json);

        assert!(result.is_err());
    }

    #[test]
    fn public_extension_coordinate_requires_every_field() {
        let json = r#"{"type":"plugin","source":"crates-io"}"#;

        let result = serde_json::from_str::<PublicExtensionCoordinate>(json);

        assert!(result.is_err());
    }

    #[test]
    fn public_extension_coordinate_validates_its_nested_name() {
        let raw_result = PublicExtensionCoordinate::try_new(
            ExtensionKind::Plugin,
            PublicExtensionSource::CratesIo,
            "private/plugin",
        );
        let json_result = serde_json::from_str::<PublicExtensionCoordinate>(
            r#"{"type":"plugin","source":"crates-io","name":"private/plugin"}"#,
        );

        assert_eq!(
            raw_result.unwrap_err(),
            PublicExtensionNameError::UnsupportedCharacter
        );
        assert!(json_result.is_err());
    }
}
