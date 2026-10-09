//! Daily private state for top-level hook aggregate rows.

use std::{collections::BTreeMap, fmt};

use super::{
    BoundRecordingObservation, DayBeforeCurrent, HookSessionCountTracker, StageCommit,
    open_day::{OpenDay, OpenDayUpdate},
    staged_entries::StagedEntries,
};
use crate::telemetry::schema::{HookAgent, HookMetricsKey, HookSurface, UtcDay};

/// Private session state for the current day's top-level hook aggregates.
///
/// The map is ordered so its eventual TOML representation is deterministic.
/// Row creation versus update remains a snapshot concern: private state may
/// survive a failed snapshot replacement, or be absent while a row survives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::telemetry) struct HookAggregateStore {
    day: OpenDay,
    entries: BTreeMap<HookMetricsKey, HookSessionCountTracker<HookMetricsKey>>,
}

impl HookAggregateStore {
    #[must_use]
    pub(in crate::telemetry) const fn new(day: UtcDay) -> Self {
        Self {
            day: OpenDay::new(day),
            entries: BTreeMap::new(),
        }
    }

    /// Return the UTC day owned by this private aggregate store.
    #[must_use]
    pub(in crate::telemetry::state) const fn day(&self) -> UtcDay {
        self.day.day()
    }

    /// Borrow entries in their deterministic persistence order.
    pub(in crate::telemetry::state) fn persistence_entries(
        &self,
    ) -> impl ExactSizeIterator<Item = (&HookMetricsKey, &HookSessionCountTracker<HookMetricsKey>)>
    {
        self.entries.iter()
    }

    /// Rebuild a store from decoded private-state fields.
    ///
    /// The persistence caller must run its store-level validator before
    /// returning this value; construction alone does not prove that every key
    /// and tracker carry consistent identity.
    #[must_use]
    pub(in crate::telemetry::state) fn from_persisted(
        day: UtcDay,
        entries: BTreeMap<HookMetricsKey, HookSessionCountTracker<HookMetricsKey>>,
    ) -> Self {
        Self {
            day: OpenDay::new(day),
            entries,
        }
    }

    /// Stage private-state edits for one hook recording operation.
    ///
    /// Day selection is applied to a copy. Dropping the returned stage leaves
    /// both the open day and every stored tracker unchanged. The invocation
    /// coordinator will become the sole caller and committer once it lands;
    /// this per-store entry point remains only for the staged rollout.
    ///
    /// # Errors
    ///
    /// Returns [`HookAggregateStoreError::DayBeforeCurrent`] when the
    /// recording belongs to a closed day, or
    /// [`HookAggregateStoreError::PrivateState`] when a stored tracker does
    /// not match its map key.
    pub(in crate::telemetry::state) fn stage<'store, 'context, 'identity>(
        &'store mut self,
        recording: &'context BoundRecordingObservation<'identity>,
    ) -> Result<HookAggregateStage<'store, 'context, 'identity>, HookAggregateStoreError> {
        let mut staged_day = self.day;
        let day_update = staged_day
            .select(recording.day())
            .map_err(HookAggregateStoreError::DayBeforeCurrent)?;

        Ok(HookAggregateStage {
            destination_day: &mut self.day,
            staged_day,
            entries: StagedEntries::new(&mut self.entries, day_update == OpenDayUpdate::Advanced),
            recording,
            poisoned: false,
        })
    }

    /// Widen staged access only for row-level tests outside private state.
    #[cfg(test)]
    pub(in crate::telemetry) fn stage_for_test<'store, 'context, 'identity>(
        &'store mut self,
        recording: &'context BoundRecordingObservation<'identity>,
    ) -> Result<HookAggregateStage<'store, 'context, 'identity>, HookAggregateStoreError> {
        self.stage(recording)
    }

    /// Remove entries derived under the previous identifier epoch.
    pub(in crate::telemetry) fn reset_identifier_epoch(&mut self) {
        self.entries.clear();
    }

    /// Remove private state paired with deleted aggregate rows.
    pub(in crate::telemetry) fn clear(&mut self) {
        self.entries.clear();
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Copy-on-write private hook state for one recording operation.
///
/// Selections borrow this stage one at a time. A coordinator must therefore
/// derive cross-plugin facts from its immutable observations before entering
/// the sequential selection loop.
#[must_use = "dropping a hook aggregate stage rolls back its private-state edits"]
pub(in crate::telemetry) struct HookAggregateStage<'store, 'context, 'identity> {
    destination_day: &'store mut OpenDay,
    staged_day: OpenDay,
    entries: StagedEntries<'store, HookMetricsKey, HookSessionCountTracker<HookMetricsKey>>,
    recording: &'context BoundRecordingObservation<'identity>,
    poisoned: bool,
}

impl HookAggregateStage<'_, '_, '_> {
    /// Return whether a failed selection made this stage unsafe to commit.
    #[must_use]
    pub(in crate::telemetry) const fn is_poisoned(&self) -> bool {
        self.poisoned
    }

    /// Select a staged tracker for one hook aggregate.
    ///
    /// Repeated selections reuse the same staged value. After day rollover,
    /// the overlay never reads from the closed day's map, even if a future key
    /// type could otherwise collide with an old entry. A selection error
    /// poisons the stage so commit cannot apply partial edits.
    pub(in crate::telemetry) fn select(
        &mut self,
        agent: HookAgent,
        hook: HookSurface,
    ) -> Result<&mut HookSessionCountTracker<HookMetricsKey>, HookAggregateStoreError> {
        let key = HookMetricsKey::new(self.recording, agent, hook);
        let Self {
            entries, poisoned, ..
        } = self;
        let session_counts =
            entries.get_or_insert_with(key, |key| HookSessionCountTracker::new(*key));
        if session_counts.key() != &key {
            *poisoned = true;
            return Err(HookAggregateStoreError::PrivateState(
                HookAggregateSelectionError,
            ));
        }

        Ok(session_counts)
    }

    /// Apply the staged day and entry edits without a recoverable failure.
    ///
    /// Hook-invocation recording commits this alongside the plugin-hook and
    /// extension-invocation stages. This method must remain infallible so that
    /// sequence cannot stop after committing only part of the private state.
    /// A poisoned stage reports its discard without changing the destination.
    pub(in crate::telemetry) fn commit(self) -> StageCommit {
        let Self {
            destination_day,
            staged_day,
            entries,
            recording: _,
            poisoned,
        } = self;
        if poisoned {
            return StageCommit::DiscardedPoisoned;
        }
        *destination_day = staged_day;
        entries.commit();
        StageCommit::Applied
    }
}

/// Failure while selecting private hook aggregate state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum HookAggregateStoreError {
    DayBeforeCurrent(DayBeforeCurrent),
    PrivateState(HookAggregateSelectionError),
}

impl fmt::Display for HookAggregateStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DayBeforeCurrent(error) => write!(formatter, "hook aggregate {error}"),
            Self::PrivateState(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for HookAggregateStoreError {}

/// A private tracker stored under another hook aggregate key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) struct HookAggregateSelectionError;

impl fmt::Display for HookAggregateSelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("hook private state belongs to another aggregate")
    }
}

impl std::error::Error for HookAggregateSelectionError {}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, TimeZone, Utc};

    use super::*;
    use crate::telemetry::{
        schema::{HookOutcome, UtcSecond},
        state::{HookSessionCountSnapshot, IDENTIFIER_WINDOW_TEST_STATE, TelemetryStateV1},
    };

    fn day(day: u32) -> UtcDay {
        UtcDay::from_date(NaiveDate::from_ymd_opt(2026, 8, day).unwrap())
    }

    fn state() -> TelemetryStateV1 {
        toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap()
    }

    fn recording_at(state: &mut TelemetryStateV1, day: u32) -> BoundRecordingObservation<'_> {
        let completed_at =
            UtcSecond::from_datetime(Utc.with_ymd_and_hms(2026, 8, day, 10, 2, 11).unwrap());
        let observation = state.observe_recording(completed_at).unwrap();

        state.bind_recording_observation(observation).unwrap()
    }

    #[test]
    fn repeated_selection_reuses_the_same_private_tracker() {
        let mut state = state();
        let mut store = HookAggregateStore::new(day(3));
        let recording = recording_at(&mut state, 3);
        let mut staged = store.stage(&recording).unwrap();
        let first_key = {
            let tracker = staged
                .select(HookAgent::Claude, HookSurface::PreToolUse)
                .unwrap();
            tracker.checked_record(0, None, HookOutcome::Ok).unwrap();
            *tracker.key()
        };

        let tracker = staged
            .select(HookAgent::Claude, HookSurface::PreToolUse)
            .unwrap();

        assert_eq!(tracker.key(), &first_key);
        assert_eq!(tracker.snapshot(), HookSessionCountSnapshot::Incomplete);
        assert_eq!(staged.commit(), StageCommit::Applied);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn another_hook_surface_gets_another_private_tracker() {
        let mut state = state();
        let mut store = HookAggregateStore::new(day(3));
        let recording = recording_at(&mut state, 3);
        let mut staged = store.stage(&recording).unwrap();
        let pre_tool_key = *staged
            .select(HookAgent::Claude, HookSurface::PreToolUse)
            .unwrap()
            .key();

        let post_tool_key = *staged
            .select(HookAgent::Claude, HookSurface::PostToolUse)
            .unwrap()
            .key();

        assert_ne!(post_tool_key, pre_tool_key);
        assert_eq!(staged.commit(), StageCommit::Applied);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn later_day_stages_a_fresh_tracker_and_drops_closed_day_only_on_commit() {
        let mut state = state();
        let mut store = HookAggregateStore::new(day(3));
        {
            let recording = recording_at(&mut state, 3);
            let mut staged = store.stage(&recording).unwrap();
            let tracker = staged
                .select(HookAgent::Claude, HookSurface::PreToolUse)
                .unwrap();
            tracker.checked_record(0, None, HookOutcome::Ok).unwrap();
            assert_eq!(staged.commit(), StageCommit::Applied);
        }
        let later = recording_at(&mut state, 4);
        let before = store.clone();

        {
            let mut staged = store.stage(&later).unwrap();
            let tracker = staged
                .select(HookAgent::Claude, HookSurface::PreToolUse)
                .unwrap();

            assert_eq!(
                tracker.snapshot(),
                HookSessionCountSnapshot::Complete {
                    identified_sessions: 0,
                    identified_sessions_non_ok: 0,
                }
            );
        }
        assert_eq!(store, before);

        let mut staged = store.stage(&later).unwrap();
        staged
            .select(HookAgent::Claude, HookSurface::PreToolUse)
            .unwrap();
        assert_eq!(staged.commit(), StageCommit::Applied);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn committing_an_unused_current_day_stage_is_a_noop() {
        let mut state = state();
        let mut store = HookAggregateStore::new(day(3));
        let recording = recording_at(&mut state, 3);
        let before = store.clone();

        let staged = store.stage(&recording).unwrap();

        assert_eq!(staged.commit(), StageCommit::Applied);
        assert_eq!(store, before);
    }

    #[test]
    fn earlier_day_is_rejected_without_changing_the_store() {
        let mut state = state();
        let mut store = HookAggregateStore::new(day(4));
        let recording = recording_at(&mut state, 3);
        let before = store.clone();

        let result = store.stage(&recording);

        let Err(error) = result else {
            panic!("accepted a recording from a closed day");
        };
        assert_eq!(
            error,
            HookAggregateStoreError::DayBeforeCurrent(DayBeforeCurrent {
                current: day(4),
                observed: day(3),
            })
        );
        assert_eq!(store, before);
    }

    #[test]
    fn selection_rejects_a_tracker_stored_under_another_key() {
        let mut state = state();
        let mut store = HookAggregateStore::new(day(3));
        let recording = recording_at(&mut state, 3);
        let selected_key =
            HookMetricsKey::new(&recording, HookAgent::Claude, HookSurface::PreToolUse);
        let other_key =
            HookMetricsKey::new(&recording, HookAgent::Claude, HookSurface::PostToolUse);
        store
            .entries
            .insert(selected_key, HookSessionCountTracker::new(other_key));
        let before = store.clone();

        let mut staged = store.stage(&recording).unwrap();
        let result = staged.select(HookAgent::Claude, HookSurface::PreToolUse);

        let Err(error) = result else {
            panic!("accepted a tracker stored under another aggregate key");
        };
        assert_eq!(
            error,
            HookAggregateStoreError::PrivateState(HookAggregateSelectionError)
        );
        assert!(staged.is_poisoned());
        assert_eq!(staged.commit(), StageCommit::DiscardedPoisoned);
        assert_eq!(store, before);
    }

    #[test]
    fn identifier_reset_drops_old_trackers_and_selects_a_new_epoch() {
        let mut state = state();
        let mut store = HookAggregateStore::new(day(3));
        let (old_key, completed_at) = {
            let recording = recording_at(&mut state, 3);
            let mut staged = store.stage(&recording).unwrap();
            let key = *staged
                .select(HookAgent::Claude, HookSurface::PreToolUse)
                .unwrap()
                .key();
            assert_eq!(staged.commit(), StageCommit::Applied);
            (key, recording.completed_at())
        };

        store.reset_identifier_epoch();
        state.reset_identifiers(day(3)).unwrap();
        let observation = state.observe_recording(completed_at).unwrap();
        let recording = state.bind_recording_observation(observation).unwrap();
        let mut staged = store.stage(&recording).unwrap();
        let new_key = *staged
            .select(HookAgent::Claude, HookSurface::PreToolUse)
            .unwrap()
            .key();
        assert_eq!(staged.commit(), StageCommit::Applied);

        assert_ne!(new_key, old_key);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn clear_removes_hook_private_state() {
        let mut state = state();
        let mut store = HookAggregateStore::new(day(3));
        let recording = recording_at(&mut state, 3);
        let mut staged = store.stage(&recording).unwrap();
        staged
            .select(HookAgent::Claude, HookSurface::PreToolUse)
            .unwrap();
        assert_eq!(staged.commit(), StageCommit::Applied);

        store.clear();

        assert_eq!(store.len(), 0);
    }
}
