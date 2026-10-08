//! Snapshot facts used to recover private aggregate admission state.
//!
//! Public-row counts and adoptable rows are deliberately separate. Every
//! surviving public row consumes the daily allowance, including rows from old
//! identifier epochs. Adoption additionally requires the observation's
//! derived subject to match the stored subject.

use std::collections::BTreeMap;

use crate::telemetry::{
    identity::{ExtensionSubject, PluginSubject},
    schema::{
        AggregateRow, EventId, ExtensionInvocationAgent, HookAgent, HookMetricsKey, HookSurface,
        PublicPluginCoordinate, PublicSkillCoordinate, UtcDay, VersionedRow,
    },
};

use super::MetricSnapshot;

/// Read-only recovery facts built from one validated metric snapshot.
///
/// The type has no public constructor: a caller can obtain it only from a
/// successfully loaded or deliberately empty [`MetricSnapshot`]. Requiring it
/// in admission APIs makes aggregate selection unavailable after a failed
/// snapshot load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::telemetry) struct AggregateRecoveryIndex {
    day: UtcDay,
    hook_rows: BTreeMap<HookMetricsKey, EventId>,
    plugin_hook_public_rows: u64,
    extension_invocation_public_rows: u64,
    plugin_hook_rows: BTreeMap<PluginHookRecoveryKey, EventId>,
    extension_invocation_rows: BTreeMap<ExtensionInvocationRecoveryKey, EventId>,
}

impl AggregateRecoveryIndex {
    pub(super) fn from_snapshot(snapshot: &MetricSnapshot) -> Self {
        let mut index = Self {
            day: snapshot.day,
            hook_rows: BTreeMap::new(),
            plugin_hook_public_rows: 0,
            extension_invocation_public_rows: 0,
            plugin_hook_rows: BTreeMap::new(),
            extension_invocation_rows: BTreeMap::new(),
        };

        for stored in &snapshot.rows {
            match &stored.row {
                AggregateRow::Hook(row) => {
                    retain_lowest_event_id(&mut index.hook_rows, row.key(), row.event_id());
                }
                AggregateRow::PluginHook(row) => {
                    let Some((agent, hook, plugin, subject)) = row.public_recovery_identity()
                    else {
                        continue;
                    };
                    index.plugin_hook_public_rows = index.plugin_hook_public_rows.saturating_add(1);
                    retain_lowest_event_id(
                        &mut index.plugin_hook_rows,
                        PluginHookRecoveryKey::new(agent, hook, plugin, subject),
                        row.event_id(),
                    );
                }
                AggregateRow::ExtensionInvocation(row) => {
                    let Some((agent, target, subject)) = row.public_recovery_identity() else {
                        continue;
                    };
                    index.extension_invocation_public_rows =
                        index.extension_invocation_public_rows.saturating_add(1);
                    retain_lowest_event_id(
                        &mut index.extension_invocation_rows,
                        ExtensionInvocationRecoveryKey::new(agent, target, subject),
                        row.event_id(),
                    );
                }
            }
        }

        index
    }

    /// Return the UTC day owned by the indexed snapshot.
    #[must_use]
    pub(in crate::telemetry) const fn day(&self) -> UtcDay {
        self.day
    }

    /// Find the deterministic surviving row for one hook aggregate key.
    #[must_use]
    pub(in crate::telemetry) fn hook_event_id(&self, key: HookMetricsKey) -> Option<EventId> {
        self.hook_rows.get(&key).copied()
    }

    /// Count surviving public plugin-hook rows, including old epochs.
    #[must_use]
    pub(in crate::telemetry) const fn plugin_hook_public_rows(&self) -> u64 {
        self.plugin_hook_public_rows
    }

    /// Count surviving public extension-invocation rows, including old epochs.
    #[must_use]
    pub(in crate::telemetry) const fn extension_invocation_public_rows(&self) -> u64 {
        self.extension_invocation_public_rows
    }

    /// Find the deterministic surviving row for one public plugin identity.
    #[must_use]
    pub(in crate::telemetry) fn plugin_hook_event_id(
        &self,
        agent: HookAgent,
        hook: HookSurface,
        plugin: &PublicPluginCoordinate,
        subject: PluginSubject,
    ) -> Option<EventId> {
        self.plugin_hook_rows
            .get(&PluginHookRecoveryKey::new(agent, hook, plugin, subject))
            .copied()
    }

    /// Find the deterministic surviving row for one public skill identity.
    #[must_use]
    pub(in crate::telemetry) fn extension_invocation_event_id(
        &self,
        agent: ExtensionInvocationAgent,
        target: &PublicSkillCoordinate,
        subject: ExtensionSubject,
    ) -> Option<EventId> {
        self.extension_invocation_rows
            .get(&ExtensionInvocationRecoveryKey::new(agent, target, subject))
            .copied()
    }
}

fn retain_lowest_event_id<K: Ord>(rows: &mut BTreeMap<K, EventId>, key: K, event_id: EventId) {
    rows.entry(key)
        .and_modify(|stored| *stored = (*stored).min(event_id))
        .or_insert(event_id);
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PluginHookRecoveryKey {
    agent: HookAgent,
    hook: HookSurface,
    source: &'static str,
    name: Box<str>,
    subject: PluginSubject,
}

impl PluginHookRecoveryKey {
    fn new(
        agent: HookAgent,
        hook: HookSurface,
        plugin: &PublicPluginCoordinate,
        subject: PluginSubject,
    ) -> Self {
        Self {
            agent,
            hook,
            source: plugin.source().as_str(),
            name: plugin.name().as_str().into(),
            subject,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ExtensionInvocationRecoveryKey {
    agent: ExtensionInvocationAgent,
    source: &'static str,
    name: Box<str>,
    subject: ExtensionSubject,
}

impl ExtensionInvocationRecoveryKey {
    fn new(
        agent: ExtensionInvocationAgent,
        target: &PublicSkillCoordinate,
        subject: ExtensionSubject,
    ) -> Self {
        Self {
            agent,
            source: target.source().as_str(),
            name: target.name().as_str().into(),
            subject,
        }
    }
}

#[cfg(test)]
mod tests;
