//! Private state belonging to plugin-hook aggregate rows.

use std::fmt;

use super::{
    BoundRecordingObservation, HookSessionCountSnapshot, HookSessionCountTracker,
    HookSessionCountUpdateError,
};
use crate::telemetry::{
    identity::{PluginSubject, SessionId},
    schema::{
        EventId, HookAgent, HookMetricsKey, HookSurface, PluginHookAttribution, PluginHookOutcome,
        PluginScope, PublicPluginCoordinate, UtcDay,
    },
};

mod admission;

#[cfg(test)]
pub(in crate::telemetry) use admission::PluginHookAggregateStore;

/// Identity fields admitted for one plugin-hook aggregate row.
///
/// The inner enum is private so only state admission can create an overflow
/// bucket or pair a public coordinate with its scoped subject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::telemetry) struct AdmittedPluginBucket(AdmittedPluginBucketKind);

#[derive(Debug, Clone, PartialEq, Eq)]
enum AdmittedPluginBucketKind {
    Public {
        plugin: PublicPluginCoordinate,
        subject: PluginSubject,
    },
    Unnamed,
    Overflow,
}

impl AdmittedPluginBucket {
    fn from_attribution(
        recording: &BoundRecordingObservation<'_>,
        attribution: PluginHookAttribution,
    ) -> Self {
        match attribution {
            PluginHookAttribution::Public(plugin) => {
                let subject = recording.identifier_window_scope().derive(&plugin);
                Self(AdmittedPluginBucketKind::Public { plugin, subject })
            }
            PluginHookAttribution::Unnamed => Self(AdmittedPluginBucketKind::Unnamed),
        }
    }

    const fn overflow() -> Self {
        Self(AdmittedPluginBucketKind::Overflow)
    }

    #[must_use]
    pub(in crate::telemetry) const fn scope(&self) -> PluginScope {
        match self.0 {
            AdmittedPluginBucketKind::Public { .. } => PluginScope::Public,
            AdmittedPluginBucketKind::Unnamed => PluginScope::Unnamed,
            AdmittedPluginBucketKind::Overflow => PluginScope::Overflow,
        }
    }

    #[must_use]
    pub(in crate::telemetry) const fn plugin(&self) -> Option<&PublicPluginCoordinate> {
        match &self.0 {
            AdmittedPluginBucketKind::Public { plugin, .. } => Some(plugin),
            AdmittedPluginBucketKind::Unnamed | AdmittedPluginBucketKind::Overflow => None,
        }
    }

    #[must_use]
    pub(in crate::telemetry) const fn plugin_subject(&self) -> Option<PluginSubject> {
        match self.0 {
            AdmittedPluginBucketKind::Public { subject, .. } => Some(subject),
            AdmittedPluginBucketKind::Unnamed | AdmittedPluginBucketKind::Overflow => None,
        }
    }

    const fn key(&self) -> PluginBucketKey {
        match self.0 {
            AdmittedPluginBucketKind::Public { subject, .. } => PluginBucketKey::Public(subject),
            AdmittedPluginBucketKind::Unnamed => PluginBucketKey::Unnamed,
            AdmittedPluginBucketKind::Overflow => PluginBucketKey::Overflow,
        }
    }

    const fn is_public(&self) -> bool {
        matches!(self.0, AdmittedPluginBucketKind::Public { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum PluginBucketKey {
    Public(PluginSubject),
    Unnamed,
    Overflow,
}

/// Stable lookup key shared by a plugin-hook row and its private state.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct PluginHookMetricsKey {
    day: UtcDay,
    // The row needs these plaintext values; `hook` separates identity epochs.
    agent: HookAgent,
    surface: HookSurface,
    hook: HookMetricsKey,
    bucket: PluginBucketKey,
}

impl PluginHookMetricsKey {
    fn new(
        recording: &BoundRecordingObservation<'_>,
        agent: HookAgent,
        surface: HookSurface,
        bucket: &AdmittedPluginBucket,
    ) -> Self {
        Self {
            day: recording.day(),
            agent,
            surface,
            hook: HookMetricsKey::new(recording, agent, surface),
            bucket: bucket.key(),
        }
    }
}

/// Private state paired with one plugin-hook aggregate row.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PluginHookAggregateState {
    event_id: EventId,
    bucket: AdmittedPluginBucket,
    session_counts: HookSessionCountTracker<PluginHookMetricsKey>,
}

impl PluginHookAggregateState {
    /// Start private state for a newly admitted aggregate key.
    #[must_use]
    fn new(key: &PluginHookMetricsKey, bucket: AdmittedPluginBucket) -> Self {
        Self {
            event_id: EventId::new(),
            bucket,
            session_counts: HookSessionCountTracker::new(key.clone()),
        }
    }

    fn select(
        &mut self,
        key: &PluginHookMetricsKey,
    ) -> Result<SelectedPluginHookAggregate<'_>, PluginHookAggregateSelectionError> {
        if self.session_counts.key() != key || self.bucket.key() != key.bucket {
            return Err(PluginHookAggregateSelectionError);
        }

        Ok(SelectedPluginHookAggregate {
            event_id: self.event_id,
            bucket: &self.bucket,
            session_counts: &mut self.session_counts,
        })
    }
}

/// Key-checked access to one admitted plugin-hook aggregate.
pub(in crate::telemetry) struct SelectedPluginHookAggregate<'a> {
    event_id: EventId,
    bucket: &'a AdmittedPluginBucket,
    session_counts: &'a mut HookSessionCountTracker<PluginHookMetricsKey>,
}

impl SelectedPluginHookAggregate<'_> {
    #[must_use]
    pub(in crate::telemetry) const fn event_id(&self) -> EventId {
        self.event_id
    }

    #[must_use]
    pub(in crate::telemetry) const fn day(&self) -> UtcDay {
        self.session_counts.key().day
    }

    #[must_use]
    pub(in crate::telemetry) const fn agent(&self) -> HookAgent {
        self.session_counts.key().agent
    }

    #[must_use]
    pub(in crate::telemetry) const fn surface(&self) -> HookSurface {
        self.session_counts.key().surface
    }

    #[must_use]
    pub(in crate::telemetry) const fn bucket(&self) -> &AdmittedPluginBucket {
        self.bucket
    }

    #[must_use]
    pub(in crate::telemetry) fn matches_recording(
        &self,
        recording: &BoundRecordingObservation<'_>,
    ) -> bool {
        let key = self.session_counts.key();
        key.day == recording.day()
            && key.hook == HookMetricsKey::new(recording, key.agent, key.surface)
    }

    /// Record one observation and return the row fields represented by the
    /// private session sets.
    ///
    /// Keeping this operation on the selection prevents the private aggregate
    /// key type from leaking into the schema layer.
    pub(in crate::telemetry) fn checked_record_session(
        &mut self,
        snapshot_contributions: u64,
        session_id: Option<SessionId>,
        outcome: PluginHookOutcome,
    ) -> Result<HookSessionCountSnapshot, HookSessionCountUpdateError> {
        self.session_counts
            .checked_record(snapshot_contributions, session_id, outcome)?;
        Ok(self.session_counts.snapshot())
    }
}

/// A private-state entry selected with another plugin-hook aggregate key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginHookAggregateSelectionError;

impl fmt::Display for PluginHookAggregateSelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("plugin-hook private state belongs to another aggregate")
    }
}

impl std::error::Error for PluginHookAggregateSelectionError {}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, TimeZone as _, Utc};

    use super::*;
    use crate::telemetry::{
        identity::SessionId,
        schema::{PluginHookOutcome, PublicPluginCoordinate, UtcDay, UtcSecond},
        state::{IDENTIFIER_WINDOW_TEST_STATE, TelemetryStateV1, recording_observation},
    };

    fn state() -> TelemetryStateV1 {
        toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap()
    }

    fn aggregate(
        recording: &BoundRecordingObservation<'_>,
        surface: HookSurface,
    ) -> (PluginHookAggregateState, PluginHookMetricsKey) {
        let bucket =
            AdmittedPluginBucket::from_attribution(recording, PluginHookAttribution::Unnamed);
        let key = PluginHookMetricsKey::new(recording, HookAgent::Claude, surface, &bucket);
        let aggregate = PluginHookAggregateState::new(&key, bucket);
        (aggregate, key)
    }

    fn public_plugin(name: &str) -> PublicPluginCoordinate {
        serde_json::from_value(serde_json::json!({
            "source": "symposium-recommendations",
            "name": name,
        }))
        .unwrap()
    }

    #[test]
    fn plugin_hook_key_is_stable_and_separates_plugins_hooks_days_and_epochs() {
        let plugin = public_plugin("example-tools");
        let other_plugin = public_plugin("other-tools");
        let mut telemetry_state = state();
        let (baseline, another_plugin, another_hook, unnamed, baseline_day) = {
            let recording = recording_observation(&mut telemetry_state);
            let public_bucket = AdmittedPluginBucket::from_attribution(
                &recording,
                PluginHookAttribution::Public(plugin.clone()),
            );
            let other_bucket = AdmittedPluginBucket::from_attribution(
                &recording,
                PluginHookAttribution::Public(other_plugin),
            );
            let unnamed_bucket =
                AdmittedPluginBucket::from_attribution(&recording, PluginHookAttribution::Unnamed);

            (
                PluginHookMetricsKey::new(
                    &recording,
                    HookAgent::Claude,
                    HookSurface::PreToolUse,
                    &public_bucket,
                ),
                PluginHookMetricsKey::new(
                    &recording,
                    HookAgent::Claude,
                    HookSurface::PreToolUse,
                    &other_bucket,
                ),
                PluginHookMetricsKey::new(
                    &recording,
                    HookAgent::Claude,
                    HookSurface::PostToolUse,
                    &public_bucket,
                ),
                PluginHookMetricsKey::new(
                    &recording,
                    HookAgent::Claude,
                    HookSurface::PreToolUse,
                    &unnamed_bucket,
                ),
                recording.day(),
            )
        };
        let same_day_at =
            UtcSecond::from_datetime(Utc.with_ymd_and_hms(2026, 8, 3, 11, 2, 11).unwrap());
        let same_day = telemetry_state.observe_recording(same_day_at).unwrap();
        let same_day = telemetry_state
            .bind_recording_observation(same_day)
            .unwrap();
        let same_bucket = AdmittedPluginBucket::from_attribution(
            &same_day,
            PluginHookAttribution::Public(plugin.clone()),
        );
        let same_selection = PluginHookMetricsKey::new(
            &same_day,
            HookAgent::Claude,
            HookSurface::PreToolUse,
            &same_bucket,
        );
        let later_at =
            UtcSecond::from_datetime(Utc.with_ymd_and_hms(2026, 8, 4, 10, 2, 11).unwrap());
        let later = telemetry_state.observe_recording(later_at).unwrap();
        let later = telemetry_state.bind_recording_observation(later).unwrap();
        let later_bucket =
            AdmittedPluginBucket::from_attribution(&later, PluginHookAttribution::Public(plugin));
        let another_day = PluginHookMetricsKey::new(
            &later,
            HookAgent::Claude,
            HookSurface::PreToolUse,
            &later_bucket,
        );

        let mut reset_state = state();
        reset_state.reset_identifiers(baseline_day).unwrap();
        let reset_recording = recording_observation(&mut reset_state);
        let reset_bucket = AdmittedPluginBucket::from_attribution(
            &reset_recording,
            PluginHookAttribution::Unnamed,
        );
        let another_epoch = PluginHookMetricsKey::new(
            &reset_recording,
            HookAgent::Claude,
            HookSurface::PreToolUse,
            &reset_bucket,
        );

        assert_eq!(baseline, same_selection);
        assert_ne!(baseline, another_plugin);
        assert_ne!(baseline, another_hook);
        assert_ne!(baseline, another_day);
        assert_ne!(unnamed, another_epoch);
    }

    #[test]
    fn selection_keeps_one_event_id_bucket_and_tracker_with_their_key() {
        let mut state = state();
        let recording = recording_observation(&mut state);
        let (mut aggregate, key) = aggregate(&recording, HookSurface::PreToolUse);

        let first_event_id = aggregate.select(&key).unwrap().event_id();
        let selected = aggregate.select(&key).unwrap();

        assert_eq!(selected.event_id(), first_event_id);
        assert_eq!(selected.bucket().scope(), PluginScope::Unnamed);
    }

    #[test]
    fn selection_rejects_another_aggregate_key() {
        let mut state = state();
        let recording = recording_observation(&mut state);
        let (mut aggregate_state, _) = aggregate(&recording, HookSurface::PreToolUse);
        let (_, other_key) = aggregate(&recording, HookSurface::PostToolUse);

        let result = aggregate_state.select(&other_key);

        assert!(matches!(result, Err(PluginHookAggregateSelectionError)));
    }

    #[test]
    fn selection_rejects_the_same_bucket_after_an_identifier_reset() {
        let mut state = state();
        let (mut aggregate, _) = {
            let recording = recording_observation(&mut state);
            aggregate(&recording, HookSurface::PreToolUse)
        };
        state
            .reset_identifiers(UtcDay::from_date(
                NaiveDate::from_ymd_opt(2026, 8, 3).unwrap(),
            ))
            .unwrap();
        let recording = recording_observation(&mut state);
        let bucket =
            AdmittedPluginBucket::from_attribution(&recording, PluginHookAttribution::Unnamed);
        let key = PluginHookMetricsKey::new(
            &recording,
            HookAgent::Claude,
            HookSurface::PreToolUse,
            &bucket,
        );

        let result = aggregate.select(&key);

        assert!(matches!(result, Err(PluginHookAggregateSelectionError)));
    }

    #[test]
    fn debug_output_omits_tracked_session_identifiers() {
        let mut state = state();
        let recording = recording_observation(&mut state);
        let (mut aggregate, key) = aggregate(&recording, HookSurface::PreToolUse);
        let session_id: SessionId = "sess_00000000000000000000000000000001".parse().unwrap();
        aggregate
            .select(&key)
            .unwrap()
            .checked_record_session(0, Some(session_id), PluginHookOutcome::Ok)
            .unwrap();

        let debug = format!("{aggregate:?}");

        assert!(!debug.contains("sess_00000000000000000000000000000001"));
        assert!(debug.contains("identified_sessions: 1"));
    }

    #[test]
    fn overflow_bucket_exposes_no_public_identity() {
        let bucket = AdmittedPluginBucket::overflow();

        assert_eq!(bucket.scope(), PluginScope::Overflow);
        assert_eq!(bucket.plugin(), None);
        assert_eq!(bucket.plugin_subject(), None);
    }
}
