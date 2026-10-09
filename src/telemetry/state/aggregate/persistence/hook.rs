//! Private wire shape for top-level hook aggregates.

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::{AggregateStateInvariantError, sessions};
use crate::telemetry::{
    identity::{IdentifierWindowScope, SessionId},
    schema::{HookAgent, HookMetricsKey, HookSurface, UtcDay},
    state::{HookSessionCountTracker, aggregate::AggregateFamily, hook::HookAggregateStore},
};

/// Borrowed hook store in canonical private-state order.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the root state codec consumes this in the next stack branch"
    )
)]
#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) struct HookStoreRef<'a> {
    day: UtcDay,
    entries: Vec<HookEntryRef<'a>>,
}

impl<'store, 'scope, 'identity>
    TryFrom<(
        &'store HookAggregateStore,
        &'scope IdentifierWindowScope<'identity>,
    )> for HookStoreRef<'store>
{
    type Error = AggregateStateInvariantError;

    fn try_from(
        (store, scope): (
            &'store HookAggregateStore,
            &'scope IdentifierWindowScope<'identity>,
        ),
    ) -> Result<Self, Self::Error> {
        validate(store, scope)?;
        let entries = store
            .persistence_entries()
            .map(|(key, tracker)| HookEntryRef::new(*key, tracker))
            .collect();

        Ok(Self {
            day: store.day(),
            entries,
        })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct HookEntryRef<'a> {
    agent: HookAgent,
    hook: HookSurface,
    contribution_count: u64,
    session_counts_complete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    identified_sessions: Option<&'a BTreeSet<SessionId>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    identified_sessions_non_ok: Option<&'a BTreeSet<SessionId>>,
}

impl<'a> HookEntryRef<'a> {
    fn new(key: HookMetricsKey, tracker: &'a HookSessionCountTracker<HookMetricsKey>) -> Self {
        let (contribution_count, pair) = tracker.persistence_parts();
        let sessions = sessions::SessionPairRef::from(pair);

        Self {
            agent: key.agent(),
            hook: key.hook(),
            contribution_count,
            session_counts_complete: sessions.complete,
            identified_sessions: sessions.first,
            identified_sessions_non_ok: sessions.second,
        }
    }
}

/// Hook store decoded before its runtime keys are reconstructed.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the root state codec consumes this in the next stack branch"
    )
)]
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(super) struct RawHookStore {
    day: UtcDay,
    entries: Vec<RawHookEntry>,
}

impl RawHookStore {
    /// Rebuild hook keys and validate the resulting runtime store.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the root state codec consumes this in the next stack branch"
        )
    )]
    pub(super) fn into_runtime(
        self,
        scope: &IdentifierWindowScope<'_>,
    ) -> Result<HookAggregateStore, AggregateStateInvariantError> {
        let mut entries = BTreeMap::new();
        for raw in self.entries {
            let key = HookMetricsKey::from_scope(self.day, scope, raw.agent, raw.hook);
            let sessions = sessions::decode_hook(
                raw.session_counts_complete,
                raw.identified_sessions,
                raw.identified_sessions_non_ok,
                raw.contribution_count,
            )?;
            let tracker =
                HookSessionCountTracker::from_persisted(key, raw.contribution_count, sessions);
            if entries.insert(key, tracker).is_some() {
                return Err(AggregateStateInvariantError::DuplicateEntry {
                    family: AggregateFamily::Hook,
                });
            }
        }

        let store = HookAggregateStore::from_persisted(self.day, entries);
        validate(&store, scope)?;
        Ok(store)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct RawHookEntry {
    agent: HookAgent,
    hook: HookSurface,
    contribution_count: u64,
    session_counts_complete: bool,
    identified_sessions: Option<Vec<SessionId>>,
    identified_sessions_non_ok: Option<Vec<SessionId>>,
}

/// Validate every hook-store invariant shared by encoding and decoding.
pub(super) fn validate(
    store: &HookAggregateStore,
    scope: &IdentifierWindowScope<'_>,
) -> Result<(), AggregateStateInvariantError> {
    for (key, tracker) in store.persistence_entries() {
        let expected = HookMetricsKey::from_scope(store.day(), scope, key.agent(), key.hook());
        if tracker.key() != key || expected != *key {
            return Err(AggregateStateInvariantError::EntryIdentityMismatch {
                family: AggregateFamily::Hook,
            });
        }

        let (contribution_count, sessions) = tracker.persistence_parts();
        sessions::validate_hook(sessions, contribution_count)?;
    }

    Ok(())
}
