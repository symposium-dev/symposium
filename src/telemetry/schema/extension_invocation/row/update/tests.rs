use chrono::{TimeZone as _, Utc};
use serde_json::json;

use super::*;
use crate::telemetry::{
    schema::{
        RowClassification, RowKind, TelemetryRow, UtcSecond, classify_row,
        extension_invocation::{
            ExtensionInvocationAgent, ExtensionInvocationAttribution, ExtensionTargetScope,
            UnnamedExtensionReason,
        },
        resolution::extension::SafeSkillAttribution,
    },
    state::{
        ExtensionInvocationAggregateStore, IDENTIFIER_WINDOW_TEST_STATE, MAX_PUBLIC_ROWS_PER_DAY,
        TelemetryStateV1, recording_observation,
    },
};

fn state() -> TelemetryStateV1 {
    toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap()
}

fn public(name: &str) -> ExtensionInvocationAttribution {
    ExtensionInvocationAttribution::Public(
        serde_json::from_value::<SafeSkillAttribution>(json!({
            "target": {
                "type": "skill",
                "source": "symposium-recommendations",
                "name": name,
            },
            "path": [{"type": "not"}],
        }))
        .unwrap(),
    )
}

const fn unnamed(reason: UnnamedExtensionReason) -> ExtensionInvocationAttribution {
    ExtensionInvocationAttribution::Unnamed(reason)
}

const fn metric_observation(
    phase: ExtensionInvocationPhase,
    vendor_session_id: Option<&VendorSessionId>,
) -> ExtensionInvocationMetricObservation<'_> {
    ExtensionInvocationMetricObservation {
        phase,
        vendor_session_id,
    }
}

fn recording_at(state: &mut TelemetryStateV1, day: u32) -> BoundRecordingObservation<'_> {
    let completed_at =
        UtcSecond::from_datetime(Utc.with_ymd_and_hms(2026, 8, day, 10, 2, 11).unwrap());
    let observation = state.observe_recording(completed_at).unwrap();
    state.bind_recording_observation(observation).unwrap()
}

fn new_row(
    store: &mut ExtensionInvocationAggregateStore,
    recording: &BoundRecordingObservation<'_>,
    attribution: ExtensionInvocationAttribution,
    observation: ExtensionInvocationMetricObservation<'_>,
) -> ExtensionInvocationMetricsV1 {
    let selected = store
        .select(recording, ExtensionInvocationAgent::Claude, attribution)
        .unwrap();

    ExtensionInvocationMetricsV1::new(recording, observation, selected).unwrap()
}

#[test]
fn first_public_attempt_builds_identity_and_complete_session_counts_from_selection() {
    let mut state = state();
    let recording = recording_observation(&mut state);
    let mut store = ExtensionInvocationAggregateStore::new(recording.day());
    let vendor_session_id = VendorSessionId::new("vendor-session-123".to_owned());
    let selected = store
        .select(
            &recording,
            ExtensionInvocationAgent::Claude,
            public("example-debugging"),
        )
        .unwrap();
    let event_id = selected.event_id();
    let expected_target = selected.bucket().target().cloned();
    let expected_subject = selected.bucket().extension_subject();

    let row = ExtensionInvocationMetricsV1::new(
        &recording,
        metric_observation(
            ExtensionInvocationPhase::Attempted,
            Some(&vendor_session_id),
        ),
        selected,
    )
    .unwrap();
    let json = serde_json::to_string(&row).unwrap();

    assert_eq!(row.kind, RowKind::ExtensionInvocationMetrics);
    assert_eq!(row.event_id, event_id);
    assert_eq!(row.day, recording.day());
    assert_eq!(row.agent, ExtensionInvocationAgent::Claude);
    assert_eq!(row.target_scope, ExtensionTargetScope::Public);
    assert_eq!(row.target, expected_target);
    assert_eq!(row.unnamed_reason, None);
    assert_eq!(row.extension_subject, expected_subject);
    assert_eq!(row.attempted, 1);
    assert_eq!(row.completed, 0);
    assert_eq!(row.failed, 0);
    assert!(row.session_counts_complete);
    assert_eq!(row.identified_sessions, Some(1));
    assert_eq!(row.identified_sessions_completed, Some(0));
    assert!(matches!(
        classify_row(&json),
        RowClassification::Supported(TelemetryRow::ExtensionInvocationMetrics(_))
    ));
}

#[test]
fn missing_snapshot_row_recovers_existing_private_state_with_incomplete_counts() {
    let mut state = state();
    let recording = recording_observation(&mut state);
    let mut store = ExtensionInvocationAggregateStore::new(recording.day());
    let vendor_session_id = VendorSessionId::new("vendor-session-123".to_owned());
    let dropped_row = new_row(
        &mut store,
        &recording,
        public("example-debugging"),
        metric_observation(
            ExtensionInvocationPhase::Attempted,
            Some(&vendor_session_id),
        ),
    );
    let selected = store
        .select(
            &recording,
            ExtensionInvocationAgent::Claude,
            public("example-debugging"),
        )
        .unwrap();

    let recovered = ExtensionInvocationMetricsV1::new(
        &recording,
        metric_observation(
            ExtensionInvocationPhase::Completed,
            Some(&vendor_session_id),
        ),
        selected,
    )
    .unwrap();

    assert_eq!(recovered.event_id, dropped_row.event_id);
    assert_eq!(
        (recovered.attempted, recovered.completed, recovered.failed),
        (0, 1, 0)
    );
    assert!(!recovered.session_counts_complete);
    assert_eq!(recovered.identified_sessions, None);
    assert_eq!(recovered.identified_sessions_completed, None);
}

#[test]
fn first_completed_and_failed_observations_keep_phase_counts_independent() {
    let mut state = state();
    let recording = recording_observation(&mut state);
    let vendor_session_id = VendorSessionId::new("vendor-session-123".to_owned());
    let mut completed_store = ExtensionInvocationAggregateStore::new(recording.day());
    let completed = new_row(
        &mut completed_store,
        &recording,
        unnamed(UnnamedExtensionReason::NotIndexed),
        metric_observation(
            ExtensionInvocationPhase::Completed,
            Some(&vendor_session_id),
        ),
    );
    let mut failed_store = ExtensionInvocationAggregateStore::new(recording.day());

    let failed = new_row(
        &mut failed_store,
        &recording,
        unnamed(UnnamedExtensionReason::InvalidSignal),
        metric_observation(ExtensionInvocationPhase::Failed, None),
    );

    assert_eq!(
        (completed.attempted, completed.completed, completed.failed),
        (0, 1, 0)
    );
    assert_eq!(completed.identified_sessions, Some(0));
    assert_eq!(completed.identified_sessions_completed, Some(1));
    assert_eq!(
        (failed.attempted, failed.completed, failed.failed),
        (0, 0, 1)
    );
    assert!(failed.session_counts_complete);
    assert_eq!(failed.identified_sessions, Some(0));
    assert_eq!(failed.identified_sessions_completed, Some(0));
}

#[test]
fn unnamed_and_overflow_rows_take_their_complete_identity_from_admission() {
    let mut state = state();
    let recording = recording_observation(&mut state);
    let mut unnamed_store = ExtensionInvocationAggregateStore::new(recording.day());
    let unnamed = new_row(
        &mut unnamed_store,
        &recording,
        unnamed(UnnamedExtensionReason::Ambiguous),
        metric_observation(ExtensionInvocationPhase::Attempted, None),
    );
    let mut overflow_store = ExtensionInvocationAggregateStore::new(recording.day());
    for index in 0..MAX_PUBLIC_ROWS_PER_DAY {
        let _selected = overflow_store
            .select(
                &recording,
                ExtensionInvocationAgent::Claude,
                public(&format!("skill-{index}")),
            )
            .unwrap();
    }

    let overflow = new_row(
        &mut overflow_store,
        &recording,
        public("overflowed-skill"),
        metric_observation(ExtensionInvocationPhase::Attempted, None),
    );

    assert_eq!(unnamed.target_scope, ExtensionTargetScope::Unnamed);
    assert_eq!(unnamed.target, None);
    assert_eq!(
        unnamed.unnamed_reason,
        Some(UnnamedExtensionReason::Ambiguous)
    );
    assert_eq!(unnamed.extension_subject, None);
    assert_eq!(overflow.target_scope, ExtensionTargetScope::Overflow);
    assert_eq!(overflow.target, None);
    assert_eq!(overflow.unnamed_reason, None);
    assert_eq!(overflow.extension_subject, None);
}

#[test]
fn later_phases_accumulate_and_round_trip_as_one_supported_row() {
    let mut state = state();
    let recording = recording_observation(&mut state);
    let mut store = ExtensionInvocationAggregateStore::new(recording.day());
    let vendor_session_id = VendorSessionId::new("vendor-session-123".to_owned());
    let mut row = new_row(
        &mut store,
        &recording,
        public("example-debugging"),
        metric_observation(
            ExtensionInvocationPhase::Attempted,
            Some(&vendor_session_id),
        ),
    );
    for phase in [
        ExtensionInvocationPhase::Completed,
        ExtensionInvocationPhase::Failed,
    ] {
        let selected = store
            .select(
                &recording,
                ExtensionInvocationAgent::Claude,
                public("example-debugging"),
            )
            .unwrap();
        row.checked_record(
            &recording,
            metric_observation(phase, Some(&vendor_session_id)),
            selected,
        )
        .unwrap();
    }
    let json = serde_json::to_string(&row).unwrap();

    let RowClassification::Supported(TelemetryRow::ExtensionInvocationMetrics(decoded)) =
        classify_row(&json)
    else {
        panic!("updated extension-invocation row was not supported");
    };

    assert_eq!((row.attempted, row.completed, row.failed), (1, 1, 1));
    assert_eq!(row.identified_sessions, Some(1));
    assert_eq!(row.identified_sessions_completed, Some(1));
    assert_eq!(*decoded, row);
}

#[test]
fn missing_attempted_session_makes_both_published_sets_incomplete() {
    let mut state = state();
    let recording = recording_observation(&mut state);
    let mut store = ExtensionInvocationAggregateStore::new(recording.day());

    let row = new_row(
        &mut store,
        &recording,
        public("example-debugging"),
        metric_observation(ExtensionInvocationPhase::Attempted, None),
    );

    assert!(!row.session_counts_complete);
    assert_eq!(row.identified_sessions, None);
    assert_eq!(row.identified_sessions_completed, None);
}

#[test]
fn failed_observation_changes_only_the_row_counter() {
    let mut state = state();
    let recording = recording_observation(&mut state);
    let mut store = ExtensionInvocationAggregateStore::new(recording.day());
    let vendor_session_id = VendorSessionId::new("vendor-session-123".to_owned());
    let mut row = new_row(
        &mut store,
        &recording,
        public("example-debugging"),
        metric_observation(
            ExtensionInvocationPhase::Attempted,
            Some(&vendor_session_id),
        ),
    );
    let store_before = store.clone();
    let selected = store
        .select(
            &recording,
            ExtensionInvocationAgent::Claude,
            public("example-debugging"),
        )
        .unwrap();

    row.checked_record(
        &recording,
        metric_observation(ExtensionInvocationPhase::Failed, None),
        selected,
    )
    .unwrap();

    assert_eq!(store, store_before);
    assert_eq!((row.attempted, row.completed, row.failed), (1, 0, 1));
    assert_eq!(row.identified_sessions, Some(1));
    assert_eq!(row.identified_sessions_completed, Some(0));
}

#[test]
fn baseline_mismatch_marks_both_session_counts_incomplete() {
    let mut state = state();
    let recording = recording_observation(&mut state);
    let mut store = ExtensionInvocationAggregateStore::new(recording.day());
    let vendor_session_id = VendorSessionId::new("vendor-session-123".to_owned());
    let mut row = new_row(
        &mut store,
        &recording,
        public("example-debugging"),
        metric_observation(
            ExtensionInvocationPhase::Attempted,
            Some(&vendor_session_id),
        ),
    );
    row.attempted = 5;
    let selected = store
        .select(
            &recording,
            ExtensionInvocationAgent::Claude,
            public("example-debugging"),
        )
        .unwrap();

    row.checked_record(
        &recording,
        metric_observation(
            ExtensionInvocationPhase::Attempted,
            Some(&vendor_session_id),
        ),
        selected,
    )
    .unwrap();

    assert_eq!(row.attempted, 6);
    assert!(!row.session_counts_complete);
    assert_eq!(row.identified_sessions, None);
    assert_eq!(row.identified_sessions_completed, None);
}

#[test]
fn every_phase_counter_overflow_rejects_without_mutating_row_or_private_state() {
    let mut state = state();
    let recording = recording_observation(&mut state);
    let mut store = ExtensionInvocationAggregateStore::new(recording.day());
    let row = new_row(
        &mut store,
        &recording,
        public("example-debugging"),
        metric_observation(ExtensionInvocationPhase::Attempted, None),
    );

    for phase in [
        ExtensionInvocationPhase::Attempted,
        ExtensionInvocationPhase::Completed,
        ExtensionInvocationPhase::Failed,
    ] {
        let mut overflowing = row.clone();
        match phase {
            ExtensionInvocationPhase::Attempted => overflowing.attempted = u64::MAX,
            ExtensionInvocationPhase::Completed => overflowing.completed = u64::MAX,
            ExtensionInvocationPhase::Failed => overflowing.failed = u64::MAX,
        }
        let row_before = overflowing.clone();
        let store_before = store.clone();
        let selected = store
            .select(
                &recording,
                ExtensionInvocationAgent::Claude,
                public("example-debugging"),
            )
            .unwrap();

        let result =
            overflowing.checked_record(&recording, metric_observation(phase, None), selected);

        assert_eq!(
            result,
            Err(ExtensionInvocationMetricsUpdateError::PhaseCountOverflow { phase })
        );
        assert_eq!(overflowing, row_before);
        assert_eq!(store, store_before);
    }
}

#[test]
fn update_rejects_another_target_without_mutation() {
    let mut state = state();
    let recording = recording_observation(&mut state);
    let mut store = ExtensionInvocationAggregateStore::new(recording.day());
    let mut row = new_row(
        &mut store,
        &recording,
        public("first-skill"),
        metric_observation(ExtensionInvocationPhase::Attempted, None),
    );
    let selected = store
        .select(
            &recording,
            ExtensionInvocationAgent::Claude,
            public("second-skill"),
        )
        .unwrap();
    let row_before = row.clone();

    let result = row.checked_record(
        &recording,
        metric_observation(ExtensionInvocationPhase::Completed, None),
        selected,
    );

    assert_eq!(
        result,
        Err(ExtensionInvocationMetricsUpdateError::TargetIdentityChanged)
    );
    assert_eq!(row, row_before);
}

#[test]
fn update_rejects_private_state_from_another_row_without_mutation() {
    let mut state = state();
    let recording = recording_observation(&mut state);
    let mut first_store = ExtensionInvocationAggregateStore::new(recording.day());
    let mut row = new_row(
        &mut first_store,
        &recording,
        public("example-debugging"),
        metric_observation(ExtensionInvocationPhase::Attempted, None),
    );
    let mut second_store = ExtensionInvocationAggregateStore::new(recording.day());
    let _second_row = new_row(
        &mut second_store,
        &recording,
        public("example-debugging"),
        metric_observation(ExtensionInvocationPhase::Attempted, None),
    );
    let second_store_before = second_store.clone();
    let selected = second_store
        .select(
            &recording,
            ExtensionInvocationAgent::Claude,
            public("example-debugging"),
        )
        .unwrap();
    let row_before = row.clone();

    let result = row.checked_record(
        &recording,
        metric_observation(ExtensionInvocationPhase::Completed, None),
        selected,
    );

    assert_eq!(
        result,
        Err(ExtensionInvocationMetricsUpdateError::RowIdentifierChanged)
    );
    assert_eq!(row, row_before);
    assert_eq!(second_store, second_store_before);
}

#[test]
fn update_rejects_a_selection_from_another_identifier_epoch() {
    let mut state = state();
    let mut store;
    let mut row;
    {
        let recording = recording_observation(&mut state);
        store = ExtensionInvocationAggregateStore::new(recording.day());
        row = new_row(
            &mut store,
            &recording,
            public("example-debugging"),
            metric_observation(ExtensionInvocationPhase::Attempted, None),
        );
    }
    let selected = {
        let recording = recording_observation(&mut state);
        store
            .select(
                &recording,
                ExtensionInvocationAgent::Claude,
                public("example-debugging"),
            )
            .unwrap()
    };
    state.reset_identifiers(row.day).unwrap();
    let recording = recording_observation(&mut state);
    let row_before = row.clone();

    let result = row.checked_record(
        &recording,
        metric_observation(ExtensionInvocationPhase::Completed, None),
        selected,
    );

    assert_eq!(
        result,
        Err(ExtensionInvocationMetricsUpdateError::RecordingContextChanged)
    );
    assert_eq!(row, row_before);
}

#[test]
fn day_rollover_rejects_the_previous_days_row_from_the_same_store() {
    let mut state = state();
    let mut store;
    let mut row;
    {
        let recording = recording_observation(&mut state);
        store = ExtensionInvocationAggregateStore::new(recording.day());
        row = new_row(
            &mut store,
            &recording,
            public("example-debugging"),
            metric_observation(ExtensionInvocationPhase::Attempted, None),
        );
    }
    let row_before = row.clone();
    let recording = recording_at(&mut state, 4);
    let selected = store
        .select(
            &recording,
            ExtensionInvocationAgent::Claude,
            public("example-debugging"),
        )
        .unwrap();

    let result = row.checked_record(
        &recording,
        metric_observation(ExtensionInvocationPhase::Completed, None),
        selected,
    );

    assert_eq!(
        result,
        Err(ExtensionInvocationMetricsUpdateError::DayChanged {
            row_day: row_before.day,
            selected_day: recording.day(),
        })
    );
    assert_eq!(row, row_before);
}
