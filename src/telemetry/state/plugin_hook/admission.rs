//! Daily admission and lookup for plugin-hook aggregates.

use std::{collections::BTreeMap, fmt};

use super::{
    AdmittedPluginBucket, PluginHookAggregateState, PluginHookMetricsKey,
    SelectedPluginHookAggregate,
};
use crate::telemetry::{
    schema::{EventId, HookAgent, HookSurface, PluginHookAttribution, UtcDay},
    state::{
        BoundRecordingObservation, DayBeforeCurrent, StageCommit,
        open_day::OpenDayUpdate,
        public_row_budget::{DailyPublicRowBudget, PublicRowAdmission},
        staged_entries::StagedEntries,
    },
    storage::metrics::AggregateRecoveryIndex,
};

/// Daily private state for plugin-hook aggregate admission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::telemetry) struct PluginHookAggregateStore {
    public_rows: DailyPublicRowBudget,
    entries: BTreeMap<PluginHookMetricsKey, PluginHookAggregateState>,
}

impl PluginHookAggregateStore {
    #[must_use]
    pub(in crate::telemetry) const fn new(day: UtcDay) -> Self {
        Self {
            public_rows: DailyPublicRowBudget::new(day),
            entries: BTreeMap::new(),
        }
    }

    /// Return the UTC day owned by this private aggregate store.
    #[must_use]
    pub(in crate::telemetry::state) const fn day(&self) -> UtcDay {
        self.public_rows.persistence_parts().0
    }

    /// Return how many public-row slots this store has spent today.
    #[must_use]
    pub(in crate::telemetry::state) const fn public_rows_spent(&self) -> u64 {
        self.public_rows.persistence_parts().1
    }

    /// Borrow entries in their deterministic persistence order.
    pub(in crate::telemetry::state) fn persistence_entries(
        &self,
    ) -> impl ExactSizeIterator<Item = (&PluginHookMetricsKey, &PluginHookAggregateState)> {
        self.entries.iter()
    }

    /// Rebuild a store from decoded private-state fields.
    ///
    /// The persistence caller must run its store-level validator before
    /// returning this value; construction alone does not establish allowance
    /// or entry-identity consistency.
    #[must_use]
    pub(in crate::telemetry::state) fn from_persisted(
        day: UtcDay,
        public_rows_spent: u64,
        entries: BTreeMap<PluginHookMetricsKey, PluginHookAggregateState>,
    ) -> Self {
        Self {
            public_rows: DailyPublicRowBudget::from_persisted(day, public_rows_spent),
            entries,
        }
    }

    /// Stage private-state edits for one hook recording operation.
    ///
    /// The required recovery index proves that the day's snapshot loaded
    /// successfully. Every surviving public row contributes to the reconciled
    /// allowance. Day selection and allowance reconciliation happen on a copy,
    /// so dropping the returned stage leaves this store unchanged. Access is
    /// restricted to private state; row-level tests use the narrower test shim.
    ///
    /// # Errors
    ///
    /// Returns an admission error if snapshot and observation days disagree
    /// or the observation day moves backward.
    pub(in crate::telemetry::state) fn stage<'store, 'context, 'identity>(
        &'store mut self,
        recovery: &'context AggregateRecoveryIndex,
        recording: &'context BoundRecordingObservation<'identity>,
    ) -> Result<PluginHookAggregateStage<'store, 'context, 'identity>, PluginHookAdmissionError>
    {
        if recovery.day() != recording.day() {
            return Err(PluginHookAdmissionError::SnapshotDayMismatch {
                snapshot_day: recovery.day(),
                observation_day: recording.day(),
            });
        }

        let mut staged_budget = self.public_rows;
        let day_update =
            staged_budget.reconcile(recording.day(), recovery.plugin_hook_public_rows())?;

        Ok(PluginHookAggregateStage {
            destination_budget: &mut self.public_rows,
            staged_budget,
            entries: StagedEntries::new(&mut self.entries, day_update == OpenDayUpdate::Advanced),
            recovery,
            recording,
            poisoned: false,
        })
    }

    /// Widen staged access only for row-level tests outside private state.
    #[cfg(test)]
    pub(in crate::telemetry) fn stage_for_test<'store, 'context, 'identity>(
        &'store mut self,
        recovery: &'context AggregateRecoveryIndex,
        recording: &'context BoundRecordingObservation<'identity>,
    ) -> Result<PluginHookAggregateStage<'store, 'context, 'identity>, PluginHookAdmissionError>
    {
        self.stage(recovery, recording)
    }

    /// Remove entries from the previous identifier epoch without restoring
    /// the current day's public-row allowance.
    pub(in crate::telemetry) fn reset_identifier_epoch(&mut self) {
        self.entries.clear();
    }

    /// Remove current rows and restore the current day's public-row allowance.
    pub(in crate::telemetry) fn clear(&mut self) {
        self.entries.clear();
        self.public_rows.clear();
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Copy-on-write plugin-hook admission for one recording operation.
///
/// Each selection's borrow ends before the next selection begins. The caller
/// must derive invocation-wide facts from its observations rather than from
/// several simultaneously borrowed private entries.
#[must_use = "dropping a plugin-hook aggregate stage rolls back its private-state edits"]
pub(in crate::telemetry) struct PluginHookAggregateStage<'store, 'context, 'identity> {
    destination_budget: &'store mut DailyPublicRowBudget,
    staged_budget: DailyPublicRowBudget,
    entries: StagedEntries<'store, PluginHookMetricsKey, PluginHookAggregateState>,
    recovery: &'context AggregateRecoveryIndex,
    recording: &'context BoundRecordingObservation<'identity>,
    poisoned: bool,
}

impl PluginHookAggregateStage<'_, '_, '_> {
    /// Return whether a failed selection made this stage unsafe to commit.
    #[must_use]
    pub(in crate::telemetry) const fn is_poisoned(&self) -> bool {
        self.poisoned
    }

    /// Select or admit the aggregate for one plugin-hook terminal result.
    ///
    /// Adoption happens before overflow and does not spend another public-row
    /// slot. Repeated selections reuse the same staged entry. A selection
    /// error poisons the stage so commit cannot apply partial admission state.
    pub(in crate::telemetry) fn select(
        &mut self,
        agent: HookAgent,
        surface: HookSurface,
        attribution: PluginHookAttribution,
    ) -> Result<SelectedPluginHookAggregate<'_>, PluginHookAdmissionError> {
        let Self {
            staged_budget,
            entries,
            recovery,
            recording,
            poisoned,
            ..
        } = self;
        let result = Self::select_inner(
            staged_budget,
            entries,
            recovery,
            recording,
            agent,
            surface,
            attribution,
        );
        if result.is_err() {
            *poisoned = true;
        }
        result
    }

    fn select_inner<'a>(
        staged_budget: &mut DailyPublicRowBudget,
        entries: &'a mut StagedEntries<'_, PluginHookMetricsKey, PluginHookAggregateState>,
        recovery: &AggregateRecoveryIndex,
        recording: &BoundRecordingObservation<'_>,
        agent: HookAgent,
        surface: HookSurface,
        attribution: PluginHookAttribution,
    ) -> Result<SelectedPluginHookAggregate<'a>, PluginHookAdmissionError> {
        let bucket = AdmittedPluginBucket::from_attribution(recording, attribution);
        let key = PluginHookMetricsKey::new(recording, agent, surface, &bucket);

        // Returning a selection borrows the overlay. On stable Rust a single
        // mutable lookup would keep that borrow alive across the vacant path,
        // so establish occupancy with a read before borrowing for the return.
        if entries.contains_key(&key) {
            return Self::select_existing(entries, &key);
        }

        if let Some((plugin, subject)) = bucket.public_identity() {
            if let Some(event_id) = recovery.plugin_hook_event_id(agent, surface, plugin, subject) {
                return Self::insert_recovered(entries, &key, bucket, event_id);
            }

            if staged_budget.is_exhausted() {
                return Self::select_public_at_capacity(entries, key);
            }

            let PublicRowAdmission::Public = staged_budget.admit_new() else {
                unreachable!("BUG: a non-exhausted public-row budget must admit a row");
            };
        }

        Self::select_or_insert(entries, &key, bucket)
    }

    fn select_public_at_capacity<'a>(
        entries: &'a mut StagedEntries<'_, PluginHookMetricsKey, PluginHookAggregateState>,
        mut key: PluginHookMetricsKey,
    ) -> Result<SelectedPluginHookAggregate<'a>, PluginHookAdmissionError> {
        let bucket = AdmittedPluginBucket::overflow();
        key.bucket = bucket.key();
        Self::select_or_insert(entries, &key, bucket)
    }

    fn select_or_insert<'a>(
        entries: &'a mut StagedEntries<'_, PluginHookMetricsKey, PluginHookAggregateState>,
        key: &PluginHookMetricsKey,
        bucket: AdmittedPluginBucket,
    ) -> Result<SelectedPluginHookAggregate<'a>, PluginHookAdmissionError> {
        let state = entries.get_or_insert_with(key.clone(), |key| {
            PluginHookAggregateState::new(key, bucket)
        });
        state
            .select(key)
            .map_err(PluginHookAdmissionError::PrivateState)
    }

    fn select_existing<'a>(
        entries: &'a mut StagedEntries<'_, PluginHookMetricsKey, PluginHookAggregateState>,
        key: &PluginHookMetricsKey,
    ) -> Result<SelectedPluginHookAggregate<'a>, PluginHookAdmissionError> {
        let Some(state) = entries.get_mut(key) else {
            return Err(PluginHookAdmissionError::PrivateState(
                super::PluginHookAggregateSelectionError,
            ));
        };
        state
            .select(key)
            .map_err(PluginHookAdmissionError::PrivateState)
    }

    fn insert_recovered<'a>(
        entries: &'a mut StagedEntries<'_, PluginHookMetricsKey, PluginHookAggregateState>,
        key: &PluginHookMetricsKey,
        bucket: AdmittedPluginBucket,
        event_id: EventId,
    ) -> Result<SelectedPluginHookAggregate<'a>, PluginHookAdmissionError> {
        let state = entries.get_or_insert_with(key.clone(), |key| {
            PluginHookAggregateState::recovered(key, bucket, event_id)
        });
        state
            .select(key)
            .map_err(PluginHookAdmissionError::PrivateState)
    }

    /// Apply the staged allowance and entries without a recoverable failure.
    ///
    /// Hook-invocation recording commits this alongside the hook and
    /// extension-invocation stages. This method must remain infallible so that
    /// sequence cannot stop after committing only part of the private state.
    /// A poisoned stage reports its discard without changing the destination.
    pub(in crate::telemetry) fn commit(self) -> StageCommit {
        let Self {
            destination_budget,
            staged_budget,
            entries,
            recovery: _,
            recording: _,
            poisoned,
        } = self;
        if poisoned {
            return StageCommit::DiscardedPoisoned;
        }
        *destination_budget = staged_budget;
        entries.commit();
        StageCommit::Applied
    }
}

/// Failure while selecting private plugin-hook aggregate state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum PluginHookAdmissionError {
    SnapshotDayMismatch {
        snapshot_day: UtcDay,
        observation_day: UtcDay,
    },
    DayBeforeCurrent(DayBeforeCurrent),
    PrivateState(super::PluginHookAggregateSelectionError),
}

impl From<DayBeforeCurrent> for PluginHookAdmissionError {
    fn from(error: DayBeforeCurrent) -> Self {
        Self::DayBeforeCurrent(error)
    }
}

impl fmt::Display for PluginHookAdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SnapshotDayMismatch {
                snapshot_day,
                observation_day,
            } => write!(
                formatter,
                concat!(
                    "plugin-hook snapshot belongs to {}, ",
                    "not observation day {}"
                ),
                snapshot_day, observation_day
            ),
            Self::DayBeforeCurrent(error) => write!(formatter, "plugin-hook admission {error}"),
            Self::PrivateState(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for PluginHookAdmissionError {}

#[cfg(test)]
mod tests;
