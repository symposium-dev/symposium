//! Extension-invocation aggregate row and strict read-side validation.

mod update;

use std::fmt;

use super::{ExtensionInvocationAgent, ExtensionTargetScope, UnnamedExtensionReason};
use crate::telemetry::identity::ExtensionSubject;

use super::super::{
    RowKind, SymposiumVersion,
    macros::strict_versioned_row,
    metrics::{
        ExtensionSessionCountError, ExtensionSessionCountInput, validate_extension_session_counts,
    },
    resolution::extension::PublicSkillCoordinate,
};

strict_versioned_row! {
    /// Version 1 daily aggregate for one skill-invocation bucket.
    pub(in crate::telemetry) struct ExtensionInvocationMetricsV1 {
        symposium: SymposiumVersion,
        agent: ExtensionInvocationAgent,
        target_scope: ExtensionTargetScope,
        #[serde(skip_serializing_if = "Option::is_none")]
        target: Option<PublicSkillCoordinate>,
        #[serde(skip_serializing_if = "Option::is_none")]
        unnamed_reason: Option<UnnamedExtensionReason>,
        attempted: u64,
        completed: u64,
        failed: u64,
        // This order deliberately follows the contract example rather than
        // the other aggregate rows.
        session_counts_complete: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        identified_sessions: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        identified_sessions_completed: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        extension_subject: Option<ExtensionSubject>,
    }

    kind: RowKind::ExtensionInvocationMetrics,
    raw: RawExtensionInvocationMetricsV1,
    validate: validate_extension_invocation_metrics,
}

fn validate_extension_invocation_metrics(
    raw: &RawExtensionInvocationMetricsV1,
) -> Result<(), ExtensionInvocationMetricsError> {
    if raw.attempted == 0 && raw.completed == 0 && raw.failed == 0 {
        return Err(ExtensionInvocationMetricsError::NoObservations);
    }

    let target_present = raw.target.is_some();
    let unnamed_reason_present = raw.unnamed_reason.is_some();
    let subject_present = raw.extension_subject.is_some();
    let identity_matches_scope = match raw.target_scope {
        ExtensionTargetScope::Public => {
            target_present && !unnamed_reason_present && subject_present
        }
        ExtensionTargetScope::Unnamed => {
            !target_present && unnamed_reason_present && !subject_present
        }
        ExtensionTargetScope::Overflow => {
            !target_present && !unnamed_reason_present && !subject_present
        }
    };
    if !identity_matches_scope {
        return Err(
            ExtensionInvocationMetricsError::TargetIdentityDoesNotMatchScope {
                scope: raw.target_scope,
                target_present,
                unnamed_reason_present,
                subject_present,
            },
        );
    }

    validate_extension_session_counts(ExtensionSessionCountInput {
        counts_complete: raw.session_counts_complete,
        identified_sessions: raw.identified_sessions,
        identified_sessions_completed: raw.identified_sessions_completed,
        attempted: raw.attempted,
        completed: raw.completed,
    })?;

    Ok(())
}

/// Invalid relationship between fields in an extension-invocation row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExtensionInvocationMetricsError {
    NoObservations,
    TargetIdentityDoesNotMatchScope {
        scope: ExtensionTargetScope,
        target_present: bool,
        unnamed_reason_present: bool,
        subject_present: bool,
    },
    SessionCounts(ExtensionSessionCountError),
}

impl From<ExtensionSessionCountError> for ExtensionInvocationMetricsError {
    fn from(error: ExtensionSessionCountError) -> Self {
        Self::SessionCounts(error)
    }
}

impl fmt::Display for ExtensionInvocationMetricsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoObservations => {
                formatter.write_str("extension-invocation metrics row has no observations")
            }
            Self::TargetIdentityDoesNotMatchScope {
                scope,
                target_present,
                unnamed_reason_present,
                subject_present,
            } => write!(
                formatter,
                "target, unnamed reason, and subject presence do not match target scope {} \
                 (target present: {target_present}, unnamed reason present: \
                 {unnamed_reason_present}, subject present: {subject_present})",
                scope.as_str()
            ),
            Self::SessionCounts(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ExtensionInvocationMetricsError {}

#[cfg(test)]
mod tests;
