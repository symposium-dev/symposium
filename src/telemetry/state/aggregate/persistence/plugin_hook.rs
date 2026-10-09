//! Private wire shape for plugin-hook aggregates.

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::{AggregateStateInvariantError, sessions};
use crate::telemetry::{
    identity::{IdentifierWindowScope, SessionId},
    schema::{EventId, HookAgent, HookSurface, PluginScope, PublicPluginCoordinate, UtcDay},
    state::{
        aggregate::AggregateFamily,
        plugin_hook::{
            AdmittedPluginBucket, PluginHookAggregateState, PluginHookAggregateStore,
            PluginHookMetricsKey,
        },
        public_row_budget::MAX_PUBLIC_ROWS_PER_DAY,
    },
};

/// Borrowed plugin-hook store in canonical private-state order.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the root state codec consumes this in the next stack branch"
    )
)]
#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) struct PluginHookStoreRef<'a> {
    // TOML requires scalar store metadata before the entry tables.
    day: UtcDay,
    public_rows_spent: u64,
    entries: Vec<PluginHookEntryRef<'a>>,
}

impl<'store, 'scope, 'identity>
    TryFrom<(
        &'store PluginHookAggregateStore,
        &'scope IdentifierWindowScope<'identity>,
    )> for PluginHookStoreRef<'store>
{
    type Error = AggregateStateInvariantError;

    fn try_from(
        (store, scope): (
            &'store PluginHookAggregateStore,
            &'scope IdentifierWindowScope<'identity>,
        ),
    ) -> Result<Self, Self::Error> {
        validate(store, scope)?;
        let entries = store
            .persistence_entries()
            .map(|(key, entry)| PluginHookEntryRef::new(key, entry))
            .collect();

        Ok(Self {
            day: store.day(),
            public_rows_spent: store.public_rows_spent(),
            entries,
        })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct PluginHookEntryRef<'a> {
    agent: HookAgent,
    hook: HookSurface,
    scope: PluginScope,
    event_id: EventId,
    contribution_count: u64,
    session_counts_complete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    identified_sessions: Option<&'a BTreeSet<SessionId>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    identified_sessions_non_ok: Option<&'a BTreeSet<SessionId>>,
    // A nested TOML table must follow every scalar and scalar-array field.
    #[serde(skip_serializing_if = "Option::is_none")]
    plugin: Option<&'a PublicPluginCoordinate>,
}

impl<'a> PluginHookEntryRef<'a> {
    fn new(key: &PluginHookMetricsKey, entry: &'a PluginHookAggregateState) -> Self {
        let (event_id, bucket, tracker) = entry.persistence_parts();
        let (contribution_count, pair) = tracker.persistence_parts();
        let sessions = sessions::SessionPairRef::from(pair);

        Self {
            agent: key.agent,
            hook: key.surface,
            scope: bucket.scope(),
            event_id,
            contribution_count,
            session_counts_complete: sessions.complete,
            identified_sessions: sessions.first,
            identified_sessions_non_ok: sessions.second,
            plugin: bucket.plugin(),
        }
    }
}

/// Plugin-hook store decoded before its runtime keys are reconstructed.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the root state codec consumes this in the next stack branch"
    )
)]
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(super) struct RawPluginHookStore {
    day: UtcDay,
    public_rows_spent: u64,
    entries: Vec<RawPluginHookEntry>,
}

impl RawPluginHookStore {
    /// Rebuild plugin-hook keys and validate the resulting runtime store.
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
    ) -> Result<PluginHookAggregateStore, AggregateStateInvariantError> {
        let mut entries = BTreeMap::new();
        for raw in self.entries {
            let RawPluginHookEntry {
                agent,
                hook,
                scope: bucket_scope,
                plugin,
                event_id,
                contribution_count,
                session_counts_complete,
                identified_sessions,
                identified_sessions_non_ok,
            } = raw;
            let bucket = decode_bucket(scope, bucket_scope, plugin)?;
            let key = PluginHookMetricsKey::from_scope(self.day, scope, agent, hook, &bucket);
            let sessions = sessions::decode_plugin_hook(
                session_counts_complete,
                identified_sessions,
                identified_sessions_non_ok,
                contribution_count,
            )?;
            let entry = PluginHookAggregateState::from_persisted(
                &key,
                event_id,
                bucket,
                contribution_count,
                sessions,
            );
            if entries.insert(key, entry).is_some() {
                return Err(AggregateStateInvariantError::DuplicateEntry {
                    family: AggregateFamily::PluginHook,
                });
            }
        }

        let store =
            PluginHookAggregateStore::from_persisted(self.day, self.public_rows_spent, entries);
        validate(&store, scope)?;
        Ok(store)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct RawPluginHookEntry {
    agent: HookAgent,
    hook: HookSurface,
    scope: PluginScope,
    plugin: Option<PublicPluginCoordinate>,
    event_id: EventId,
    contribution_count: u64,
    session_counts_complete: bool,
    identified_sessions: Option<Vec<SessionId>>,
    identified_sessions_non_ok: Option<Vec<SessionId>>,
}

fn decode_bucket(
    identity: &IdentifierWindowScope<'_>,
    scope: PluginScope,
    plugin: Option<PublicPluginCoordinate>,
) -> Result<AdmittedPluginBucket, AggregateStateInvariantError> {
    match (scope, plugin) {
        (PluginScope::Public, Some(plugin)) => Ok(AdmittedPluginBucket::public(identity, plugin)),
        (PluginScope::Unnamed, None) => Ok(AdmittedPluginBucket::unnamed()),
        (PluginScope::Overflow, None) => Ok(AdmittedPluginBucket::overflow()),
        _ => Err(AggregateStateInvariantError::InvalidBucketShape {
            family: AggregateFamily::PluginHook,
        }),
    }
}

/// Validate every plugin-hook store invariant shared by encoding and decoding.
pub(super) fn validate(
    store: &PluginHookAggregateStore,
    scope: &IdentifierWindowScope<'_>,
) -> Result<(), AggregateStateInvariantError> {
    let public_entries = store
        .persistence_entries()
        .filter(|(_, entry)| {
            let (_, bucket, _) = entry.persistence_parts();
            bucket.scope() == PluginScope::Public
        })
        .count();
    validate_spend(public_entries, store.public_rows_spent())?;

    let mut event_ids = BTreeSet::new();
    for (key, entry) in store.persistence_entries() {
        let (event_id, bucket, tracker) = entry.persistence_parts();
        let expected =
            PluginHookMetricsKey::from_scope(store.day(), scope, key.agent, key.surface, bucket);
        if tracker.key() != key || expected != *key {
            return Err(AggregateStateInvariantError::EntryIdentityMismatch {
                family: AggregateFamily::PluginHook,
            });
        }
        if !event_ids.insert(event_id) {
            return Err(AggregateStateInvariantError::DuplicateEventId);
        }

        let (contribution_count, sessions) = tracker.persistence_parts();
        sessions::validate_plugin_hook(sessions, contribution_count)?;
    }

    Ok(())
}

fn validate_spend(public_entries: usize, spent: u64) -> Result<(), AggregateStateInvariantError> {
    let public_entries = u64::try_from(public_entries).unwrap_or(u64::MAX);
    if spent > MAX_PUBLIC_ROWS_PER_DAY || public_entries > spent {
        return Err(AggregateStateInvariantError::InvalidPublicRowSpend {
            family: AggregateFamily::PluginHook,
        });
    }

    Ok(())
}
