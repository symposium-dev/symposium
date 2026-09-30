//! Daily private state for top-level hook aggregate rows.

use std::{
    collections::{BTreeMap, btree_map::Entry},
    fmt,
};

use super::{
    BoundRecordingObservation, DayBeforeCurrent, HookSessionCountTracker,
    open_day::{OpenDay, OpenDayUpdate},
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

    /// Select private state for one hook aggregate.
    ///
    /// A later day drops the closed day's entries before selecting the new
    /// key. Repeated selections for the same day and identifier epoch reuse
    /// the existing tracker.
    ///
    /// # Errors
    ///
    /// Returns [`HookAggregateStoreError::DayBeforeCurrent`] when the
    /// recording belongs to a closed day, or
    /// [`HookAggregateStoreError::PrivateState`] when a stored tracker does
    /// not match its map key.
    pub(in crate::telemetry) fn select(
        &mut self,
        recording: &BoundRecordingObservation<'_>,
        agent: HookAgent,
        hook: HookSurface,
    ) -> Result<&mut HookSessionCountTracker<HookMetricsKey>, HookAggregateStoreError> {
        let day_update = self
            .day
            .select(recording.day())
            .map_err(HookAggregateStoreError::DayBeforeCurrent)?;
        if day_update == OpenDayUpdate::Advanced {
            self.entries.clear();
        }

        let key = HookMetricsKey::new(recording, agent, hook);
        let session_counts = match self.entries.entry(key) {
            Entry::Occupied(entry) => {
                if entry.get().key() != entry.key() {
                    return Err(HookAggregateStoreError::PrivateState(
                        HookAggregateSelectionError,
                    ));
                }

                entry.into_mut()
            }
            Entry::Vacant(entry) => entry.insert(HookSessionCountTracker::new(key)),
        };

        Ok(session_counts)
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
        let first_key = {
            let tracker = store
                .select(&recording, HookAgent::Claude, HookSurface::PreToolUse)
                .unwrap();
            tracker.checked_record(0, None, HookOutcome::Ok).unwrap();
            *tracker.key()
        };

        let tracker = store
            .select(&recording, HookAgent::Claude, HookSurface::PreToolUse)
            .unwrap();

        assert_eq!(tracker.key(), &first_key);
        assert_eq!(tracker.snapshot(), HookSessionCountSnapshot::Incomplete);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn another_hook_surface_gets_another_private_tracker() {
        let mut state = state();
        let mut store = HookAggregateStore::new(day(3));
        let recording = recording_at(&mut state, 3);
        let pre_tool_key = *store
            .select(&recording, HookAgent::Claude, HookSurface::PreToolUse)
            .unwrap()
            .key();

        let post_tool_key = *store
            .select(&recording, HookAgent::Claude, HookSurface::PostToolUse)
            .unwrap()
            .key();

        assert_ne!(post_tool_key, pre_tool_key);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn later_day_drops_closed_day_trackers() {
        let mut state = state();
        let mut store = HookAggregateStore::new(day(3));
        {
            let recording = recording_at(&mut state, 3);
            let tracker = store
                .select(&recording, HookAgent::Claude, HookSurface::PreToolUse)
                .unwrap();
            tracker.checked_record(0, None, HookOutcome::Ok).unwrap();
        }
        let later = recording_at(&mut state, 4);

        let tracker = store
            .select(&later, HookAgent::Claude, HookSurface::PreToolUse)
            .unwrap();

        assert_eq!(
            tracker.snapshot(),
            HookSessionCountSnapshot::Complete {
                identified_sessions: 0,
                identified_sessions_non_ok: 0,
            }
        );
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn earlier_day_is_rejected_without_changing_the_store() {
        let mut state = state();
        let mut store = HookAggregateStore::new(day(4));
        let recording = recording_at(&mut state, 3);
        let before = store.clone();

        let result = store.select(&recording, HookAgent::Claude, HookSurface::PreToolUse);

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

        let result = store.select(&recording, HookAgent::Claude, HookSurface::PreToolUse);

        let Err(error) = result else {
            panic!("accepted a tracker stored under another aggregate key");
        };
        assert_eq!(
            error,
            HookAggregateStoreError::PrivateState(HookAggregateSelectionError)
        );
        assert_eq!(store, before);
    }

    #[test]
    fn identifier_reset_drops_old_trackers_and_selects_a_new_epoch() {
        let mut state = state();
        let mut store = HookAggregateStore::new(day(3));
        let (old_key, completed_at) = {
            let recording = recording_at(&mut state, 3);
            let key = *store
                .select(&recording, HookAgent::Claude, HookSurface::PreToolUse)
                .unwrap()
                .key();
            (key, recording.completed_at())
        };

        store.reset_identifier_epoch();
        state.reset_identifiers(day(3)).unwrap();
        let observation = state.observe_recording(completed_at).unwrap();
        let recording = state.bind_recording_observation(observation).unwrap();
        let new_key = *store
            .select(&recording, HookAgent::Claude, HookSurface::PreToolUse)
            .unwrap()
            .key();

        assert_ne!(new_key, old_key);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn clear_removes_hook_private_state() {
        let mut state = state();
        let mut store = HookAggregateStore::new(day(3));
        let recording = recording_at(&mut state, 3);
        store
            .select(&recording, HookAgent::Claude, HookSurface::PreToolUse)
            .unwrap();

        store.clear();

        assert_eq!(store.len(), 0);
    }
}
