//! Private wire conversion for aggregate telemetry state.
//!
//! Runtime trackers deliberately do not implement Serde: they contain keyed
//! session identifiers, and a general serializer would make accidental
//! disclosure easy. This module is the only bridge between those runtime
//! values and `telemetry-state.toml`.
//!
//! Borrowed store wire types declare scalar metadata before their entry
//! arrays. TOML renders those arrays as tables and cannot encode a later
//! scalar, reporting `ValueAfterTable` instead.
//!
//! Family validators reject duplicate row identifiers within one store. The
//! root aggregate-state validator owns cross-family identifier uniqueness and
//! agreement between every store day and the identity high-water day.
#[cfg(test)]
mod tests;

mod extension_invocation;
mod hook;
mod plugin_hook;
mod sessions;

use std::{collections::BTreeSet, error::Error, fmt};

use serde::{Deserialize, Serialize};

use super::{AggregateFamily, AggregateState};
use crate::telemetry::{identity::IdentifierWindowScope, schema::UtcDay};

/// Identity fields needed to validate the aggregate section in either direction.
#[derive(Clone, Copy)]
pub(super) struct AggregatePersistenceContext<'scope, 'identity> {
    scope: &'scope IdentifierWindowScope<'identity>,
    latest_opened_day: UtcDay,
}

impl<'scope, 'identity> AggregatePersistenceContext<'scope, 'identity> {
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the root state codec consumes this in the next stack branch"
        )
    )]
    pub(super) const fn new(
        scope: &'scope IdentifierWindowScope<'identity>,
        latest_opened_day: UtcDay,
    ) -> Self {
        Self {
            scope,
            latest_opened_day,
        }
    }
}

/// Borrowed aggregate private state in canonical wire order.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the root state codec consumes this in the next stack branch"
    )
)]
#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) struct AggregateStateRef<'a> {
    hook: hook::HookStoreRef<'a>,
    plugin_hook: plugin_hook::PluginHookStoreRef<'a>,
    extension_invocation: extension_invocation::ExtensionInvocationStoreRef<'a>,
}

impl<'store> TryFrom<(&'store AggregateState, AggregatePersistenceContext<'_, '_>)>
    for AggregateStateRef<'store>
{
    type Error = AggregateStateInvariantError;

    fn try_from(
        (state, context): (&'store AggregateState, AggregatePersistenceContext<'_, '_>),
    ) -> Result<Self, Self::Error> {
        validate_cross_sections(state, context.latest_opened_day)?;
        Ok(Self {
            hook: hook::HookStoreRef::try_from((&state.hook, context.scope))?,
            plugin_hook: plugin_hook::PluginHookStoreRef::try_from((
                &state.plugin_hook,
                context.scope,
            ))?,
            extension_invocation: extension_invocation::ExtensionInvocationStoreRef::try_from((
                &state.extension_invocation,
                context.scope,
            ))?,
        })
    }
}

/// Aggregate private state decoded before runtime keys are reconstructed.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the root state codec consumes this in the next stack branch"
    )
)]
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(super) struct RawAggregateState {
    hook: hook::RawHookStore,
    plugin_hook: plugin_hook::RawPluginHookStore,
    extension_invocation: extension_invocation::RawExtensionInvocationStore,
}

impl RawAggregateState {
    /// Rebuild all family stores, then validate their shared invariants.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the root state codec consumes this in the next stack branch"
        )
    )]
    pub(super) fn into_runtime(
        self,
        context: AggregatePersistenceContext<'_, '_>,
    ) -> Result<AggregateState, AggregateStateInvariantError> {
        let state = AggregateState {
            hook: self.hook.into_runtime(context.scope)?,
            plugin_hook: self.plugin_hook.into_runtime(context.scope)?,
            extension_invocation: self.extension_invocation.into_runtime(context.scope)?,
        };
        validate_cross_sections(&state, context.latest_opened_day)?;
        Ok(state)
    }
}

/// Validate invariants that span aggregate families or the identity section.
fn validate_cross_sections(
    state: &AggregateState,
    latest_opened_day: UtcDay,
) -> Result<(), AggregateStateInvariantError> {
    let store_days = [
        (AggregateFamily::Hook, state.hook.day()),
        (AggregateFamily::PluginHook, state.plugin_hook.day()),
        (
            AggregateFamily::ExtensionInvocation,
            state.extension_invocation.day(),
        ),
    ];
    for (family, day) in store_days {
        if day != latest_opened_day {
            return Err(AggregateStateInvariantError::StoreDayMismatch { family });
        }
    }

    // Hook private state has no event id. The two persisted event-id families
    // must be disjoint so one row can never be claimed by both trackers.
    let mut event_ids = BTreeSet::new();
    for (_, entry) in state.plugin_hook.persistence_entries() {
        let (event_id, _, _) = entry.persistence_parts();
        if !event_ids.insert(event_id) {
            return Err(AggregateStateInvariantError::DuplicateEventId);
        }
    }
    for (_, entry) in state.extension_invocation.persistence_entries() {
        let (event_id, _, _) = entry.persistence_parts();
        if !event_ids.insert(event_id) {
            return Err(AggregateStateInvariantError::DuplicateEventId);
        }
    }

    Ok(())
}

/// Content-free reason aggregate private state violates version 1 invariants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum AggregateStateInvariantError {
    StoreDayMismatch { family: AggregateFamily },
    EntryIdentityMismatch { family: AggregateFamily },
    DuplicateEntry { family: AggregateFamily },
    DuplicateEventId,
    InvalidBucketShape { family: AggregateFamily },
    InvalidPublicRowSpend { family: AggregateFamily },
    InvalidSessionShape { family: AggregateFamily },
    TooManySessions { family: AggregateFamily },
    DuplicateSession { family: AggregateFamily },
    InvalidSessionRelationship { family: AggregateFamily },
    InvalidSessionContributionCount { family: AggregateFamily },
}

impl fmt::Display for AggregateStateInvariantError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StoreDayMismatch { family } => {
                write!(
                    formatter,
                    "{family} aggregate store day disagrees with the latest opened day"
                )
            }
            Self::EntryIdentityMismatch { family } => {
                write!(
                    formatter,
                    "{family} aggregate entry has inconsistent identity"
                )
            }
            Self::DuplicateEntry { family } => {
                write!(
                    formatter,
                    "{family} aggregate state contains a duplicate key"
                )
            }
            Self::DuplicateEventId => {
                formatter.write_str("aggregate private state contains a duplicate event id")
            }
            Self::InvalidBucketShape { family } => {
                write!(
                    formatter,
                    "{family} aggregate bucket has inconsistent fields"
                )
            }
            Self::InvalidPublicRowSpend { family } => {
                write!(
                    formatter,
                    "{family} aggregate public-row spend is inconsistent"
                )
            }
            Self::InvalidSessionShape { family } => {
                write!(
                    formatter,
                    "{family} aggregate session fields are inconsistent"
                )
            }
            Self::TooManySessions { family } => {
                write!(
                    formatter,
                    "{family} aggregate session set exceeds its bound"
                )
            }
            Self::DuplicateSession { family } => {
                write!(
                    formatter,
                    "{family} aggregate session set contains a duplicate"
                )
            }
            Self::InvalidSessionRelationship { family } => {
                write!(
                    formatter,
                    "{family} aggregate session sets have an invalid relationship"
                )
            }
            Self::InvalidSessionContributionCount { family } => write!(
                formatter,
                "{family} aggregate session sets disagree with their contribution counts"
            ),
        }
    }
}

impl Error for AggregateStateInvariantError {}
