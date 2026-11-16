//! What a result records of a concern: a stable `reason` code for scripts, and a
//! `detail` for people, which may change.

use serde::Serialize;

use super::capped;
use super::expectations::event_name;
use crate::expect::crosscheck::{Concern, Reason};

#[derive(Debug, Serialize)]
pub(super) struct ConcernRecord {
    kind: ConcernKind,
    reason: ReasonCode,
    detail: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum ConcernKind {
    Contradiction,
    Uncertainty,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum ReasonCode {
    HookAbsent,
    HookDidNotSucceed,
    HookSucceeded,
    HookRecordIncomplete,
    ExtraHookRecords,
    HookOutcomeUnknown,
    Mutated,
    MutationUnsettled,
    UnplacedMutation,
    CaseOnlyMatch,
    ActivityIncomplete,
    CommandNamesProtected,
    CommandTruncated,
    ConfigChanged,
}

impl From<&Concern> for ConcernRecord {
    fn from(concern: &Concern) -> Self {
        let (kind, reason) = match concern {
            Concern::Contradiction(reason) => (ConcernKind::Contradiction, reason),
            Concern::Uncertainty(reason) => (ConcernKind::Uncertainty, reason),
        };
        Self {
            kind,
            reason: ReasonCode::from(reason),
            detail: capped(detail(reason)),
        }
    }
}

impl From<&Reason> for ReasonCode {
    fn from(reason: &Reason) -> Self {
        match reason {
            Reason::HookAbsent { .. } => Self::HookAbsent,
            Reason::HookDidNotSucceed { .. } => Self::HookDidNotSucceed,
            Reason::HookSucceeded { .. } => Self::HookSucceeded,
            Reason::HookRecordIncomplete { .. } => Self::HookRecordIncomplete,
            Reason::ExtraHookRecords { .. } => Self::ExtraHookRecords,
            Reason::HookOutcomeUnknown { .. } => Self::HookOutcomeUnknown,
            Reason::Mutated { .. } => Self::Mutated,
            Reason::MutationUnsettled { .. } => Self::MutationUnsettled,
            Reason::UnplacedMutation { .. } => Self::UnplacedMutation,
            Reason::CaseOnlyMatch { .. } => Self::CaseOnlyMatch,
            Reason::ActivityIncomplete => Self::ActivityIncomplete,
            Reason::CommandNamesProtected { .. } => Self::CommandNamesProtected,
            Reason::CommandTruncated { .. } => Self::CommandTruncated,
            Reason::ConfigChanged(_) => Self::ConfigChanged,
        }
    }
}

/// What a person needs to follow up on `reason`.
fn detail(reason: &Reason) -> String {
    match reason {
        Reason::HookAbsent { event }
        | Reason::HookSucceeded { event }
        | Reason::HookRecordIncomplete { event }
        | Reason::HookOutcomeUnknown { event } => event_name(event).to_owned(),
        Reason::HookDidNotSucceed { event, results } => {
            format!("{}: {results:?}", event_name(event))
        }
        Reason::ExtraHookRecords {
            event,
            records,
            runs,
        } => format!("records: {records}, runs of {}: {runs}", event_name(event)),
        Reason::Mutated { protected, path } => format!("{protected:?} at {}", path.display()),
        Reason::MutationUnsettled {
            protected,
            path,
            result,
        }
        | Reason::CaseOnlyMatch {
            protected,
            path,
            result,
        } => format!("{protected:?} at {} ({result:?})", path.display()),
        Reason::UnplacedMutation { reported, result } => {
            let path = reported.as_deref().unwrap_or("an unreadable path");
            format!("{path} ({result:?})")
        }
        Reason::ActivityIncomplete => String::new(),
        Reason::CommandNamesProtected {
            protected,
            command,
            result,
        } => format!("{protected:?}: {} ({result:?})", command.prefix),
        Reason::CommandTruncated { command, result } => format!(
            "{} of {} bytes kept ({result:?})",
            command.prefix.len(),
            command.original_bytes
        ),
        Reason::ConfigChanged(change) => {
            format!("{:?} {} {:?}", change.root, change.file, change.change)
        }
    }
}
