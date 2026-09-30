//! Types that define telemetry's serialized data contract.
#![cfg_attr(
    not(test),
    expect(dead_code, reason = "the new schema is built before storage uses it.")
)]

mod agent;
mod command;
mod extension;
mod extension_invocation;
mod hook;
mod macros;
mod metrics;
mod name;
mod plugin_hook;
mod resolution;

pub(in crate::telemetry) use agent::{HookAgent, SupportedAgent};
pub(in crate::telemetry) use extension_invocation::{
    ExtensionInvocationAgent, ExtensionInvocationAttribution, ExtensionInvocationPhase,
    ExtensionTargetScope, UnnamedExtensionReason,
};

pub(in crate::telemetry) use hook::{HookMetricsKey, HookOutcome, HookSurface};
pub(in crate::telemetry) use metrics::MAX_IDENTIFIED_SESSIONS;
pub(in crate::telemetry) use plugin_hook::{
    PluginHookAttribution, PluginHookOutcome, PluginScope, PublicPluginCoordinate,
};

use std::{fmt, num::NonZeroU64, sync::LazyLock};

use chrono::{DateTime, NaiveDate, SecondsFormat, Timelike, Utc};
use semver::Version;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{DeserializeOwned, Error as _},
};
use uuid::Uuid;

#[cfg(test)]
use crate::telemetry::state::{IDENTIFIER_WINDOW_TEST_STATE, recording_observation};

use agent::{AgentConfigurationV1, SessionStartV1};
use command::CommandV1;
use extension_invocation::ExtensionInvocationMetricsV1;
use hook::HookMetricsV1;
use macros::strict_versioned_row;
use plugin_hook::PluginHookMetricsV1;
pub(in crate::telemetry) use resolution::extension::PublicSkillCoordinate;
use resolution::{
    ResolutionSummaryV1, extension::ExtensionResolutionV1, package::PackageResolutionV1,
};

/// Random identifier for one telemetry row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub(super) struct EventId(Uuid);

impl EventId {
    /// Generate a new random version 4 UUID.
    pub(super) fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

/// Positive schema version carried by a telemetry row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub(super) struct SchemaVersion(NonZeroU64);

impl SchemaVersion {
    /// Initial version of every telemetry row kind.
    pub(super) const V1: Self = Self(NonZeroU64::MIN);
}

/// Kind of row stored in the telemetry data files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RowKind {
    SessionStart,
    AgentConfiguration,
    ResolutionSummary,
    PackageResolution,
    ExtensionResolution,
    HookMetrics,
    PluginHookMetrics,
    ExtensionInvocationMetrics,
    Command,
    StorageLimit,
}

/// Result of interpreting one physical telemetry line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RowClassification {
    Supported(TelemetryRow),
    UnknownSchema,
    Invalid,
    Malformed,
}

/// Telemetry row understood by this version of Symposium.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TelemetryRow {
    SessionStart(SessionStartV1),
    AgentConfiguration(AgentConfigurationV1),
    ResolutionSummary(ResolutionSummaryV1),
    PackageResolution(PackageResolutionV1),
    ExtensionResolution(ExtensionResolutionV1),
    // Box large aggregate rows so every enum value does not inherit their size.
    HookMetrics(Box<HookMetricsV1>),
    PluginHookMetrics(Box<PluginHookMetricsV1>),
    ExtensionInvocationMetrics(Box<ExtensionInvocationMetricsV1>),
    Command(CommandV1),
    StorageLimit(StorageLimitV1),
}

impl Serialize for TelemetryRow {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::SessionStart(row) => row.serialize(serializer),
            Self::AgentConfiguration(row) => row.serialize(serializer),
            Self::ResolutionSummary(row) => row.serialize(serializer),
            Self::PackageResolution(row) => row.serialize(serializer),
            Self::ExtensionResolution(row) => row.serialize(serializer),
            Self::HookMetrics(row) => row.serialize(serializer),
            Self::PluginHookMetrics(row) => row.serialize(serializer),
            Self::ExtensionInvocationMetrics(row) => row.serialize(serializer),
            Self::Command(row) => row.serialize(serializer),
            Self::StorageLimit(row) => row.serialize(serializer),
        }
    }
}

/// Lenient header used to select a complete versioned row schema.
#[derive(Debug, Deserialize)]
struct RowEnvelope {
    #[serde(rename = "v")]
    version: u64,
    kind: String,
}

fn deserialize_version_one<'de, D>(deserializer: D) -> Result<SchemaVersion, D::Error>
where
    D: Deserializer<'de>,
{
    let version = SchemaVersion::deserialize(deserializer)?;

    if version != SchemaVersion::V1 {
        return Err(D::Error::custom(format_args!(
            "expected schema version 1, found {}",
            version.0
        )));
    }

    Ok(version)
}

/// UTC calendar day used to partition telemetry rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct UtcDay(NaiveDate);

impl UtcDay {
    /// Wrap a calendar date known to be in UTC.
    #[must_use]
    pub(super) const fn from_date(date: NaiveDate) -> Self {
        Self(date)
    }

    /// Return the signed number of calendar days from `earlier` to this day.
    #[must_use]
    pub(super) fn days_since(self, earlier: Self) -> i64 {
        self.0.signed_duration_since(earlier.0).num_days()
    }
}

impl fmt::Display for UtcDay {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0.format("%Y-%m-%d"))
    }
}

impl Serialize for UtcDay {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for UtcDay {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;

        if !has_utc_day_shape(&value) {
            return Err(D::Error::custom("expected a UTC day in YYYY-MM-DD form"));
        }

        let date = NaiveDate::parse_from_str(&value, "%Y-%m-%d").map_err(D::Error::custom)?;
        Ok(Self(date))
    }
}

fn has_utc_day_shape(value: &str) -> bool {
    let bytes = value.as_bytes();

    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[8..].iter().all(u8::is_ascii_digit)
}

/// Zero-based day within one D0-D30 observed-session return cohort.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct CohortDay(u8);

impl CohortDay {
    /// The first observed day in a return cohort.
    pub(super) const D0: Self = Self(0);

    /// The last day in a return cohort.
    pub(super) const D30: Self = Self(30);

    /// Return the zero-based day number.
    #[must_use]
    pub(super) const fn get(self) -> u8 {
        self.0
    }
}

/// A day number outside the D0-D30 return-cohort range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CohortDayOutOfRange(i64);

impl fmt::Display for CohortDayOutOfRange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cohort day must be from 0 through {}, found {}",
            CohortDay::D30.get(),
            self.0
        )
    }
}

impl std::error::Error for CohortDayOutOfRange {}

impl TryFrom<i64> for CohortDay {
    type Error = CohortDayOutOfRange;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        let last_cohort_day = i64::from(Self::D30.get());
        if !(0..=last_cohort_day).contains(&value) {
            return Err(CohortDayOutOfRange(value));
        }

        let value = u8::try_from(value).expect("BUG: a validated return-cohort day must fit in u8");
        Ok(Self(value))
    }
}

impl Serialize for CohortDay {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(self.0)
    }
}

impl<'de> Deserialize<'de> for CohortDay {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = i64::deserialize(deserializer)?;
        Self::try_from(value).map_err(D::Error::custom)
    }
}

/// RFC 3339 UTC timestamp with no subsecond precision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct UtcSecond(DateTime<Utc>);

impl UtcSecond {
    /// Convert a UTC timestamp, discarding any subsecond precision.
    pub(super) fn from_datetime(timestamp: DateTime<Utc>) -> Self {
        Self(
            timestamp
                .with_nanosecond(0)
                .expect("BUG: zero nanoseconds must be valid for a UTC timestamp"),
        )
    }

    /// Return the UTC calendar day containing this timestamp.
    pub(super) fn day(&self) -> UtcDay {
        UtcDay::from_date(self.0.date_naive())
    }
}

impl Serialize for UtcSecond {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_rfc3339_opts(SecondsFormat::Secs, true))
    }
}

impl<'de> Deserialize<'de> for UtcSecond {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let timestamp = DateTime::parse_from_rfc3339(&value).map_err(D::Error::custom)?;

        if timestamp.offset().local_minus_utc() != 0 {
            return Err(D::Error::custom("expected a UTC timestamp"));
        }

        let canonical_z = timestamp.to_rfc3339_opts(SecondsFormat::Secs, true);
        let canonical_offset = timestamp.to_rfc3339_opts(SecondsFormat::Secs, false);

        if value != canonical_z && value != canonical_offset {
            return Err(D::Error::custom(
                "expected a UTC timestamp with whole-second precision",
            ));
        }

        Ok(Self(timestamp.with_timezone(&Utc)))
    }
}

/// Version of Symposium that produced a telemetry row.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct SymposiumVersion(Version);

static CURRENT_SYMPOSIUM_VERSION: LazyLock<Version> = LazyLock::new(|| {
    Version::parse(env!("CARGO_PKG_VERSION"))
        .expect("BUG: Cargo package version must be valid semantic versioning")
});

impl SymposiumVersion {
    /// Return the version of the running Symposium binary.
    pub(super) fn current() -> Self {
        Self(CURRENT_SYMPOSIUM_VERSION.clone())
    }
}

impl Serialize for SymposiumVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SymposiumVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Version::parse(&value).map(Self).map_err(D::Error::custom)
    }
}

strict_versioned_row! {
    /// Version 1 marker recording that the daily storage limit rejected an operation.
    pub(super) struct StorageLimitV1 {
        symposium: SymposiumVersion,
        dropped_operation: DroppedOperation,
    }

    kind: RowKind::StorageLimit,
    raw: RawStorageLimitV1,
}

impl StorageLimitV1 {
    /// Create a marker for an operation rejected by the daily storage limit.
    #[must_use]
    pub(super) fn new(day: UtcDay, dropped_operation: DroppedOperation) -> Self {
        Self {
            version: SchemaVersion::V1,
            kind: Self::KIND,
            event_id: EventId::new(),
            day,
            symposium: SymposiumVersion::current(),
            dropped_operation,
        }
    }
}

/// Operation whose telemetry did not fit in the daily storage allowance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum DroppedOperation {
    SessionStart,
    ManualSync,
    Use,
    Remove,
    Init,
    Configuration,
    Command,
}

/// Classify a physical JSONL line and return typed data only for a known schema.
///
/// This is the only supported entry point for reading typed rows. Individual
/// versioned row types assume the envelope dispatch has already matched their
/// `kind` and must not be deserialized directly.
pub(super) fn classify_row(line: &str) -> RowClassification {
    let Ok(envelope) = serde_json::from_str::<RowEnvelope>(line) else {
        return RowClassification::Malformed;
    };

    match (envelope.kind.as_str(), envelope.version) {
        ("session_start", 1) => deserialize_supported_row(line, TelemetryRow::SessionStart),
        ("agent_configuration", 1) => {
            deserialize_supported_row(line, TelemetryRow::AgentConfiguration)
        }
        ("resolution_summary", 1) => {
            deserialize_supported_row(line, TelemetryRow::ResolutionSummary)
        }
        ("package_resolution", 1) => {
            deserialize_supported_row(line, TelemetryRow::PackageResolution)
        }
        ("extension_resolution", 1) => {
            deserialize_supported_row(line, TelemetryRow::ExtensionResolution)
        }
        ("hook_metrics", 1) => {
            deserialize_supported_row(line, |row| TelemetryRow::HookMetrics(Box::new(row)))
        }
        ("plugin_hook_metrics", 1) => {
            deserialize_supported_row(line, |row| TelemetryRow::PluginHookMetrics(Box::new(row)))
        }
        ("extension_invocation_metrics", 1) => deserialize_supported_row(line, |row| {
            TelemetryRow::ExtensionInvocationMetrics(Box::new(row))
        }),
        ("command", 1) => deserialize_supported_row(line, TelemetryRow::Command),
        ("storage_limit", 1) => deserialize_supported_row(line, TelemetryRow::StorageLimit),
        _ => RowClassification::UnknownSchema,
    }
}

fn deserialize_supported_row<T>(line: &str, wrap: fn(T) -> TelemetryRow) -> RowClassification
where
    T: DeserializeOwned,
{
    match serde_json::from_str(line) {
        Ok(row) => RowClassification::Supported(wrap(row)),
        Err(_) => RowClassification::Invalid,
    }
}

#[cfg(test)]
const RECORDED_DATA_CONTRACT: &str =
    include_str!("../../../md/rfds/telemetry-recording/contract/recorded-data.md");

#[cfg(test)]
fn recorded_data_example_block(section_heading: &str, opening_fence: &str) -> &'static str {
    recorded_data_example_block_at(section_heading, opening_fence, 0)
}

#[cfg(test)]
fn recorded_data_example_block_at(
    section_heading: &str,
    opening_fence: &str,
    block_index: usize,
) -> &'static str {
    let (_, after_heading) = RECORDED_DATA_CONTRACT
        .split_once(section_heading)
        .unwrap_or_else(|| panic!("recorded-data contract must contain {section_heading}"));
    let after_fence = after_heading
        .split(opening_fence)
        .skip(1)
        .nth(block_index)
        .unwrap_or_else(|| {
            panic!("{section_heading} must contain {opening_fence} block {block_index}")
        });
    let (example_block, _) = after_fence
        .split_once("```")
        .unwrap_or_else(|| panic!("{section_heading} example block must have a closing fence"));

    example_block
}

#[cfg(test)]
fn recorded_data_example_row(requested_kind: &str) -> &'static str {
    let example_block =
        recorded_data_example_block("## Example JSONL for every row kind", "```jsonl");

    example_block
        .lines()
        .filter_map(|line| {
            serde_json::from_str::<RowEnvelope>(line)
                .ok()
                .map(|envelope| (line, envelope))
        })
        .find_map(|(line, envelope)| (envelope.kind == requested_kind).then_some(line))
        .unwrap_or_else(|| panic!("missing {requested_kind} example in recorded-data contract"))
}

#[cfg(test)]
fn assert_contract_names<T>(cases: &[(T, &str)])
where
    T: Copy + fmt::Debug + PartialEq + Serialize + DeserializeOwned,
{
    for &(value, name) in cases {
        let encoded = serde_json::to_string(&value).unwrap();
        let decoded: T = serde_json::from_str(&encoded).unwrap();

        assert_eq!(encoded, format!(r#""{name}""#));
        assert_eq!(decoded, value);
    }
}

#[cfg(test)]
fn assert_contract_names_with_labels<T>(cases: &[(T, &str)], label: impl Fn(T) -> &'static str)
where
    T: Copy + fmt::Debug + PartialEq + Serialize + DeserializeOwned,
{
    assert_contract_names(cases);

    for &(value, name) in cases {
        assert_eq!(label(value), name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example_row(requested_kind: &str) -> &'static str {
        recorded_data_example_row(requested_kind)
    }

    #[test]
    fn new_event_id_is_uuid_v4() {
        let event_id = EventId::new();

        assert_eq!(event_id.0.get_version(), Some(uuid::Version::Random));
    }

    #[test]
    fn event_id_serializes_as_uuid_string() {
        let uuid = Uuid::parse_str("9f2c41b6-495e-4c88-a22b-c597f8102aed").unwrap();
        let event_id = EventId(uuid);

        let json = serde_json::to_string(&event_id).unwrap();

        assert_eq!(json, r#""9f2c41b6-495e-4c88-a22b-c597f8102aed""#);
    }

    #[test]
    fn event_id_round_trips_through_json() {
        let event_id = EventId::new();

        let json = serde_json::to_string(&event_id).unwrap();
        let decoded = serde_json::from_str::<EventId>(&json).unwrap();

        assert_eq!(decoded, event_id);
    }

    #[test]
    fn event_id_rejects_invalid_uuid() {
        let result = serde_json::from_str::<EventId>(r#""not-a-uuid""#);

        assert!(result.is_err());
    }

    #[test]
    fn schema_version_one_serializes_as_number() {
        let json = serde_json::to_string(&SchemaVersion::V1).unwrap();

        assert_eq!(json, "1");
    }

    #[test]
    fn schema_version_accepts_future_positive_value() {
        let version = serde_json::from_str::<SchemaVersion>("2").unwrap();

        assert_eq!(version.0.get(), 2);
    }

    #[test]
    fn schema_version_rejects_invalid_values() {
        for invalid in ["0", "-1", "1.5", r#""1""#] {
            assert!(
                serde_json::from_str::<SchemaVersion>(invalid).is_err(),
                "accepted invalid schema version {invalid}"
            );
        }
    }

    #[test]
    fn row_kinds_round_trip_with_contract_names() {
        let cases = [
            (RowKind::SessionStart, "session_start"),
            (RowKind::AgentConfiguration, "agent_configuration"),
            (RowKind::ResolutionSummary, "resolution_summary"),
            (RowKind::PackageResolution, "package_resolution"),
            (RowKind::ExtensionResolution, "extension_resolution"),
            (RowKind::HookMetrics, "hook_metrics"),
            (RowKind::PluginHookMetrics, "plugin_hook_metrics"),
            (
                RowKind::ExtensionInvocationMetrics,
                "extension_invocation_metrics",
            ),
            (RowKind::Command, "command"),
            (RowKind::StorageLimit, "storage_limit"),
        ];

        assert_contract_names(&cases);
    }

    #[test]
    fn row_kind_rejects_unknown_name() {
        let result = serde_json::from_str::<RowKind>(r#""future_kind""#);

        assert!(result.is_err());
    }

    #[test]
    fn utc_day_serializes_as_calendar_date() {
        let day = UtcDay(NaiveDate::from_ymd_opt(2026, 8, 3).unwrap());

        let json = serde_json::to_string(&day).unwrap();

        assert_eq!(json, r#""2026-08-03""#);
    }

    #[test]
    fn utc_day_displays_as_calendar_date() {
        let day = UtcDay(NaiveDate::from_ymd_opt(2026, 8, 3).unwrap());

        let displayed = day.to_string();

        assert_eq!(displayed, "2026-08-03");
    }

    #[test]
    fn utc_day_round_trips_through_json() {
        let day = UtcDay(NaiveDate::from_ymd_opt(2026, 8, 3).unwrap());

        let json = serde_json::to_string(&day).unwrap();
        let decoded = serde_json::from_str::<UtcDay>(&json).unwrap();

        assert_eq!(decoded, day);
    }

    #[test]
    fn utc_day_rejects_invalid_calendar_date() {
        let result = serde_json::from_str::<UtcDay>(r#""2026-02-30""#);

        assert!(result.is_err());
    }

    #[test]
    fn utc_day_rejects_noncanonical_shapes() {
        for value in ["2026-8-03", "2026-08-3", "+2026-08-03", "2026/08/03"] {
            let json = format!(r#""{value}""#);

            assert!(
                serde_json::from_str::<UtcDay>(&json).is_err(),
                "accepted noncanonical UTC day {value}"
            );
        }
    }

    #[test]
    fn utc_day_difference_is_signed() {
        let earlier = UtcDay(NaiveDate::from_ymd_opt(2026, 8, 11).unwrap());
        let later = UtcDay(NaiveDate::from_ymd_opt(2026, 9, 10).unwrap());

        assert_eq!(later.days_since(earlier), 30);
        assert_eq!(earlier.days_since(later), -30);
    }

    #[test]
    fn cohort_day_accepts_d0_through_d30() {
        for value in 0_u8..=30 {
            let cohort_day = CohortDay::try_from(i64::from(value)).unwrap();

            assert_eq!(cohort_day.get(), value);
        }

        assert_eq!(CohortDay::D0.get(), 0);
        assert_eq!(CohortDay::D30.get(), 30);
    }

    #[test]
    fn cohort_day_rejects_values_outside_d0_through_d30() {
        for value in [-1_i64, 31] {
            let error = CohortDay::try_from(value).unwrap_err();

            assert_eq!(
                error.to_string(),
                format!("cohort day must be from 0 through 30, found {value}")
            );
        }
    }

    #[test]
    fn cohort_day_round_trips_as_a_json_number() {
        for value in [0_i64, 1, 30] {
            let cohort_day = CohortDay::try_from(value).unwrap();

            let json = serde_json::to_string(&cohort_day).unwrap();
            let decoded = serde_json::from_str::<CohortDay>(&json).unwrap();

            assert_eq!(json, value.to_string());
            assert_eq!(decoded, cohort_day);
        }
    }

    #[test]
    fn cohort_day_rejects_invalid_json_values() {
        for invalid in ["-1", "31", "255", "256", "1.5", r#""1""#] {
            assert!(
                serde_json::from_str::<CohortDay>(invalid).is_err(),
                "accepted invalid cohort day {invalid}"
            );
        }
    }

    #[test]
    fn utc_second_constructor_removes_subsecond_precision() {
        let timestamp = DateTime::parse_from_rfc3339("2026-08-03T09:14:02.987Z")
            .unwrap()
            .with_timezone(&Utc);

        let utc_second = UtcSecond::from_datetime(timestamp);

        assert_eq!(utc_second.0.nanosecond(), 0);
    }

    #[test]
    fn utc_second_serializes_as_canonical_utc() {
        let timestamp = DateTime::parse_from_rfc3339("2026-08-03T09:14:02Z")
            .unwrap()
            .with_timezone(&Utc);
        let utc_second = UtcSecond::from_datetime(timestamp);

        let json = serde_json::to_string(&utc_second).unwrap();

        assert_eq!(json, r#""2026-08-03T09:14:02Z""#);
    }

    #[test]
    fn utc_second_accepts_zero_offset() {
        let utc_second =
            serde_json::from_str::<UtcSecond>(r#""2026-08-03T09:14:02+00:00""#).unwrap();

        let json = serde_json::to_string(&utc_second).unwrap();

        assert_eq!(json, r#""2026-08-03T09:14:02Z""#);
    }

    #[test]
    fn utc_second_rejects_fractional_precision() {
        let result = serde_json::from_str::<UtcSecond>(r#""2026-08-03T09:14:02.000Z""#);

        assert!(result.is_err());
    }

    #[test]
    fn utc_second_rejects_non_utc_offset() {
        let result = serde_json::from_str::<UtcSecond>(r#""2026-08-03T12:14:02+03:00""#);

        assert!(result.is_err());
    }

    #[test]
    fn utc_second_rejects_unknown_local_offset() {
        let result = serde_json::from_str::<UtcSecond>(r#""2026-08-03T09:14:02-00:00""#);

        assert!(result.is_err());
    }

    #[test]
    fn utc_second_returns_its_utc_day() {
        let utc_second = serde_json::from_str::<UtcSecond>(r#""2026-08-03T23:59:59Z""#).unwrap();

        assert_eq!(
            utc_second.day(),
            UtcDay(NaiveDate::from_ymd_opt(2026, 8, 3).unwrap())
        );
    }

    #[test]
    fn current_symposium_version_uses_package_version() {
        let version = SymposiumVersion::current();

        assert_eq!(version.0.to_string(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn symposium_version_serializes_as_semver_string() {
        let version = SymposiumVersion(Version::new(1, 2, 3));

        let json = serde_json::to_string(&version).unwrap();

        assert_eq!(json, r#""1.2.3""#);
    }

    #[test]
    fn symposium_version_round_trips_prerelease_and_build_metadata() {
        let version = SymposiumVersion(Version::parse("1.2.3-beta.1+build.7").unwrap());

        let json = serde_json::to_string(&version).unwrap();
        let decoded = serde_json::from_str::<SymposiumVersion>(&json).unwrap();

        assert_eq!(decoded, version);
    }

    #[test]
    fn symposium_version_rejects_invalid_semver() {
        let result = serde_json::from_str::<SymposiumVersion>(r#""not-a-version""#);

        assert!(result.is_err());
    }

    #[test]
    fn storage_limit_example_round_trips() {
        let example = example_row("storage_limit");

        let RowClassification::Supported(row) = classify_row(example) else {
            panic!("storage_limit contract example was not classified as supported");
        };

        // Compared as text, not as `Value`: a `Value` map sorts its keys, which
        // would stop this from pinning the contract's field order.
        assert_eq!(serde_json::to_string(&row).unwrap(), example);
    }

    #[test]
    fn session_start_example_round_trips() {
        let example = example_row("session_start");

        let RowClassification::Supported(row) = classify_row(example) else {
            panic!("session_start contract example was not classified as supported");
        };

        // Compared as text, not as `Value`: a `Value` map sorts its keys, which
        // would stop this from pinning the contract's field order.
        assert_eq!(serde_json::to_string(&row).unwrap(), example);
    }

    #[test]
    fn unsupported_session_start_version_is_unknown_schema() {
        let example = example_row("session_start");
        let json = example.replacen(r#""v":1"#, r#""v":2"#, 1);

        let classification = classify_row(&json);

        assert_eq!(classification, RowClassification::UnknownSchema);
    }

    #[test]
    fn session_start_with_unknown_field_is_invalid() {
        let example = example_row("session_start");
        let json = example.replacen(r#""agent""#, r#""future_field":true,"agent""#, 1);

        let classification = classify_row(&json);

        assert_eq!(classification, RowClassification::Invalid);
    }

    #[test]
    fn session_start_with_a_timestamp_from_another_day_is_invalid() {
        let example = example_row("session_start");
        let mut value = serde_json::from_str::<serde_json::Value>(example).unwrap();
        value["day"] = serde_json::Value::String("2026-08-04".to_owned());
        let json = serde_json::to_string(&value).unwrap();

        let classification = classify_row(&json);

        assert_eq!(classification, RowClassification::Invalid);
    }

    #[test]
    fn agent_configuration_example_round_trips() {
        let example = example_row("agent_configuration");

        let RowClassification::Supported(row) = classify_row(example) else {
            panic!("agent_configuration contract example was not classified as supported");
        };

        assert_eq!(serde_json::to_string(&row).unwrap(), example);
    }

    #[test]
    fn resolution_summary_example_round_trips() {
        let example = example_row("resolution_summary");

        let RowClassification::Supported(row) = classify_row(example) else {
            panic!("resolution_summary contract example was not classified as supported");
        };

        assert_eq!(serde_json::to_string(&row).unwrap(), example);
    }

    #[test]
    fn package_resolution_example_round_trips() {
        let example = example_row("package_resolution");

        let RowClassification::Supported(row) = classify_row(example) else {
            panic!("package_resolution contract example was not classified as supported");
        };

        assert_eq!(serde_json::to_string(&row).unwrap(), example);
    }

    #[test]
    fn extension_resolution_example_round_trips() {
        let example = example_row("extension_resolution");

        let RowClassification::Supported(row) = classify_row(example) else {
            panic!("extension_resolution contract example was not classified as supported");
        };

        assert_eq!(serde_json::to_string(&row).unwrap(), example);
    }

    #[test]
    fn unsupported_extension_resolution_version_is_unknown_schema() {
        let example = example_row("extension_resolution");
        let json = example.replacen(r#""v":1"#, r#""v":2"#, 1);

        let classification = classify_row(&json);

        assert_eq!(classification, RowClassification::UnknownSchema);
    }

    #[test]
    fn extension_resolution_with_unknown_field_is_invalid() {
        let example = example_row("extension_resolution");
        let json = example.replacen(r#""target""#, r#""future_field":true,"target""#, 1);

        let classification = classify_row(&json);

        assert_eq!(classification, RowClassification::Invalid);
    }

    #[test]
    fn extension_resolution_with_missing_field_is_invalid() {
        let example = example_row("extension_resolution");
        let mut value = serde_json::from_str::<serde_json::Value>(example).unwrap();
        value.as_object_mut().unwrap().remove("path");
        let json = serde_json::to_string(&value).unwrap();

        let classification = classify_row(&json);

        assert_eq!(classification, RowClassification::Invalid);
    }

    #[test]
    fn extension_resolution_with_invalid_target_is_invalid() {
        let example = example_row("extension_resolution");
        let mut value = serde_json::from_str::<serde_json::Value>(example).unwrap();
        value["target"]["name"] = serde_json::json!("private/example-debugging");
        let json = serde_json::to_string(&value).unwrap();

        let classification = classify_row(&json);

        assert_eq!(classification, RowClassification::Invalid);
    }

    #[test]
    fn extension_resolution_with_over_limit_path_is_invalid() {
        let example = example_row("extension_resolution");
        let mut value = serde_json::from_str::<serde_json::Value>(example).unwrap();
        value["path"] = serde_json::Value::Array(
            std::iter::repeat_n(serde_json::json!({ "type": "not" }), 17).collect(),
        );
        let json = serde_json::to_string(&value).unwrap();

        let classification = classify_row(&json);

        assert_eq!(classification, RowClassification::Invalid);
    }

    #[test]
    fn unsupported_package_resolution_version_is_unknown_schema() {
        let example = example_row("package_resolution");
        let json = example.replacen(r#""v":1"#, r#""v":2"#, 1);

        let classification = classify_row(&json);

        assert_eq!(classification, RowClassification::UnknownSchema);
    }

    #[test]
    fn package_resolution_with_unknown_field_is_invalid() {
        let example = example_row("package_resolution");
        let json = example.replacen(r#""package""#, r#""future_field":true,"package""#, 1);

        let classification = classify_row(&json);

        assert_eq!(classification, RowClassification::Invalid);
    }

    #[test]
    fn package_resolution_with_invalid_coordinate_is_invalid() {
        let example = example_row("package_resolution");
        let json = example.replacen(r#""version":"1.2.3""#, r#""version":"*""#, 1);

        let classification = classify_row(&json);

        assert_eq!(classification, RowClassification::Invalid);
    }

    #[test]
    fn unsupported_resolution_summary_version_is_unknown_schema() {
        let example = example_row("resolution_summary");
        let json = example.replacen(r#""v":1"#, r#""v":2"#, 1);

        let classification = classify_row(&json);

        assert_eq!(classification, RowClassification::UnknownSchema);
    }

    #[test]
    fn resolution_summary_with_unknown_field_is_invalid() {
        let example = example_row("resolution_summary");
        let json = example.replacen(r#""trigger""#, r#""future_field":true,"trigger""#, 1);

        let classification = classify_row(&json);

        assert_eq!(classification, RowClassification::Invalid);
    }

    #[test]
    fn resolution_summary_with_mismatched_unnamed_count_is_invalid() {
        let example = example_row("resolution_summary");
        let json = example.replacen(r#""unnamed_packages":1"#, r#""unnamed_packages":2"#, 1);

        let classification = classify_row(&json);

        assert_eq!(classification, RowClassification::Invalid);
    }

    #[test]
    fn unsupported_agent_configuration_version_is_unknown_schema() {
        let example = example_row("agent_configuration");
        let json = example.replacen(r#""v":1"#, r#""v":2"#, 1);

        let classification = classify_row(&json);

        assert_eq!(classification, RowClassification::UnknownSchema);
    }

    #[test]
    fn agent_configuration_with_unknown_field_is_invalid() {
        let example = example_row("agent_configuration");
        let json = example.replacen(r#""configured""#, r#""future_field":true,"configured""#, 1);

        let classification = classify_row(&json);

        assert_eq!(classification, RowClassification::Invalid);
    }

    #[test]
    fn new_storage_limit_uses_fixed_common_fields() {
        let day = UtcDay(NaiveDate::from_ymd_opt(2026, 8, 3).unwrap());

        let row = StorageLimitV1::new(day, DroppedOperation::ManualSync);

        assert_eq!(row.version, SchemaVersion::V1);
        assert_eq!(row.kind, RowKind::StorageLimit);
        assert_eq!(row.event_id.0.get_version(), Some(uuid::Version::Random));
        assert_eq!(row.day, day);
        assert_eq!(row.symposium, SymposiumVersion::current());
        assert_eq!(row.dropped_operation, DroppedOperation::ManualSync);
    }

    #[test]
    fn structurally_strict_row_rejects_future_version_when_deserialized_directly() {
        let example = example_row("storage_limit");
        let json = example.replacen(r#""v":1"#, r#""v":2"#, 1);

        let result = serde_json::from_str::<StorageLimitV1>(&json);

        assert!(result.is_err());
    }

    #[test]
    fn structurally_strict_row_rejects_wrong_kind_when_deserialized_directly() {
        let example = example_row("storage_limit");
        let json = example.replacen(r#""kind":"storage_limit""#, r#""kind":"command""#, 1);

        let result = serde_json::from_str::<StorageLimitV1>(&json);

        assert!(result.is_err());
    }

    #[test]
    fn dropped_operations_round_trip_with_contract_names() {
        let cases = [
            (DroppedOperation::SessionStart, "session_start"),
            (DroppedOperation::ManualSync, "manual_sync"),
            (DroppedOperation::Use, "use"),
            (DroppedOperation::Remove, "remove"),
            (DroppedOperation::Init, "init"),
            (DroppedOperation::Configuration, "configuration"),
            (DroppedOperation::Command, "command"),
        ];

        assert_contract_names(&cases);
    }

    #[test]
    fn unsupported_storage_limit_versions_are_unknown_schema() {
        let example = example_row("storage_limit");

        for version in [0, 2] {
            let json = example.replacen(r#""v":1"#, &format!(r#""v":{version}"#), 1);

            assert_eq!(classify_row(&json), RowClassification::UnknownSchema);
        }
    }

    #[test]
    fn unknown_row_kind_is_unknown_schema() {
        let json = r#"{"v":1,"kind":"future_kind","future_field":true}"#;

        let classification = classify_row(json);

        assert_eq!(classification, RowClassification::UnknownSchema);
    }

    #[test]
    fn recognized_schema_with_unknown_field_is_invalid() {
        let example = example_row("storage_limit");
        let json = example.replacen(
            r#""dropped_operation""#,
            r#""at":"2026-08-03T10:02:11Z","dropped_operation""#,
            1,
        );

        let classification = classify_row(&json);

        assert_eq!(classification, RowClassification::Invalid);
    }

    #[test]
    fn unusable_row_envelopes_are_malformed() {
        let cases = [
            "not JSON",
            "[]",
            "{}",
            r#"{"v":"1","kind":"storage_limit"}"#,
            r#"{"v":1}"#,
            r#"{"v":1,"v":2,"kind":"storage_limit"}"#,
        ];

        for line in cases {
            assert_eq!(
                classify_row(line),
                RowClassification::Malformed,
                "accepted unusable envelope {line}"
            );
        }
    }
}
