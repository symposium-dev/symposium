//! Daily admission and lookup for plugin-hook aggregates.

use std::{
    collections::{
        BTreeMap,
        btree_map::{Entry, OccupiedEntry, VacantEntry},
    },
    fmt,
};

use super::{
    AdmittedPluginBucket, PluginHookAggregateSelectionError, PluginHookAggregateState,
    PluginHookMetricsKey, SelectedPluginHookAggregate,
};
use crate::telemetry::{
    schema::{HookAgent, HookSurface, PluginHookAttribution, UtcDay},
    state::{
        BoundRecordingObservation, DayBeforeCurrent,
        open_day::OpenDayUpdate,
        public_row_budget::{DailyPublicRowBudget, PublicRowAdmission},
    },
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

    /// Select or admit the aggregate for one plugin-hook attempt.
    ///
    /// Existing public keys do not spend the daily allowance again. Once the
    /// allowance is exhausted, a new public plugin joins the overflow row for
    /// its agent, hook surface, and identifier epoch. A forward UTC-day
    /// transition clears the previous entries and restores the allowance.
    ///
    /// # Errors
    ///
    /// Returns an admission error if the observation day moves backward or
    /// stored private state does not match its map key.
    pub(in crate::telemetry) fn select(
        &mut self,
        recording: &BoundRecordingObservation<'_>,
        agent: HookAgent,
        surface: HookSurface,
        attribution: PluginHookAttribution,
    ) -> Result<SelectedPluginHookAggregate<'_>, PluginHookAdmissionError> {
        if self.public_rows.select_day(recording.day())? == OpenDayUpdate::Advanced {
            self.entries.clear();
        }

        let bucket = AdmittedPluginBucket::from_attribution(recording, attribution);
        let key = PluginHookMetricsKey::new(recording, agent, surface, &bucket);
        if bucket.is_public() && self.public_rows.is_exhausted() {
            return Self::select_public_at_capacity(&mut self.entries, key);
        }

        match self.entries.entry(key) {
            Entry::Occupied(entry) => Self::select_occupied(entry),
            Entry::Vacant(entry) => {
                if bucket.is_public() {
                    let PublicRowAdmission::Public = self.public_rows.admit_new() else {
                        unreachable!("BUG: a non-exhausted public-row budget must admit a row");
                    };
                }
                Self::insert_vacant(entry, bucket)
            }
        }
    }

    fn select_public_at_capacity(
        entries: &mut BTreeMap<PluginHookMetricsKey, PluginHookAggregateState>,
        mut key: PluginHookMetricsKey,
    ) -> Result<SelectedPluginHookAggregate<'_>, PluginHookAdmissionError> {
        // This deliberately performs a read lookup before the mutable one.
        // Returning a selection borrows the map, so a single `entry` match
        // cannot also reuse the map in its vacant branch on stable Rust.
        if entries.contains_key(&key) {
            return Self::select_existing(entries, &key);
        }

        let bucket = AdmittedPluginBucket::overflow();
        key.bucket = bucket.key();
        Self::select_or_insert(entries, key, bucket)
    }

    fn select_existing<'a>(
        entries: &'a mut BTreeMap<PluginHookMetricsKey, PluginHookAggregateState>,
        key: &PluginHookMetricsKey,
    ) -> Result<SelectedPluginHookAggregate<'a>, PluginHookAdmissionError> {
        let Some(entry) = entries.get_mut(key) else {
            return Err(PluginHookAdmissionError::PrivateState(
                PluginHookAggregateSelectionError,
            ));
        };

        entry
            .select(key)
            .map_err(PluginHookAdmissionError::PrivateState)
    }

    fn select_or_insert(
        entries: &mut BTreeMap<PluginHookMetricsKey, PluginHookAggregateState>,
        key: PluginHookMetricsKey,
        bucket: AdmittedPluginBucket,
    ) -> Result<SelectedPluginHookAggregate<'_>, PluginHookAdmissionError> {
        match entries.entry(key) {
            Entry::Occupied(entry) => Self::select_occupied(entry),
            Entry::Vacant(entry) => Self::insert_vacant(entry, bucket),
        }
    }

    fn select_occupied(
        entry: OccupiedEntry<'_, PluginHookMetricsKey, PluginHookAggregateState>,
    ) -> Result<SelectedPluginHookAggregate<'_>, PluginHookAdmissionError> {
        let key = entry.key().clone();
        entry
            .into_mut()
            .select(&key)
            .map_err(PluginHookAdmissionError::PrivateState)
    }

    fn insert_vacant(
        entry: VacantEntry<'_, PluginHookMetricsKey, PluginHookAggregateState>,
        bucket: AdmittedPluginBucket,
    ) -> Result<SelectedPluginHookAggregate<'_>, PluginHookAdmissionError> {
        let state = PluginHookAggregateState::new(entry.key(), bucket);
        let key = entry.key().clone();
        entry
            .insert(state)
            .select(&key)
            .map_err(PluginHookAdmissionError::PrivateState)
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
    const fn admitted_public_rows(&self) -> u64 {
        self.public_rows.admitted()
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Failure while selecting private plugin-hook aggregate state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum PluginHookAdmissionError {
    DayBeforeCurrent(DayBeforeCurrent),
    PrivateState(PluginHookAggregateSelectionError),
}

impl From<DayBeforeCurrent> for PluginHookAdmissionError {
    fn from(error: DayBeforeCurrent) -> Self {
        Self::DayBeforeCurrent(error)
    }
}

impl fmt::Display for PluginHookAdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DayBeforeCurrent(error) => write!(formatter, "plugin-hook admission {error}"),
            Self::PrivateState(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for PluginHookAdmissionError {}

#[cfg(test)]
mod tests;
