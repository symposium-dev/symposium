//! Schema types for resolution telemetry.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::{
    DroppedOperation, EventId, RowKind, SchemaVersion, SymposiumVersion, UtcDay,
    deserialize_version_one,
};
use crate::telemetry::identity::SessionId;

/// Operation that caused a full resolution and sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::telemetry) enum ResolutionTrigger {
    SessionStart,
    ManualSync,
    Use,
    Remove,
}

impl From<ResolutionTrigger> for DroppedOperation {
    fn from(trigger: ResolutionTrigger) -> Self {
        match trigger {
            ResolutionTrigger::SessionStart => Self::SessionStart,
            ResolutionTrigger::ManualSync => Self::ManualSync,
            ResolutionTrigger::Use => Self::Use,
            ResolutionTrigger::Remove => Self::Remove,
        }
    }
}

/// Result of a completed full resolution and sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::telemetry) enum ResolutionOutcome {
    Ok,
    Partial,
    Error,
}

/// Public package ecosystem approved for version 1 telemetry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::telemetry) enum PackageEcosystem {
    Cargo,
}

/// Kind of extension content contributed by one public package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::telemetry) enum ExtensionMatch {
    Public,
    UnnamedOnly,
    None,
}

/// One reason that a package coordinate cannot be named.
///
/// The public-identity policy selects this reason after applying source
/// provenance precedence. Recording accepts one selected reason at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::telemetry) enum UnnamedPackageReason {
    PrivateRegistry,
    Git,
    Path,
    Workspace,
    UnknownSource,
    InvalidCoordinate,
}

/// Mutually exclusive reasons that package coordinates cannot be named.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::telemetry) struct UnnamedPackageReasons {
    private_registry: u64,
    git: u64,
    path: u64,
    workspace: u64,
    unknown_source: u64,
    invalid_coordinate: u64,
}

impl UnnamedPackageReasons {
    /// Increment exactly one reason counter.
    ///
    /// # Errors
    ///
    /// Returns [`ResolutionSummaryError::UnnamedPackageCountOverflow`] when
    /// the selected counter cannot be incremented.
    #[must_use = "counter overflow must drop the containing telemetry batch"]
    pub(in crate::telemetry) fn checked_record(
        &mut self,
        reason: UnnamedPackageReason,
    ) -> Result<(), ResolutionSummaryError> {
        let counter = match reason {
            UnnamedPackageReason::PrivateRegistry => &mut self.private_registry,
            UnnamedPackageReason::Git => &mut self.git,
            UnnamedPackageReason::Path => &mut self.path,
            UnnamedPackageReason::Workspace => &mut self.workspace,
            UnnamedPackageReason::UnknownSource => &mut self.unknown_source,
            UnnamedPackageReason::InvalidCoordinate => &mut self.invalid_coordinate,
        };

        *counter = counter
            .checked_add(1)
            .ok_or(ResolutionSummaryError::UnnamedPackageCountOverflow)?;
        Ok(())
    }

    /// Return the total number of unnamed packages, or `None` on overflow.
    #[must_use]
    fn checked_total(self) -> Option<u64> {
        [
            self.private_registry,
            self.git,
            self.path,
            self.workspace,
            self.unknown_source,
            self.invalid_coordinate,
        ]
        .into_iter()
        .try_fold(0_u64, u64::checked_add)
    }
}

/// Fields supplied by a completed full resolution and sync.
///
/// These fields are repeated on [`ResolutionSummaryV1`] because flattening
/// this constructor input would weaken strict unknown-field rejection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) struct ResolutionSummaryFields {
    pub(in crate::telemetry) trigger: ResolutionTrigger,
    pub(in crate::telemetry) outcome: ResolutionOutcome,
    pub(in crate::telemetry) duration_ms: u64,
    pub(in crate::telemetry) public_packages: u64,
    pub(in crate::telemetry) unnamed_package_reasons: UnnamedPackageReasons,
    pub(in crate::telemetry) plugins: u64,
    pub(in crate::telemetry) skills: u64,
    pub(in crate::telemetry) installed: u64,
    pub(in crate::telemetry) updated: u64,
    pub(in crate::telemetry) reaped: u64,
    pub(in crate::telemetry) session_id: Option<SessionId>,
}

/// Version 1 summary of one completed full resolution and sync.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawResolutionSummaryV1")]
pub(in crate::telemetry) struct ResolutionSummaryV1 {
    #[serde(rename = "v")]
    version: SchemaVersion,
    kind: RowKind,
    event_id: EventId,
    day: UtcDay,
    symposium: SymposiumVersion,
    trigger: ResolutionTrigger,
    outcome: ResolutionOutcome,
    duration_ms: u64,
    public_packages: u64,
    unnamed_packages: u64,
    unnamed_package_reasons: UnnamedPackageReasons,
    plugins: u64,
    skills: u64,
    installed: u64,
    updated: u64,
    reaped: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<SessionId>,
}

impl ResolutionSummaryV1 {
    /// Create a summary for one completed full resolution and sync.
    ///
    /// # Errors
    ///
    /// Returns [`ResolutionSummaryError::UnnamedPackageCountOverflow`] when
    /// the unnamed-package reason counters cannot be represented by `u64`.
    pub(in crate::telemetry) fn new(
        day: UtcDay,
        fields: ResolutionSummaryFields,
    ) -> Result<Self, ResolutionSummaryError> {
        let unnamed_packages = fields
            .unnamed_package_reasons
            .checked_total()
            .ok_or(ResolutionSummaryError::UnnamedPackageCountOverflow)?;

        Ok(Self {
            version: SchemaVersion::V1,
            kind: RowKind::ResolutionSummary,
            event_id: EventId::new(),
            day,
            symposium: SymposiumVersion::current(),
            trigger: fields.trigger,
            outcome: fields.outcome,
            duration_ms: fields.duration_ms,
            public_packages: fields.public_packages,
            unnamed_packages,
            unnamed_package_reasons: fields.unnamed_package_reasons,
            plugins: fields.plugins,
            skills: fields.skills,
            installed: fields.installed,
            updated: fields.updated,
            reaped: fields.reaped,
            session_id: fields.session_id,
        })
    }
}

/// Invalid relationship between fields in a resolution summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum ResolutionSummaryError {
    UnnamedPackageCountOverflow,
    UnnamedPackageCountMismatch { stored: u64, derived: u64 },
}

impl fmt::Display for ResolutionSummaryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnnamedPackageCountOverflow => {
                formatter.write_str("unnamed package reason counters overflow u64")
            }
            Self::UnnamedPackageCountMismatch { stored, derived } => write!(
                formatter,
                "stored unnamed package count {stored} does not match derived reason count {derived}"
            ),
        }
    }
}

impl std::error::Error for ResolutionSummaryError {}

/// Strict wire representation validated before becoming a resolution summary.
///
/// Serde's `try_from` deserializes this type rather than the outer row, so its
/// version and unknown-field checks are deliberately declared here.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawResolutionSummaryV1 {
    #[serde(rename = "v", deserialize_with = "deserialize_version_one")]
    version: SchemaVersion,
    kind: RowKind,
    event_id: EventId,
    day: UtcDay,
    symposium: SymposiumVersion,
    trigger: ResolutionTrigger,
    outcome: ResolutionOutcome,
    duration_ms: u64,
    public_packages: u64,
    unnamed_packages: u64,
    unnamed_package_reasons: UnnamedPackageReasons,
    plugins: u64,
    skills: u64,
    installed: u64,
    updated: u64,
    reaped: u64,
    session_id: Option<SessionId>,
}

impl TryFrom<RawResolutionSummaryV1> for ResolutionSummaryV1 {
    type Error = ResolutionSummaryError;

    fn try_from(raw: RawResolutionSummaryV1) -> Result<Self, Self::Error> {
        let derived = raw
            .unnamed_package_reasons
            .checked_total()
            .ok_or(ResolutionSummaryError::UnnamedPackageCountOverflow)?;

        if raw.unnamed_packages != derived {
            return Err(ResolutionSummaryError::UnnamedPackageCountMismatch {
                stored: raw.unnamed_packages,
                derived,
            });
        }

        Ok(Self {
            version: raw.version,
            kind: raw.kind,
            event_id: raw.event_id,
            day: raw.day,
            symposium: raw.symposium,
            trigger: raw.trigger,
            outcome: raw.outcome,
            duration_ms: raw.duration_ms,
            public_packages: raw.public_packages,
            unnamed_packages: raw.unnamed_packages,
            unnamed_package_reasons: raw.unnamed_package_reasons,
            plugins: raw.plugins,
            skills: raw.skills,
            installed: raw.installed,
            updated: raw.updated,
            reaped: raw.reaped,
            session_id: raw.session_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    fn example_reasons() -> UnnamedPackageReasons {
        UnnamedPackageReasons {
            private_registry: 1,
            git: 2,
            path: 3,
            workspace: 4,
            unknown_source: 5,
            invalid_coordinate: 6,
        }
    }

    fn summary_day() -> UtcDay {
        UtcDay::from_date(NaiveDate::from_ymd_opt(2026, 8, 3).unwrap())
    }

    fn summary_fields(session_id: Option<SessionId>) -> ResolutionSummaryFields {
        ResolutionSummaryFields {
            trigger: ResolutionTrigger::SessionStart,
            outcome: ResolutionOutcome::Ok,
            duration_ms: 142,
            public_packages: 2,
            unnamed_package_reasons: UnnamedPackageReasons {
                private_registry: 1,
                ..UnnamedPackageReasons::default()
            },
            plugins: 1,
            skills: 1,
            installed: 2,
            updated: 0,
            reaped: 0,
            session_id,
        }
    }

    fn raw_summary(
        unnamed_packages: u64,
        unnamed_package_reasons: UnnamedPackageReasons,
    ) -> RawResolutionSummaryV1 {
        RawResolutionSummaryV1 {
            version: SchemaVersion::V1,
            kind: RowKind::ResolutionSummary,
            event_id: EventId::new(),
            day: summary_day(),
            symposium: SymposiumVersion::current(),
            trigger: ResolutionTrigger::SessionStart,
            outcome: ResolutionOutcome::Ok,
            duration_ms: 142,
            public_packages: 2,
            unnamed_packages,
            unnamed_package_reasons,
            plugins: 1,
            skills: 1,
            installed: 2,
            updated: 0,
            reaped: 0,
            session_id: None,
        }
    }

    #[test]
    fn new_resolution_summary_derives_fixed_fields_and_unnamed_total() {
        let session_id = "sess_31d8b1916028f65a0c0521dc1f4c86fb".parse().unwrap();
        let fields = summary_fields(Some(session_id));

        let row = ResolutionSummaryV1::new(summary_day(), fields).unwrap();

        assert_eq!(row.version, SchemaVersion::V1);
        assert_eq!(row.kind, RowKind::ResolutionSummary);
        assert_eq!(row.event_id.0.get_version(), Some(uuid::Version::Random));
        assert_eq!(row.day, summary_day());
        assert_eq!(row.symposium, SymposiumVersion::current());
        assert_eq!(row.trigger, fields.trigger);
        assert_eq!(row.outcome, fields.outcome);
        assert_eq!(row.duration_ms, fields.duration_ms);
        assert_eq!(row.public_packages, fields.public_packages);
        assert_eq!(row.unnamed_packages, 1);
        assert_eq!(row.unnamed_package_reasons, fields.unnamed_package_reasons);
        assert_eq!(row.plugins, fields.plugins);
        assert_eq!(row.skills, fields.skills);
        assert_eq!(row.installed, fields.installed);
        assert_eq!(row.updated, fields.updated);
        assert_eq!(row.reaped, fields.reaped);
        assert_eq!(row.session_id, fields.session_id);
    }

    #[test]
    fn new_resolution_summary_reports_reason_counter_overflow() {
        let mut fields = summary_fields(None);
        fields.unnamed_package_reasons = UnnamedPackageReasons {
            private_registry: u64::MAX,
            git: 1,
            ..UnnamedPackageReasons::default()
        };

        let result = ResolutionSummaryV1::new(summary_day(), fields);

        assert_eq!(
            result,
            Err(ResolutionSummaryError::UnnamedPackageCountOverflow)
        );
    }

    #[test]
    fn direct_resolution_summary_deserialization_rejects_future_version() {
        let row = ResolutionSummaryV1::new(summary_day(), summary_fields(None)).unwrap();
        let json = serde_json::to_string(&row).unwrap();
        let future = json.replacen(r#""v":1"#, r#""v":2"#, 1);

        let result = serde_json::from_str::<ResolutionSummaryV1>(&future);

        assert!(result.is_err());
    }

    #[test]
    fn resolution_summary_rejects_unknown_fields() {
        let row = ResolutionSummaryV1::new(summary_day(), summary_fields(None)).unwrap();
        let json = serde_json::to_string(&row).unwrap();
        let unknown = json.replacen(r#""trigger""#, r#""future_field":true,"trigger""#, 1);

        let result = serde_json::from_str::<ResolutionSummaryV1>(&unknown);

        assert!(result.is_err());
    }

    #[test]
    fn resolution_summary_requires_every_top_level_field() {
        let row = ResolutionSummaryV1::new(summary_day(), summary_fields(None)).unwrap();
        let json = serde_json::to_string(&row).unwrap();
        let missing = json.replacen(r#","plugins":1"#, "", 1);

        let result = serde_json::from_str::<ResolutionSummaryV1>(&missing);

        assert!(result.is_err());
    }

    #[test]
    fn resolution_summary_json_rejects_mismatched_unnamed_count() {
        let row = ResolutionSummaryV1::new(summary_day(), summary_fields(None)).unwrap();
        let json = serde_json::to_string(&row).unwrap();
        let mismatched = json.replacen(r#""unnamed_packages":1"#, r#""unnamed_packages":2"#, 1);

        let error = serde_json::from_str::<ResolutionSummaryV1>(&mismatched).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("stored unnamed package count 2 does not match derived reason count 1")
        );
    }

    #[test]
    fn resolution_summary_rejects_mismatched_unnamed_count() {
        let raw = raw_summary(20, example_reasons());

        let result = ResolutionSummaryV1::try_from(raw);

        assert_eq!(
            result,
            Err(ResolutionSummaryError::UnnamedPackageCountMismatch {
                stored: 20,
                derived: 21,
            })
        );
    }

    #[test]
    fn resolution_summary_validation_reports_reason_counter_overflow() {
        let reasons = UnnamedPackageReasons {
            private_registry: u64::MAX,
            git: 1,
            ..UnnamedPackageReasons::default()
        };
        let raw = raw_summary(u64::MAX, reasons);

        let result = ResolutionSummaryV1::try_from(raw);

        assert_eq!(
            result,
            Err(ResolutionSummaryError::UnnamedPackageCountOverflow)
        );
    }

    #[test]
    fn resolution_summary_without_session_id_round_trips_without_the_field() {
        let row = ResolutionSummaryV1::new(summary_day(), summary_fields(None)).unwrap();

        let json = serde_json::to_string(&row).unwrap();
        let value = serde_json::from_str::<serde_json::Value>(&json).unwrap();
        let decoded = serde_json::from_str::<ResolutionSummaryV1>(&json).unwrap();

        assert_eq!(value.get("session_id"), None);
        assert_eq!(decoded, row);
    }

    #[test]
    fn resolution_triggers_round_trip_and_match_storage_names() {
        let cases = [
            (ResolutionTrigger::SessionStart, "session_start"),
            (ResolutionTrigger::ManualSync, "manual_sync"),
            (ResolutionTrigger::Use, "use"),
            (ResolutionTrigger::Remove, "remove"),
        ];

        for (trigger, name) in cases {
            let json = serde_json::to_string(&trigger).unwrap();
            let decoded = serde_json::from_str::<ResolutionTrigger>(&json).unwrap();
            let dropped_operation =
                serde_json::to_string(&DroppedOperation::from(trigger)).unwrap();

            assert_eq!(json, format!(r#""{name}""#));
            assert_eq!(decoded, trigger);
            assert_eq!(dropped_operation, json);
        }
    }

    #[test]
    fn resolution_outcomes_round_trip_with_contract_names() {
        let cases = [
            (ResolutionOutcome::Ok, "ok"),
            (ResolutionOutcome::Partial, "partial"),
            (ResolutionOutcome::Error, "error"),
        ];

        for (outcome, name) in cases {
            let json = serde_json::to_string(&outcome).unwrap();
            let decoded = serde_json::from_str::<ResolutionOutcome>(&json).unwrap();

            assert_eq!(json, format!(r#""{name}""#));
            assert_eq!(decoded, outcome);
        }
    }

    #[test]
    fn package_ecosystems_round_trip_with_contract_names() {
        let cases = [(PackageEcosystem::Cargo, "cargo")];

        for (ecosystem, name) in cases {
            let json = serde_json::to_string(&ecosystem).unwrap();
            let decoded = serde_json::from_str::<PackageEcosystem>(&json).unwrap();

            assert_eq!(json, format!(r#""{name}""#));
            assert_eq!(decoded, ecosystem);
        }
    }

    #[test]
    fn extension_matches_round_trip_with_contract_names() {
        let cases = [
            (ExtensionMatch::Public, "public"),
            (ExtensionMatch::UnnamedOnly, "unnamed_only"),
            (ExtensionMatch::None, "none"),
        ];

        for (extension_match, name) in cases {
            let json = serde_json::to_string(&extension_match).unwrap();
            let decoded = serde_json::from_str::<ExtensionMatch>(&json).unwrap();

            assert_eq!(json, format!(r#""{name}""#));
            assert_eq!(decoded, extension_match);
        }
    }

    #[test]
    fn resolution_vocabulary_rejects_unknown_contract_names() {
        let unknown = r#""future_value""#;

        let trigger = serde_json::from_str::<ResolutionTrigger>(unknown);
        let outcome = serde_json::from_str::<ResolutionOutcome>(unknown);
        let ecosystem = serde_json::from_str::<PackageEcosystem>(unknown);
        let extension_match = serde_json::from_str::<ExtensionMatch>(unknown);

        assert!(trigger.is_err());
        assert!(outcome.is_err());
        assert!(ecosystem.is_err());
        assert!(extension_match.is_err());
    }

    #[test]
    fn unnamed_package_reasons_round_trip_in_contract_order() {
        let reasons = example_reasons();

        let json = serde_json::to_string(&reasons).unwrap();
        let decoded = serde_json::from_str::<UnnamedPackageReasons>(&json).unwrap();

        assert_eq!(
            json,
            r#"{"private_registry":1,"git":2,"path":3,"workspace":4,"unknown_source":5,"invalid_coordinate":6}"#
        );
        assert_eq!(decoded, reasons);
    }

    #[test]
    fn unnamed_package_reasons_reject_unknown_fields() {
        let json = r#"{"private_registry":1,"git":2,"path":3,"workspace":4,"unknown_source":5,"invalid_coordinate":6,"future_source":7}"#;

        let result = serde_json::from_str::<UnnamedPackageReasons>(json);

        assert!(result.is_err());
    }

    #[test]
    fn unnamed_package_reasons_require_every_contract_field() {
        let json = r#"{"private_registry":1,"git":2,"path":3,"workspace":4,"unknown_source":5}"#;

        let result = serde_json::from_str::<UnnamedPackageReasons>(json);

        assert!(result.is_err());
    }

    #[test]
    fn unnamed_package_reason_total_uses_checked_arithmetic() {
        let reasons = example_reasons();

        let total = reasons.checked_total();

        assert_eq!(total, Some(21));
    }

    #[test]
    fn recording_a_reason_increments_only_its_counter() {
        let cases = [
            (
                UnnamedPackageReason::PrivateRegistry,
                UnnamedPackageReasons {
                    private_registry: 1,
                    ..UnnamedPackageReasons::default()
                },
            ),
            (
                UnnamedPackageReason::Git,
                UnnamedPackageReasons {
                    git: 1,
                    ..UnnamedPackageReasons::default()
                },
            ),
            (
                UnnamedPackageReason::Path,
                UnnamedPackageReasons {
                    path: 1,
                    ..UnnamedPackageReasons::default()
                },
            ),
            (
                UnnamedPackageReason::Workspace,
                UnnamedPackageReasons {
                    workspace: 1,
                    ..UnnamedPackageReasons::default()
                },
            ),
            (
                UnnamedPackageReason::UnknownSource,
                UnnamedPackageReasons {
                    unknown_source: 1,
                    ..UnnamedPackageReasons::default()
                },
            ),
            (
                UnnamedPackageReason::InvalidCoordinate,
                UnnamedPackageReasons {
                    invalid_coordinate: 1,
                    ..UnnamedPackageReasons::default()
                },
            ),
        ];

        for (reason, expected) in cases {
            let mut reasons = UnnamedPackageReasons::default();

            let recorded = reasons.checked_record(reason);

            assert_eq!(recorded, Ok(()));
            assert_eq!(reasons, expected);
        }
    }

    #[test]
    fn recording_a_reason_rejects_overflow_without_mutation() {
        let mut reasons = UnnamedPackageReasons {
            private_registry: u64::MAX,
            ..UnnamedPackageReasons::default()
        };
        let before = reasons;

        let recorded = reasons.checked_record(UnnamedPackageReason::PrivateRegistry);

        assert_eq!(
            recorded,
            Err(ResolutionSummaryError::UnnamedPackageCountOverflow)
        );
        assert_eq!(reasons, before);
    }

    #[test]
    fn unnamed_package_reason_total_rejects_overflow() {
        let reasons = UnnamedPackageReasons {
            private_registry: u64::MAX,
            git: 1,
            ..UnnamedPackageReasons::default()
        };

        let total = reasons.checked_total();

        assert_eq!(total, None);
    }
}
