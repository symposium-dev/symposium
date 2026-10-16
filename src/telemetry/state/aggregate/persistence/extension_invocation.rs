//! Private wire shape for extension-invocation aggregates.

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::{AggregateStateInvariantError, sessions};
use crate::telemetry::{
    identity::{IdentifierWindowScope, SessionId},
    schema::{
        EventId, ExtensionInvocationAgent, ExtensionTargetScope, SafeSkillAttribution,
        UnnamedExtensionReason, UtcDay,
    },
    state::{
        aggregate::AggregateFamily,
        extension_invocation::{
            AdmittedExtensionBucket, ExtensionInvocationAggregateKey,
            ExtensionInvocationAggregateState, ExtensionInvocationAggregateStore,
        },
        public_row_budget::MAX_PUBLIC_ROWS_PER_DAY,
    },
};

/// Borrowed extension-invocation store in canonical private-state order.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the root state codec consumes this in the next stack branch"
    )
)]
#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) struct ExtensionInvocationStoreRef<'a> {
    // TOML requires scalar store metadata before the entry tables.
    day: UtcDay,
    public_rows_spent: u64,
    entries: Vec<ExtensionInvocationEntryRef<'a>>,
}

impl<'store, 'scope, 'identity>
    TryFrom<(
        &'store ExtensionInvocationAggregateStore,
        &'scope IdentifierWindowScope<'identity>,
    )> for ExtensionInvocationStoreRef<'store>
{
    type Error = AggregateStateInvariantError;

    fn try_from(
        (store, scope): (
            &'store ExtensionInvocationAggregateStore,
            &'scope IdentifierWindowScope<'identity>,
        ),
    ) -> Result<Self, Self::Error> {
        validate(store, scope)?;
        let entries = store
            .persistence_entries()
            .map(|(key, entry)| ExtensionInvocationEntryRef::new(key, entry))
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
struct ExtensionInvocationEntryRef<'a> {
    agent: ExtensionInvocationAgent,
    scope: ExtensionTargetScope,
    #[serde(skip_serializing_if = "Option::is_none")]
    unnamed_reason: Option<UnnamedExtensionReason>,
    event_id: EventId,
    attempted_contribution_count: u64,
    completed_contribution_count: u64,
    session_counts_complete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    identified_sessions: Option<&'a BTreeSet<SessionId>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    identified_sessions_completed: Option<&'a BTreeSet<SessionId>>,
    // A nested TOML table must follow every scalar and scalar-array field.
    #[serde(skip_serializing_if = "Option::is_none")]
    attribution: Option<&'a SafeSkillAttribution>,
}

impl<'a> ExtensionInvocationEntryRef<'a> {
    fn new(
        key: &ExtensionInvocationAggregateKey,
        entry: &'a ExtensionInvocationAggregateState,
    ) -> Self {
        let (event_id, bucket, tracker) = entry.persistence_parts();
        let (attempted_contributions, completed_contributions, pair) = tracker.persistence_parts();
        let sessions = sessions::SessionPairRef::from(pair);

        Self {
            agent: key.agent,
            scope: bucket.scope(),
            unnamed_reason: bucket.unnamed_reason(),
            event_id,
            attempted_contribution_count: attempted_contributions,
            completed_contribution_count: completed_contributions,
            session_counts_complete: sessions.complete,
            identified_sessions: sessions.first,
            identified_sessions_completed: sessions.second,
            attribution: bucket.safe_attribution(),
        }
    }
}

/// Extension-invocation store decoded before runtime keys are reconstructed.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the root state codec consumes this in the next stack branch"
    )
)]
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(super) struct RawExtensionInvocationStore {
    day: UtcDay,
    public_rows_spent: u64,
    entries: Vec<RawExtensionInvocationEntry>,
}

impl RawExtensionInvocationStore {
    /// Rebuild extension keys and validate the resulting runtime store.
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
    ) -> Result<ExtensionInvocationAggregateStore, AggregateStateInvariantError> {
        let mut entries = BTreeMap::new();
        for raw in self.entries {
            let RawExtensionInvocationEntry {
                agent,
                scope: bucket_scope,
                unnamed_reason,
                attribution,
                event_id,
                attempted_contribution_count,
                completed_contribution_count,
                session_counts_complete,
                identified_sessions,
                identified_sessions_completed,
            } = raw;
            let bucket = decode_bucket(scope, bucket_scope, attribution, unnamed_reason)?;
            let key = ExtensionInvocationAggregateKey::from_scope(self.day, scope, agent, &bucket);
            let sessions = sessions::decode_extension_invocation(
                session_counts_complete,
                identified_sessions,
                identified_sessions_completed,
                attempted_contribution_count,
                completed_contribution_count,
            )?;
            let entry = ExtensionInvocationAggregateState::from_persisted(
                &key,
                event_id,
                bucket,
                attempted_contribution_count,
                completed_contribution_count,
                sessions,
            );
            if entries.insert(key, entry).is_some() {
                return Err(AggregateStateInvariantError::DuplicateEntry {
                    family: AggregateFamily::ExtensionInvocation,
                });
            }
        }

        let store = ExtensionInvocationAggregateStore::from_persisted(
            self.day,
            self.public_rows_spent,
            entries,
        );
        validate(&store, scope)?;
        Ok(store)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct RawExtensionInvocationEntry {
    agent: ExtensionInvocationAgent,
    scope: ExtensionTargetScope,
    unnamed_reason: Option<UnnamedExtensionReason>,
    attribution: Option<SafeSkillAttribution>,
    event_id: EventId,
    attempted_contribution_count: u64,
    completed_contribution_count: u64,
    session_counts_complete: bool,
    identified_sessions: Option<Vec<SessionId>>,
    identified_sessions_completed: Option<Vec<SessionId>>,
}

fn decode_bucket(
    identity: &IdentifierWindowScope<'_>,
    scope: ExtensionTargetScope,
    attribution: Option<SafeSkillAttribution>,
    unnamed_reason: Option<UnnamedExtensionReason>,
) -> Result<AdmittedExtensionBucket, AggregateStateInvariantError> {
    match (scope, attribution, unnamed_reason) {
        (ExtensionTargetScope::Public, Some(attribution), None) => {
            Ok(AdmittedExtensionBucket::public(identity, attribution))
        }
        (ExtensionTargetScope::Unnamed, None, Some(reason)) => {
            Ok(AdmittedExtensionBucket::unnamed(reason))
        }
        (ExtensionTargetScope::Overflow, None, None) => Ok(AdmittedExtensionBucket::overflow()),
        _ => Err(AggregateStateInvariantError::InvalidBucketShape {
            family: AggregateFamily::ExtensionInvocation,
        }),
    }
}

/// Validate every extension store invariant shared by encoding and decoding.
pub(super) fn validate(
    store: &ExtensionInvocationAggregateStore,
    scope: &IdentifierWindowScope<'_>,
) -> Result<(), AggregateStateInvariantError> {
    let public_entries = store
        .persistence_entries()
        .filter(|(_, entry)| {
            let (_, bucket, _) = entry.persistence_parts();
            bucket.scope() == ExtensionTargetScope::Public
        })
        .count();
    validate_spend(public_entries, store.public_rows_spent())?;

    let mut event_ids = BTreeSet::new();
    for (key, entry) in store.persistence_entries() {
        let (event_id, bucket, tracker) = entry.persistence_parts();
        let expected =
            ExtensionInvocationAggregateKey::from_scope(store.day(), scope, key.agent, bucket);
        if tracker.key() != key || expected != *key {
            return Err(AggregateStateInvariantError::EntryIdentityMismatch {
                family: AggregateFamily::ExtensionInvocation,
            });
        }
        if !event_ids.insert(event_id) {
            return Err(AggregateStateInvariantError::DuplicateEventId);
        }

        let (attempted_contributions, completed_contributions, sessions) =
            tracker.persistence_parts();
        sessions::validate_extension_invocation(
            sessions,
            attempted_contributions,
            completed_contributions,
        )?;
    }

    Ok(())
}

fn validate_spend(public_entries: usize, spent: u64) -> Result<(), AggregateStateInvariantError> {
    let public_entries = u64::try_from(public_entries).unwrap_or(u64::MAX);
    if spent > MAX_PUBLIC_ROWS_PER_DAY || public_entries > spent {
        return Err(AggregateStateInvariantError::InvalidPublicRowSpend {
            family: AggregateFamily::ExtensionInvocation,
        });
    }

    Ok(())
}
