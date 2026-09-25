//! Plugin-hook public identity, bounded buckets, and aggregate keys.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::super::extension::{
    ExtensionKind, PublicExtensionCoordinate, PublicExtensionName, PublicExtensionNameError,
    PublicExtensionSource,
};
use crate::telemetry::identity::{DimensionWriter, IdentityDimension, PluginDomain};
/// Identity exposure assigned to one plugin-hook aggregate bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::telemetry) enum PluginScope {
    Public,
    Unnamed,
    Overflow,
}

impl PluginScope {
    /// Return the frozen version 1 wire label.
    #[must_use]
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Unnamed => "unnamed",
            Self::Overflow => "overflow",
        }
    }
}

/// Public plugin coordinate safe to place in plugin-hook telemetry.
///
/// The enclosing row establishes that this coordinate identifies a plugin,
/// so its wire form contains only the reviewed public source and validated
/// name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::telemetry) struct PublicPluginCoordinate {
    source: PublicExtensionSource,
    name: PublicExtensionName,
}

impl PublicPluginCoordinate {
    /// Combine validated components into one public plugin coordinate.
    #[must_use]
    pub(in crate::telemetry) const fn new(
        source: PublicExtensionSource,
        name: PublicExtensionName,
    ) -> Self {
        Self { source, name }
    }

    /// Validate a raw public name and combine it with an allowlisted source.
    ///
    /// # Errors
    ///
    /// Returns an error when `name` is outside the version 1 public extension
    /// grammar.
    pub(in crate::telemetry) fn try_new(
        source: PublicExtensionSource,
        name: &str,
    ) -> Result<Self, PublicExtensionNameError> {
        Ok(Self::new(source, name.parse()?))
    }

    /// Return the reviewed public source.
    #[must_use]
    pub(in crate::telemetry) const fn source(&self) -> PublicExtensionSource {
        self.source
    }

    /// Return the validated public plugin name.
    #[must_use]
    pub(in crate::telemetry) const fn name(&self) -> &PublicExtensionName {
        &self.name
    }
}

impl TryFrom<&PublicExtensionCoordinate> for PublicPluginCoordinate {
    type Error = NotPublicPlugin;

    fn try_from(coordinate: &PublicExtensionCoordinate) -> Result<Self, Self::Error> {
        if coordinate.kind() != ExtensionKind::Plugin {
            return Err(NotPublicPlugin {
                found: coordinate.kind(),
            });
        }

        Ok(Self::new(coordinate.source(), coordinate.name().clone()))
    }
}

impl IdentityDimension for PublicPluginCoordinate {
    type Domain = PluginDomain;

    /// Write public source and plugin name in version 1 contract order.
    fn write(&self, writer: &mut DimensionWriter<'_>) {
        writer.field(self.source.as_str().as_bytes());
        writer.field(self.name.as_str().as_bytes());
    }
}

/// A public extension coordinate that identifies something other than a plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) struct NotPublicPlugin {
    found: ExtensionKind,
}

impl fmt::Display for NotPublicPlugin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "expected a public plugin coordinate, found {}",
            self.found.as_str()
        )
    }
}

impl std::error::Error for NotPublicPlugin {}

/// Plugin identity observed before private-state admission.
///
/// Overflow is deliberately absent. Only the private aggregate store may
/// assign a public observation to the bounded overflow row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::telemetry) enum PluginHookAttribution {
    Public(PublicPluginCoordinate),
    Unnamed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::{
        identity::{PluginSubject, encode_dimension_for_test},
        schema::{
            IDENTIFIER_WINDOW_TEST_STATE, assert_contract_names_with_labels, recording_observation,
        },
        state::TelemetryStateV1,
    };
    fn public_extension(
        kind: ExtensionKind,
        source: PublicExtensionSource,
        name: &str,
    ) -> PublicExtensionCoordinate {
        PublicExtensionCoordinate::try_new(kind, source, name).unwrap()
    }

    fn public_plugin(source: PublicExtensionSource, name: &str) -> PublicPluginCoordinate {
        PublicPluginCoordinate::try_new(source, name).unwrap()
    }

    fn plugin_subject(coordinate: &PublicPluginCoordinate) -> PluginSubject {
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let recording = recording_observation(&mut state);

        recording.identifier_window_scope().derive(coordinate)
    }

    #[test]
    fn plugin_scopes_round_trip_with_contract_names() {
        assert_contract_names_with_labels(
            &[
                (PluginScope::Public, "public"),
                (PluginScope::Unnamed, "unnamed"),
                (PluginScope::Overflow, "overflow"),
            ],
            PluginScope::as_str,
        );
    }

    #[test]
    fn plugin_hook_vocabulary_rejects_unknown_contract_names() {
        let scope = serde_json::from_str::<PluginScope>(r#""private""#);

        assert!(scope.is_err());
    }

    #[test]
    fn public_plugin_coordinate_is_derived_from_a_validated_plugin() {
        let extension = public_extension(
            ExtensionKind::Plugin,
            PublicExtensionSource::SymposiumRecommendations,
            "Example-tools_2",
        );

        let coordinate = PublicPluginCoordinate::try_from(&extension).unwrap();
        let json = serde_json::to_string(&coordinate).unwrap();
        let decoded = serde_json::from_str::<PublicPluginCoordinate>(&json).unwrap();

        assert_eq!(
            json,
            r#"{"source":"symposium-recommendations","name":"Example-tools_2"}"#
        );
        assert_eq!(decoded, coordinate);
        assert_eq!(
            coordinate.source(),
            PublicExtensionSource::SymposiumRecommendations
        );
        assert_eq!(coordinate.name().as_str(), "Example-tools_2");
    }

    #[test]
    fn public_plugin_coordinate_rejects_a_skill_coordinate() {
        let skill = public_extension(
            ExtensionKind::Skill,
            PublicExtensionSource::SymposiumRecommendations,
            "example-debugging",
        );

        let result = PublicPluginCoordinate::try_from(&skill);

        assert_eq!(
            result,
            Err(NotPublicPlugin {
                found: ExtensionKind::Skill,
            })
        );
    }

    #[test]
    fn public_plugin_coordinate_rejects_invalid_or_unknown_fields() {
        let invalid_raw =
            PublicPluginCoordinate::try_new(PublicExtensionSource::CratesIo, "example.tools");
        let invalid_name = serde_json::from_str::<PublicPluginCoordinate>(
            r#"{"source":"crates-io","name":"example.tools"}"#,
        );
        let unknown_field = serde_json::from_str::<PublicPluginCoordinate>(
            r#"{"source":"crates-io","name":"example-tools","type":"plugin"}"#,
        );
        let missing_field =
            serde_json::from_str::<PublicPluginCoordinate>(r#"{"source":"crates-io"}"#);

        assert_eq!(
            invalid_raw,
            Err(PublicExtensionNameError::UnsupportedCharacter)
        );
        assert!(invalid_name.is_err());
        assert!(unknown_field.is_err());
        assert!(missing_field.is_err());
    }

    #[test]
    fn plugin_subject_dimension_uses_source_then_name() {
        let coordinate = public_plugin(
            PublicExtensionSource::SymposiumRecommendations,
            "example-tools",
        );
        let expected = [
            [0, 0, 0, 0, 0, 0, 0, 25].as_slice(),
            b"symposium-recommendations".as_slice(),
            [0, 0, 0, 0, 0, 0, 0, 13].as_slice(),
            b"example-tools".as_slice(),
        ]
        .concat();

        let encoded = encode_dimension_for_test(&coordinate);

        assert_eq!(encoded, expected);
    }

    #[test]
    fn plugin_subject_derivation_matches_independent_vector() {
        let coordinate = public_plugin(
            PublicExtensionSource::SymposiumRecommendations,
            "example-tools",
        );
        // Cross-checked with .NET's HMACSHA256 over the contract header,
        // identifier window, public source, and plugin name. The complete
        // digest is
        // 7af980f6c5e53598991b1ac981cffb14c5b48919349bcd2494961fb512f74d71.
        let expected = "plg_7af980f6c5e53598991b1ac981cffb14".parse().unwrap();

        let subject = plugin_subject(&coordinate);

        assert_eq!(subject, expected);
    }

    #[test]
    fn plugin_subject_changes_with_source_or_name() {
        let baseline = public_plugin(
            PublicExtensionSource::SymposiumRecommendations,
            "example-tools",
        );
        let other_source = public_plugin(PublicExtensionSource::CratesIo, "example-tools");
        let other_name = public_plugin(
            PublicExtensionSource::SymposiumRecommendations,
            "other-tools",
        );

        let baseline = plugin_subject(&baseline);
        let other_source = plugin_subject(&other_source);
        let other_name = plugin_subject(&other_name);

        assert_ne!(baseline, other_source);
        assert_ne!(baseline, other_name);
    }
}
