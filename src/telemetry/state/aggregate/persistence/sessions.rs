//! Wire conversion for bounded private session pairs.

#[cfg(test)]
mod tests;

use std::collections::BTreeSet;

use super::AggregateStateInvariantError;
use crate::telemetry::{
    identity::SessionId,
    schema::MAX_IDENTIFIED_SESSIONS,
    state::{aggregate::AggregateFamily, session_pair::TrackedSessionPair},
};

/// Borrowed session fields used by each aggregate family's wire entry.
///
/// Keeping this value free of Serde lets each family retain its meaningful
/// field names while sharing the complete/incomplete representation.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "used by the next persistence commit")
)]
pub(super) struct SessionPairRef<'a> {
    pub(super) complete: bool,
    pub(super) first: Option<&'a BTreeSet<SessionId>>,
    pub(super) second: Option<&'a BTreeSet<SessionId>>,
}

impl<'a> From<&'a TrackedSessionPair> for SessionPairRef<'a> {
    fn from(pair: &'a TrackedSessionPair) -> Self {
        match pair {
            TrackedSessionPair::Complete { first, second } => Self {
                complete: true,
                first: Some(first),
                second: Some(second),
            },
            TrackedSessionPair::Incomplete => Self {
                complete: false,
                first: None,
                second: None,
            },
        }
    }
}

#[derive(Clone, Copy)]
enum SessionContributionCounts {
    Hook(u64),
    PluginHook(u64),
    ExtensionInvocation { attempted: u64, completed: u64 },
}

impl SessionContributionCounts {
    const fn family(self) -> AggregateFamily {
        match self {
            Self::Hook(_) => AggregateFamily::Hook,
            Self::PluginHook(_) => AggregateFamily::PluginHook,
            Self::ExtensionInvocation { .. } => AggregateFamily::ExtensionInvocation,
        }
    }

    const fn requires_second_subset(self) -> bool {
        matches!(self, Self::Hook(_) | Self::PluginHook(_))
    }
}

/// Decode and validate one top-level hook tracker.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "used by the next persistence commit")
)]
pub(super) fn decode_hook(
    complete: bool,
    identified: Option<Vec<SessionId>>,
    non_ok: Option<Vec<SessionId>>,
    contributions: u64,
) -> Result<TrackedSessionPair, AggregateStateInvariantError> {
    decode_entry(
        SessionContributionCounts::Hook(contributions),
        complete,
        identified,
        non_ok,
    )
}

/// Decode and validate one plugin-hook tracker.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "used by the next persistence commit")
)]
pub(super) fn decode_plugin_hook(
    complete: bool,
    identified: Option<Vec<SessionId>>,
    non_ok: Option<Vec<SessionId>>,
    contributions: u64,
) -> Result<TrackedSessionPair, AggregateStateInvariantError> {
    decode_entry(
        SessionContributionCounts::PluginHook(contributions),
        complete,
        identified,
        non_ok,
    )
}

/// Decode and validate one extension-invocation tracker.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "used by the next persistence commit")
)]
pub(super) fn decode_extension_invocation(
    complete: bool,
    attempted_sessions: Option<Vec<SessionId>>,
    completed_sessions: Option<Vec<SessionId>>,
    attempted_contributions: u64,
    completed_contributions: u64,
) -> Result<TrackedSessionPair, AggregateStateInvariantError> {
    decode_entry(
        SessionContributionCounts::ExtensionInvocation {
            attempted: attempted_contributions,
            completed: completed_contributions,
        },
        complete,
        attempted_sessions,
        completed_sessions,
    )
}

/// Validate one top-level hook tracker before encoding it.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "used by the next persistence commit")
)]
pub(super) fn validate_hook(
    pair: &TrackedSessionPair,
    contributions: u64,
) -> Result<(), AggregateStateInvariantError> {
    validate_entry(SessionContributionCounts::Hook(contributions), pair)
}

/// Validate one plugin-hook tracker before encoding it.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "used by the next persistence commit")
)]
pub(super) fn validate_plugin_hook(
    pair: &TrackedSessionPair,
    contributions: u64,
) -> Result<(), AggregateStateInvariantError> {
    validate_entry(SessionContributionCounts::PluginHook(contributions), pair)
}

/// Validate one extension-invocation tracker before encoding it.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "used by the next persistence commit")
)]
pub(super) fn validate_extension_invocation(
    pair: &TrackedSessionPair,
    attempted_contributions: u64,
    completed_contributions: u64,
) -> Result<(), AggregateStateInvariantError> {
    validate_entry(
        SessionContributionCounts::ExtensionInvocation {
            attempted: attempted_contributions,
            completed: completed_contributions,
        },
        pair,
    )
}

fn decode_entry(
    contributions: SessionContributionCounts,
    complete: bool,
    first: Option<Vec<SessionId>>,
    second: Option<Vec<SessionId>>,
) -> Result<TrackedSessionPair, AggregateStateInvariantError> {
    let pair = decode_raw(contributions.family(), complete, first, second)?;
    validate_entry(contributions, &pair)?;
    Ok(pair)
}

fn decode_raw(
    family: AggregateFamily,
    complete: bool,
    first: Option<Vec<SessionId>>,
    second: Option<Vec<SessionId>>,
) -> Result<TrackedSessionPair, AggregateStateInvariantError> {
    match (complete, first, second) {
        (false, None, None) => Ok(TrackedSessionPair::Incomplete),
        (true, Some(first), Some(second)) => Ok(TrackedSessionPair::Complete {
            first: decode_set(family, first)?,
            second: decode_set(family, second)?,
        }),
        _ => Err(AggregateStateInvariantError::InvalidSessionShape { family }),
    }
}

fn validate_entry(
    contributions: SessionContributionCounts,
    pair: &TrackedSessionPair,
) -> Result<(), AggregateStateInvariantError> {
    let TrackedSessionPair::Complete { first, second } = pair else {
        return Ok(());
    };
    let family = contributions.family();
    if first.len() > max_sessions() || second.len() > max_sessions() {
        return Err(AggregateStateInvariantError::TooManySessions { family });
    }
    if contributions.requires_second_subset() && !second.is_subset(first) {
        return Err(AggregateStateInvariantError::InvalidSessionRelationship { family });
    }

    let contribution_counts_match = match contributions {
        SessionContributionCounts::Hook(count) | SessionContributionCounts::PluginHook(count) => {
            !set_disagrees_with_contributions(first, count)
        }
        SessionContributionCounts::ExtensionInvocation {
            attempted,
            completed,
        } => {
            !set_disagrees_with_contributions(first, attempted)
                && !set_disagrees_with_contributions(second, completed)
        }
    };
    if !contribution_counts_match {
        return Err(AggregateStateInvariantError::InvalidSessionContributionCount { family });
    }

    Ok(())
}

fn set_disagrees_with_contributions(sessions: &BTreeSet<SessionId>, contributions: u64) -> bool {
    let session_count = u64::try_from(sessions.len()).unwrap_or(u64::MAX);
    session_count > contributions || (contributions > 0 && sessions.is_empty())
}

fn decode_set(
    family: AggregateFamily,
    values: Vec<SessionId>,
) -> Result<BTreeSet<SessionId>, AggregateStateInvariantError> {
    let original_len = values.len();
    let sessions = values.into_iter().collect::<BTreeSet<_>>();
    if sessions.len() != original_len {
        return Err(AggregateStateInvariantError::DuplicateSession { family });
    }

    Ok(sessions)
}

fn max_sessions() -> usize {
    usize::try_from(MAX_IDENTIFIED_SESSIONS)
        .expect("BUG: the session-set limit must fit usize on supported targets")
}
