//! Safe extension-resolution path vocabulary.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use super::{
    super::{
        EventId, RowKind, SchemaVersion, SymposiumVersion,
        extension::{
            ExtensionKind, PublicExtensionCoordinate, PublicExtensionName, PublicExtensionSource,
        },
        macros::strict_versioned_row,
    },
    package::PublicPackageCoordinate,
};
use crate::telemetry::identity::{
    DimensionWriter, ExtensionDomain, ExtensionSubject, IdentifierWindowScope, IdentityDimension,
};
use crate::telemetry::state::BoundRecordingObservation;

/// Maximum root-to-leaf depth of a recorded resolution path.
const MAX_RESOLUTION_PATH_DEPTH: usize = 8;

/// Maximum combined terminal-node count in a recorded resolution path.
const MAX_RESOLUTION_PATH_LEAVES: usize = 16;

/// Maximum compact UTF-8 JSON size of a complete resolution path.
const MAX_RESOLUTION_PATH_ENCODED_BYTES: usize = 4 * 1024;

/// Safe evidence node in a successful extension-resolution path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ResolutionPathNode {
    Package(PublicPackageCoordinate),
    Extension(ExtensionNode),
    All(AllNode),
    Any(AnyNode),
    Not(NotNode),
    Opaque(OpaqueNode),
}

impl ResolutionPathNode {
    fn write_identity(&self, writer: &mut DimensionWriter<'_>) {
        match self {
            Self::Package(coordinate) => writer.variant("package", |writer| {
                coordinate.write_identity_fields(writer);
            }),
            Self::Extension(node) => writer.variant("extension", |writer| {
                writer.field(node.extension_type.as_str().as_bytes());
                writer.field(node.source.as_str().as_bytes());
                writer.field(node.name.as_str().as_bytes());
            }),
            Self::All(node) => writer.variant("all", |writer| {
                writer.sequence(&node.children, |writer, child| {
                    child.write_identity(writer);
                });
            }),
            Self::Any(node) => writer.variant("any", |writer| {
                node.child.write_identity(writer);
            }),
            Self::Not(_) => writer.variant("not", |_| {}),
            Self::Opaque(node) => writer.variant("opaque", |writer| {
                writer.field(node.reason.as_str().as_bytes());
            }),
        }
    }

    fn validate_depth(&self, depth: usize) -> Result<(), ResolutionPathError> {
        if depth > MAX_RESOLUTION_PATH_DEPTH {
            return Err(ResolutionPathError::DepthExceeded {
                observed: depth,
                maximum: MAX_RESOLUTION_PATH_DEPTH,
            });
        }

        match self {
            Self::All(node) => {
                let child_depth = depth
                    .checked_add(1)
                    .expect("BUG: resolution path depth is bounded before descending");
                for child in &node.children {
                    child.validate_depth(child_depth)?;
                }
            }
            Self::Any(node) => {
                let child_depth = depth
                    .checked_add(1)
                    .expect("BUG: resolution path depth is bounded before descending");
                node.child.validate_depth(child_depth)?;
            }
            Self::Package(_) | Self::Extension(_) | Self::Not(_) | Self::Opaque(_) => {}
        }

        Ok(())
    }

    fn count_leaves(&self, leaf_count: &mut usize) -> Result<(), ResolutionPathError> {
        match self {
            Self::All(node) => {
                for child in &node.children {
                    child.count_leaves(leaf_count)?;
                }
            }
            Self::Any(node) => node.child.count_leaves(leaf_count)?,
            Self::Package(_) | Self::Extension(_) | Self::Not(_) | Self::Opaque(_) => {
                let observed = leaf_count
                    .checked_add(1)
                    .expect("BUG: resolution path leaf count is bounded before incrementing");

                if observed > MAX_RESOLUTION_PATH_LEAVES {
                    return Err(ResolutionPathError::LeafCountExceeded {
                        observed,
                        maximum: MAX_RESOLUTION_PATH_LEAVES,
                    });
                }

                *leaf_count = observed;
            }
        }

        Ok(())
    }
}

/// Complete evidence path for one successful extension resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<ResolutionPathNode>")]
pub(in crate::telemetry) struct ResolutionPath(Vec<ResolutionPathNode>);

impl ResolutionPath {
    /// Derive the subject for this path and its resolved public target.
    #[must_use]
    fn derive_subject(
        &self,
        scope: &IdentifierWindowScope<'_>,
        target: &PublicExtensionCoordinate,
    ) -> ExtensionSubject {
        scope.derive(&ExtensionSubjectDimension { target, path: self })
    }

    fn write_identity(&self, writer: &mut DimensionWriter<'_>) {
        writer.sequence(&self.0, |writer, node| node.write_identity(writer));
    }
}

/// Public skill coordinate safe to persist in the installation index.
///
/// The full public extension coordinate remains on the wire, including its
/// `type` field. Construction and deserialization both reject coordinates for
/// any extension kind other than `skill`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::telemetry) struct PublicSkillCoordinate(PublicExtensionCoordinate);

impl PublicSkillCoordinate {
    /// Return the validated public extension coordinate for this skill.
    #[must_use]
    const fn as_extension(&self) -> &PublicExtensionCoordinate {
        &self.0
    }
}

impl TryFrom<PublicExtensionCoordinate> for PublicSkillCoordinate {
    type Error = NotPublicSkill;

    fn try_from(coordinate: PublicExtensionCoordinate) -> Result<Self, Self::Error> {
        if coordinate.kind() != ExtensionKind::Skill {
            return Err(NotPublicSkill {
                found: coordinate.kind(),
            });
        }

        Ok(Self(coordinate))
    }
}

impl Serialize for PublicSkillCoordinate {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PublicSkillCoordinate {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let coordinate = PublicExtensionCoordinate::deserialize(deserializer)?;

        Self::try_from(coordinate).map_err(D::Error::custom)
    }
}

/// A public extension coordinate that identifies something other than a skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) struct NotPublicSkill {
    found: ExtensionKind,
}

impl fmt::Display for NotPublicSkill {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "expected a public skill coordinate, found {}",
            self.found.as_str()
        )
    }
}

impl std::error::Error for NotPublicSkill {}

/// Safe public attribution persisted for one installed skill.
///
/// Deserializing this type revalidates the skill-only target and the complete
/// resolution-path depth, leaf-count, and encoded-size limits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::telemetry) struct SafeSkillAttribution {
    target: PublicSkillCoordinate,
    path: ResolutionPath,
}

impl SafeSkillAttribution {
    /// Combine a validated public skill with the safe path that selected it.
    #[must_use]
    pub(in crate::telemetry) const fn new(
        target: PublicSkillCoordinate,
        path: ResolutionPath,
    ) -> Self {
        Self { target, path }
    }

    /// Return the validated public skill selected by this attribution.
    #[must_use]
    pub(in crate::telemetry) const fn target(&self) -> &PublicSkillCoordinate {
        &self.target
    }

    /// Derive the subject shared by resolution and invocation telemetry.
    #[must_use]
    pub(in crate::telemetry) fn derive_subject(
        &self,
        scope: &IdentifierWindowScope<'_>,
    ) -> ExtensionSubject {
        self.path.derive_subject(scope, self.target.as_extension())
    }
}

strict_versioned_row! {
    /// Version 1 record of one public extension and a safe path that selected it.
    pub(in crate::telemetry) struct ExtensionResolutionV1 {
        symposium: SymposiumVersion,
        target: PublicExtensionCoordinate,
        path: ResolutionPath,
        extension_subject: ExtensionSubject,
    }

    kind: RowKind::ExtensionResolution,
    raw: RawExtensionResolutionV1,
}

impl ExtensionResolutionV1 {
    /// Create a record for one public extension and its safe resolution path.
    #[must_use]
    pub(in crate::telemetry) fn new(
        observation: &BoundRecordingObservation<'_>,
        target: PublicExtensionCoordinate,
        path: ResolutionPath,
    ) -> Self {
        let extension_subject = path.derive_subject(observation.identifier_window_scope(), &target);

        Self {
            version: SchemaVersion::V1,
            kind: Self::KIND,
            event_id: EventId::new(),
            day: observation.day(),
            symposium: SymposiumVersion::current(),
            target,
            path,
            extension_subject,
        }
    }
}

struct ExtensionSubjectDimension<'a> {
    target: &'a PublicExtensionCoordinate,
    path: &'a ResolutionPath,
}

impl IdentityDimension for ExtensionSubjectDimension<'_> {
    type Domain = ExtensionDomain;

    /// Write the version 1 `extension_subject` fields in contract order.
    fn write(&self, writer: &mut DimensionWriter<'_>) {
        writer.field(self.target.kind().as_str().as_bytes());
        writer.field(self.target.source().as_str().as_bytes());
        writer.field(self.target.name().as_str().as_bytes());
        self.path.write_identity(writer);
    }
}

impl TryFrom<Vec<ResolutionPathNode>> for ResolutionPath {
    type Error = ResolutionPathError;

    fn try_from(nodes: Vec<ResolutionPathNode>) -> Result<Self, Self::Error> {
        if nodes.is_empty() {
            return Err(ResolutionPathError::Empty);
        }

        for node in &nodes {
            node.validate_depth(1)?;
        }

        let mut leaf_count = 0;
        for node in &nodes {
            node.count_leaves(&mut leaf_count)?;
        }

        let encoded_size = serde_json::to_vec(&nodes)
            .expect("BUG: resolution path nodes must have an infallible JSON representation")
            .len();
        if encoded_size > MAX_RESOLUTION_PATH_ENCODED_BYTES {
            return Err(ResolutionPathError::EncodedSizeExceeded {
                observed: encoded_size,
                maximum: MAX_RESOLUTION_PATH_ENCODED_BYTES,
            });
        }

        Ok(Self(nodes))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResolutionPathError {
    Empty,
    DepthExceeded { observed: usize, maximum: usize },
    LeafCountExceeded { observed: usize, maximum: usize },
    EncodedSizeExceeded { observed: usize, maximum: usize },
}

impl fmt::Display for ResolutionPathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("resolution path must contain at least one node"),
            Self::DepthExceeded { observed, maximum } => write!(
                formatter,
                "resolution path depth {observed} exceeds maximum {maximum}"
            ),
            Self::LeafCountExceeded { observed, maximum } => write!(
                formatter,
                "resolution path leaf count {observed} exceeds maximum {maximum}"
            ),
            Self::EncodedSizeExceeded { observed, maximum } => write!(
                formatter,
                "resolution path encoded size {observed} bytes exceeds maximum {maximum} bytes"
            ),
        }
    }
}

impl std::error::Error for ResolutionPathError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExtensionNode {
    extension_type: ExtensionKind,
    source: PublicExtensionSource,
    name: PublicExtensionName,
}

impl From<PublicExtensionCoordinate> for ExtensionNode {
    fn from(coordinate: PublicExtensionCoordinate) -> Self {
        Self {
            extension_type: coordinate.kind(),
            source: coordinate.source(),
            name: coordinate.name().clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawAllNode")]
struct AllNode {
    children: Vec<ResolutionPathNode>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAllNode {
    children: Vec<ResolutionPathNode>,
}

impl TryFrom<RawAllNode> for AllNode {
    type Error = EmptyAllNode;

    fn try_from(raw: RawAllNode) -> Result<Self, Self::Error> {
        if raw.children.is_empty() {
            return Err(EmptyAllNode);
        }

        Ok(Self {
            children: raw.children,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EmptyAllNode;

impl fmt::Display for EmptyAllNode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("all resolution node must contain at least one child")
    }
}

impl std::error::Error for EmptyAllNode {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnyNode {
    child: Box<ResolutionPathNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NotNode {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OpaqueNode {
    reason: OpaqueResolutionReason,
}

/// Fixed explanation for resolution evidence that is unsafe to name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum OpaqueResolutionReason {
    PrivateSource,
    NonPackagePredicate,
    Limit,
}

impl OpaqueResolutionReason {
    const fn as_str(self) -> &'static str {
        match self {
            Self::PrivateSource => "private_source",
            Self::NonPackagePredicate => "non_package_predicate",
            Self::Limit => "limit",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::{
        IDENTIFIER_WINDOW_TEST_STATE, LowVolumeRow, RowClassification, TelemetryRow,
        assert_contract_names_with_labels, classify_row, recorded_data_example_block,
        recording_observation,
    };
    use super::*;
    use crate::telemetry::{identity::encode_dimension_for_test, state::TelemetryStateV1};

    fn documented_path_node_examples() -> impl Iterator<Item = &'static str> {
        let example_block = recorded_data_example_block("### `extension_resolution`", "```json");

        example_block.lines().filter(|line| !line.is_empty())
    }

    fn resolution_path_with_depth(depth: usize) -> String {
        assert!(depth > 0);

        let mut node = r#"{"type":"not"}"#.to_owned();
        for _ in 1..depth {
            node = format!(r#"{{"type":"any","child":{node}}}"#);
        }

        format!("[{node}]")
    }

    fn resolution_path_with_all_depth(depth: usize) -> String {
        assert!(depth > 0);

        let mut node = r#"{"type":"not"}"#.to_owned();
        for _ in 1..depth {
            node = format!(r#"{{"type":"all","children":[{node}]}}"#);
        }

        format!("[{node}]")
    }

    fn resolution_path_with_leaves(leaves: usize) -> String {
        let nodes = std::iter::repeat_n(r#"{"type":"not"}"#, leaves)
            .collect::<Vec<_>>()
            .join(",");

        format!("[{nodes}]")
    }

    fn package_resolution_path_with_encoded_size(encoded_size: usize) -> String {
        const PREFIX: &str =
            r#"[{"type":"package","ecosystem":"cargo","name":"a","version":"1.2.3+"#;
        const SUFFIX: &str = r#""}]"#;

        let metadata_size = encoded_size
            .checked_sub(PREFIX.len() + SUFFIX.len())
            .unwrap();
        let json = format!("{PREFIX}{}{SUFFIX}", "a".repeat(metadata_size));

        assert_eq!(json.len(), encoded_size);
        json
    }

    fn resolution_path_with_leaves_nested_under_all_and_any(
        left_leaves: usize,
        right_leaves: usize,
    ) -> String {
        let left_children = std::iter::repeat_n(serde_json::json!({ "type": "not" }), left_leaves)
            .collect::<Vec<_>>();
        let right_children =
            std::iter::repeat_n(serde_json::json!({ "type": "not" }), right_leaves)
                .collect::<Vec<_>>();

        serde_json::json!([{
            "type": "all",
            "children": [
                { "type": "all", "children": left_children },
                {
                    "type": "any",
                    "child": { "type": "all", "children": right_children }
                }
            ]
        }])
        .to_string()
    }

    fn validate_resolution_path(json: &str) -> Result<ResolutionPath, ResolutionPathError> {
        let nodes = serde_json::from_str::<Vec<ResolutionPathNode>>(json)
            .expect("BUG: generated test path must contain valid resolution nodes");

        ResolutionPath::try_from(nodes)
    }

    fn append_expected_field(output: &mut Vec<u8>, value: &str) {
        let length = u64::try_from(value.len()).unwrap();
        output.extend_from_slice(&length.to_be_bytes());
        output.extend_from_slice(value.as_bytes());
    }

    fn public_target() -> PublicExtensionCoordinate {
        PublicExtensionCoordinate::try_new(
            ExtensionKind::Skill,
            PublicExtensionSource::SymposiumRecommendations,
            "example-debugging",
        )
        .unwrap()
    }

    fn resolution_path_with_every_node_variant() -> ResolutionPath {
        serde_json::from_str(
            r#"[{"type":"package","ecosystem":"cargo","name":"example-runtime","version":"1.2.3"},{"type":"extension","extension_type":"plugin","source":"crates-io","name":"example-tools"},{"type":"all","children":[{"type":"any","child":{"type":"opaque","reason":"private_source"}},{"type":"not"}]}]"#,
        )
        .unwrap()
    }

    #[test]
    fn documented_resolution_path_nodes_round_trip_in_contract_shape() {
        for json in documented_path_node_examples() {
            let node = serde_json::from_str::<ResolutionPathNode>(json).unwrap();
            let encoded = serde_json::to_string(&node).unwrap();

            assert_eq!(encoded, json);
        }
    }

    #[test]
    fn opaque_resolution_reasons_round_trip_with_identity_labels() {
        let cases = [
            (OpaqueResolutionReason::PrivateSource, "private_source"),
            (
                OpaqueResolutionReason::NonPackagePredicate,
                "non_package_predicate",
            ),
            (OpaqueResolutionReason::Limit, "limit"),
        ];

        assert_contract_names_with_labels(&cases, OpaqueResolutionReason::as_str);
    }

    #[test]
    fn non_empty_resolution_path_round_trips_as_an_array() {
        let json = r#"[{"type":"not"}]"#;

        let path = serde_json::from_str::<ResolutionPath>(json).unwrap();
        let encoded = serde_json::to_string(&path).unwrap();

        assert_eq!(encoded, json);
    }

    #[test]
    fn extension_subject_dimension_places_target_before_counted_path() {
        let target = public_target();
        let path = serde_json::from_str::<ResolutionPath>(r#"[{"type":"not"}]"#).unwrap();
        let dimension = ExtensionSubjectDimension {
            target: &target,
            path: &path,
        };

        let encoded = encode_dimension_for_test(&dimension);
        let expected = [
            5_u64.to_be_bytes().as_slice(),
            b"skill",
            25_u64.to_be_bytes().as_slice(),
            b"symposium-recommendations",
            17_u64.to_be_bytes().as_slice(),
            b"example-debugging",
            1_u64.to_be_bytes().as_slice(),
            3_u64.to_be_bytes().as_slice(),
            b"not",
        ]
        .concat();

        assert_eq!(encoded, expected);
    }

    #[test]
    fn extension_subject_dimension_encodes_every_path_node_variant() {
        let target = public_target();
        let path = resolution_path_with_every_node_variant();
        let dimension = ExtensionSubjectDimension {
            target: &target,
            path: &path,
        };

        let encoded = encode_dimension_for_test(&dimension);
        let mut expected = Vec::new();
        append_expected_field(&mut expected, "skill");
        append_expected_field(&mut expected, "symposium-recommendations");
        append_expected_field(&mut expected, "example-debugging");
        expected.extend_from_slice(&3_u64.to_be_bytes());
        append_expected_field(&mut expected, "package");
        append_expected_field(&mut expected, "cargo");
        append_expected_field(&mut expected, "example-runtime");
        append_expected_field(&mut expected, "1.2.3");
        append_expected_field(&mut expected, "extension");
        append_expected_field(&mut expected, "plugin");
        append_expected_field(&mut expected, "crates-io");
        append_expected_field(&mut expected, "example-tools");
        append_expected_field(&mut expected, "all");
        expected.extend_from_slice(&2_u64.to_be_bytes());
        append_expected_field(&mut expected, "any");
        append_expected_field(&mut expected, "opaque");
        append_expected_field(&mut expected, "private_source");
        append_expected_field(&mut expected, "not");

        assert_eq!(encoded, expected);
    }

    #[test]
    fn safe_skill_attribution_derives_the_independent_subject_vector() {
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let observation = recording_observation(&mut state);
        let target = PublicSkillCoordinate::try_from(public_target()).unwrap();
        let attribution =
            SafeSkillAttribution::new(target, resolution_path_with_every_node_variant());
        // Cross-checked with .NET's HMACSHA256 over the contract header,
        // identifier window, public target, and complete recursive path. The
        // complete digest is
        // 63872efd4737ec84179b4e8b0662c1212e9e3295e1940387c4a4e2cca0a9090e.
        let expected_subject = "ext_63872efd4737ec84179b4e8b0662c121".parse().unwrap();

        let subject = attribution.derive_subject(observation.identifier_window_scope());

        assert_eq!(subject, expected_subject);
    }

    #[test]
    fn public_skill_coordinate_preserves_the_full_contract_shape() {
        let target = PublicSkillCoordinate::try_from(public_target()).unwrap();

        let json = serde_json::to_string(&target).unwrap();
        let decoded = serde_json::from_str::<PublicSkillCoordinate>(&json).unwrap();

        assert_eq!(
            json,
            r#"{"type":"skill","source":"symposium-recommendations","name":"example-debugging"}"#
        );
        assert_eq!(decoded, target);
    }

    #[test]
    fn public_skill_coordinate_rejects_a_plugin_at_both_boundaries() {
        let plugin = PublicExtensionCoordinate::try_new(
            ExtensionKind::Plugin,
            PublicExtensionSource::CratesIo,
            "example-tools",
        )
        .unwrap();
        let constructed = PublicSkillCoordinate::try_from(plugin);
        let deserialized = serde_json::from_str::<PublicSkillCoordinate>(
            r#"{"type":"plugin","source":"crates-io","name":"example-tools"}"#,
        );

        assert_eq!(
            constructed,
            Err(NotPublicSkill {
                found: ExtensionKind::Plugin,
            })
        );
        assert!(
            deserialized
                .unwrap_err()
                .to_string()
                .contains("expected a public skill coordinate, found plugin")
        );
    }

    #[test]
    fn safe_skill_attribution_round_trips_in_index_shape() {
        let attribution = SafeSkillAttribution::new(
            PublicSkillCoordinate::try_from(public_target()).unwrap(),
            serde_json::from_str(r#"[{"type":"not"}]"#).unwrap(),
        );

        let json = serde_json::to_string(&attribution).unwrap();
        let decoded = serde_json::from_str::<SafeSkillAttribution>(&json).unwrap();

        assert_eq!(
            json,
            r#"{"target":{"type":"skill","source":"symposium-recommendations","name":"example-debugging"},"path":[{"type":"not"}]}"#
        );
        assert_eq!(decoded, attribution);
    }

    #[test]
    fn safe_skill_attribution_rejects_unknown_fields() {
        let json = r#"{"target":{"type":"skill","source":"symposium-recommendations","name":"example-debugging"},"path":[{"type":"not"}],"private_name":"debugging"}"#;

        let result = serde_json::from_str::<SafeSkillAttribution>(json);

        assert!(result.is_err());
    }

    #[test]
    fn safe_skill_attribution_revalidates_resolution_path_limits() {
        let path = resolution_path_with_depth(9);
        let json = format!(
            r#"{{"target":{{"type":"skill","source":"symposium-recommendations","name":"example-debugging"}},"path":{path}}}"#
        );

        let result = serde_json::from_str::<SafeSkillAttribution>(&json);

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("resolution path depth 9 exceeds maximum 8")
        );
    }

    #[test]
    fn new_extension_resolution_derives_subject_from_its_target_and_path() {
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let observation = recording_observation(&mut state);
        let day = observation.day();
        let target = public_target();
        let path = resolution_path_with_every_node_variant();
        let expected_subject = "ext_63872efd4737ec84179b4e8b0662c121".parse().unwrap();

        let row = ExtensionResolutionV1::new(&observation, target, path);

        assert_eq!(row.version, SchemaVersion::V1);
        assert_eq!(row.kind, RowKind::ExtensionResolution);
        assert_eq!(row.event_id.0.get_version(), Some(uuid::Version::Random));
        assert_eq!(row.day, day);
        assert_eq!(row.symposium, SymposiumVersion::current());
        assert_eq!(row.target, public_target());
        assert_eq!(row.path, resolution_path_with_every_node_variant());
        assert_eq!(row.extension_subject, expected_subject);
    }

    #[test]
    fn nested_extension_resolution_round_trips_through_the_classifier() {
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let observation = recording_observation(&mut state);
        let row = ExtensionResolutionV1::new(
            &observation,
            public_target(),
            resolution_path_with_every_node_variant(),
        );
        let json = serde_json::to_string(&row).unwrap();

        let RowClassification::Supported(TelemetryRow::LowVolume(
            LowVolumeRow::ExtensionResolution(decoded),
        )) = classify_row(&json)
        else {
            panic!("nested extension_resolution row was not classified as supported");
        };

        assert_eq!(decoded, row);
    }

    #[test]
    fn empty_resolution_path_is_rejected_at_both_boundaries() {
        let constructed = ResolutionPath::try_from(Vec::new());
        let deserialized = serde_json::from_str::<ResolutionPath>("[]");

        assert_eq!(constructed.unwrap_err(), ResolutionPathError::Empty);
        assert!(
            deserialized
                .unwrap_err()
                .to_string()
                .contains("resolution path must contain at least one node")
        );
    }

    #[test]
    fn resolution_path_accepts_depth_eight_and_rejects_depth_nine() {
        let at_limit = resolution_path_with_depth(8);
        let beyond_limit = resolution_path_with_depth(9);

        let accepted = validate_resolution_path(&at_limit);
        let rejected = validate_resolution_path(&beyond_limit);

        assert!(accepted.is_ok());
        assert_eq!(
            rejected.unwrap_err(),
            ResolutionPathError::DepthExceeded {
                observed: 9,
                maximum: 8,
            }
        );
    }

    #[test]
    fn resolution_path_counts_depth_through_all_nodes() {
        let at_limit = resolution_path_with_all_depth(8);
        let beyond_limit = resolution_path_with_all_depth(9);

        let accepted = validate_resolution_path(&at_limit);
        let rejected = validate_resolution_path(&beyond_limit);

        assert!(accepted.is_ok());
        assert_eq!(
            rejected.unwrap_err(),
            ResolutionPathError::DepthExceeded {
                observed: 9,
                maximum: 8,
            }
        );
    }

    #[test]
    fn resolution_path_accepts_sixteen_leaves_and_rejects_seventeen() {
        let at_limit = resolution_path_with_leaves(16);
        let beyond_limit = resolution_path_with_leaves(17);

        let accepted = validate_resolution_path(&at_limit);
        let rejected = validate_resolution_path(&beyond_limit);

        assert!(accepted.is_ok());
        assert_eq!(
            rejected.unwrap_err(),
            ResolutionPathError::LeafCountExceeded {
                observed: 17,
                maximum: 16,
            }
        );
    }

    #[test]
    fn resolution_path_counts_only_terminal_leaves_across_nested_all_and_any_nodes() {
        let at_limit = resolution_path_with_leaves_nested_under_all_and_any(8, 8);
        let beyond_limit = resolution_path_with_leaves_nested_under_all_and_any(8, 9);

        let accepted = validate_resolution_path(&at_limit);
        let rejected = validate_resolution_path(&beyond_limit);

        assert!(accepted.is_ok());
        assert_eq!(
            rejected.unwrap_err(),
            ResolutionPathError::LeafCountExceeded {
                observed: 17,
                maximum: 16,
            }
        );
    }

    #[test]
    fn resolution_path_accepts_4096_bytes_and_rejects_4097() {
        let at_limit = package_resolution_path_with_encoded_size(4_096);
        let beyond_limit = package_resolution_path_with_encoded_size(4_097);

        let accepted = validate_resolution_path(&at_limit);
        let rejected = validate_resolution_path(&beyond_limit);

        assert!(accepted.is_ok());
        assert_eq!(
            rejected.unwrap_err(),
            ResolutionPathError::EncodedSizeExceeded {
                observed: 4_097,
                maximum: 4_096,
            }
        );
    }

    #[test]
    fn extension_node_is_built_from_a_validated_coordinate() {
        let coordinate = PublicExtensionCoordinate::try_new(
            ExtensionKind::Skill,
            PublicExtensionSource::SymposiumRecommendations,
            "example-debugging",
        )
        .unwrap();

        let node = ResolutionPathNode::Extension(coordinate.into());
        let encoded = serde_json::to_string(&node).unwrap();

        assert_eq!(
            encoded,
            r#"{"type":"extension","extension_type":"skill","source":"symposium-recommendations","name":"example-debugging"}"#
        );
    }

    #[test]
    fn resolution_path_nodes_reject_unknown_nested_fields() {
        let json = r#"{"type":"any","child":{"type":"not","predicate":"private"}}"#;

        let result = serde_json::from_str::<ResolutionPathNode>(json);

        assert!(result.is_err());
    }

    #[test]
    fn resolution_path_nodes_require_their_contract_fields() {
        let json = r#"{"type":"extension","extension_type":"skill","name":"example-debugging"}"#;

        let result = serde_json::from_str::<ResolutionPathNode>(json);

        assert!(result.is_err());
    }

    #[test]
    fn all_resolution_node_requires_at_least_one_child() {
        let json = r#"{"type":"all","children":[]}"#;

        let result = serde_json::from_str::<ResolutionPathNode>(json);

        assert!(result.is_err());
    }

    #[test]
    fn resolution_path_nodes_reject_unknown_contract_vocabulary() {
        let unknown_type = serde_json::from_str::<ResolutionPathNode>(r#"{"type":"custom"}"#);
        let unknown_reason = serde_json::from_str::<ResolutionPathNode>(
            r#"{"type":"opaque","reason":"private_predicate"}"#,
        );

        assert!(unknown_type.is_err());
        assert!(unknown_reason.is_err());
    }

    #[test]
    fn resolution_path_nodes_validate_public_coordinates() {
        let invalid_package = serde_json::from_str::<ResolutionPathNode>(
            r#"{"type":"package","ecosystem":"cargo","name":"example-runtime","version":"1.2"}"#,
        );
        let invalid_extension = serde_json::from_str::<ResolutionPathNode>(
            r#"{"type":"extension","extension_type":"skill","source":"symposium-recommendations","name":"private/skill"}"#,
        );

        assert!(invalid_package.is_err());
        assert!(invalid_extension.is_err());
    }
}
