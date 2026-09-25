use chrono::{NaiveDate, TimeZone as _, Utc};
use serde_json::json;

use super::*;
use crate::telemetry::{
    schema::UtcSecond,
    state::{
        IDENTIFIER_WINDOW_TEST_STATE, TelemetryStateV1,
        extension_invocation::ExtensionSessionCountSnapshot,
        public_row_budget::MAX_PUBLIC_ROWS_PER_DAY, recording_observation,
    },
};

fn state() -> TelemetryStateV1 {
    toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap()
}

fn day(day: u32) -> UtcDay {
    UtcDay::from_date(NaiveDate::from_ymd_opt(2026, 8, day).unwrap())
}

fn recording_at(
    state: &mut TelemetryStateV1,
    day: u32,
    hour: u32,
) -> BoundRecordingObservation<'_> {
    let completed_at =
        UtcSecond::from_datetime(Utc.with_ymd_and_hms(2026, 8, day, hour, 2, 11).unwrap());
    let observation = state.observe_recording(completed_at).unwrap();
    state.bind_recording_observation(observation).unwrap()
}

fn public(name: &str) -> ExtensionInvocationAttribution {
    ExtensionInvocationAttribution::Public(
        serde_json::from_value(json!({
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

#[test]
fn public_admission_keeps_target_and_subject_together() {
    let mut state = state();
    let recording = recording_observation(&mut state);
    let attribution = public("example-skill");
    let ExtensionInvocationAttribution::Public(safe_attribution) = &attribution else {
        panic!("public helper returned unnamed attribution");
    };
    let expected_target = safe_attribution.target().clone();
    let expected_subject = safe_attribution.derive_subject(recording.identifier_window_scope());
    let mut store = ExtensionInvocationAggregateStore::new(recording.day());

    let mut selected = store
        .select(&recording, ExtensionInvocationAgent::Claude, attribution)
        .unwrap();

    assert_eq!(selected.day(), recording.day());
    assert_eq!(selected.agent(), ExtensionInvocationAgent::Claude);
    assert_eq!(selected.bucket().scope(), ExtensionTargetScope::Public);
    assert_eq!(selected.bucket().target(), Some(&expected_target));
    assert_eq!(selected.bucket().unnamed_reason(), None);
    assert_eq!(
        selected.bucket().extension_subject(),
        Some(expected_subject)
    );
    assert_eq!(
        selected.session_counts().snapshot(),
        ExtensionSessionCountSnapshot::Complete {
            identified_sessions: 0,
            identified_sessions_completed: 0,
        }
    );
}

#[test]
fn unnamed_admission_exposes_only_its_fixed_reason() {
    let mut state = state();
    let recording = recording_observation(&mut state);
    let mut store = ExtensionInvocationAggregateStore::new(recording.day());

    let selected = store
        .select(
            &recording,
            ExtensionInvocationAgent::Claude,
            ExtensionInvocationAttribution::Unnamed(UnnamedExtensionReason::AttributionUnavailable),
        )
        .unwrap();

    assert_eq!(selected.bucket().scope(), ExtensionTargetScope::Unnamed);
    assert_eq!(selected.bucket().target(), None);
    assert_eq!(
        selected.bucket().unnamed_reason(),
        Some(UnnamedExtensionReason::AttributionUnavailable)
    );
    assert_eq!(selected.bucket().extension_subject(), None);
}

#[test]
fn separate_recordings_reuse_the_public_event_id_without_spending_again() {
    let mut state = state();
    let mut store = ExtensionInvocationAggregateStore::new(day(3));

    let first_event_id = {
        let recording = recording_at(&mut state, 3, 10);
        store
            .select(
                &recording,
                ExtensionInvocationAgent::Claude,
                public("example-skill"),
            )
            .unwrap()
            .event_id()
    };
    let recording = recording_at(&mut state, 3, 11);
    let second_event_id = store
        .select(
            &recording,
            ExtensionInvocationAgent::Claude,
            public("example-skill"),
        )
        .unwrap()
        .event_id();

    assert_eq!(second_event_id, first_event_id);
    assert_eq!(store.len(), 1);
    assert_eq!(store.admitted_public_rows(), 1);
}

#[test]
fn the_129th_public_target_joins_one_overflow_row() {
    let mut state = state();
    let recording = recording_observation(&mut state);
    let mut store = ExtensionInvocationAggregateStore::new(recording.day());
    let mut first_event_id = None;

    for index in 0..MAX_PUBLIC_ROWS_PER_DAY {
        let name = format!("skill-{index}");
        let selected = store
            .select(&recording, ExtensionInvocationAgent::Claude, public(&name))
            .unwrap();
        assert_eq!(selected.bucket().scope(), ExtensionTargetScope::Public);
        if index == 0 {
            first_event_id = Some(selected.event_id());
        }
    }

    let repeated_event_id = store
        .select(
            &recording,
            ExtensionInvocationAgent::Claude,
            public("skill-0"),
        )
        .unwrap()
        .event_id();
    let overflow_event_id = store
        .select(
            &recording,
            ExtensionInvocationAgent::Claude,
            public("overflowed-skill"),
        )
        .unwrap()
        .event_id();
    let repeated_overflow = store
        .select(
            &recording,
            ExtensionInvocationAgent::Claude,
            public("another-overflowed-skill"),
        )
        .unwrap();

    assert_eq!(repeated_event_id, first_event_id.unwrap());
    assert_eq!(repeated_overflow.event_id(), overflow_event_id);
    assert_eq!(
        repeated_overflow.bucket().scope(),
        ExtensionTargetScope::Overflow
    );
    assert_eq!(repeated_overflow.bucket().target(), None);
    assert_eq!(repeated_overflow.bucket().unnamed_reason(), None);
    assert_eq!(repeated_overflow.bucket().extension_subject(), None);
    assert_eq!(store.len(), 129);
    assert_eq!(store.admitted_public_rows(), MAX_PUBLIC_ROWS_PER_DAY);
}

#[test]
fn identifier_reset_does_not_restore_the_daily_allowance() {
    let mut state = state();
    let mut store = {
        let recording = recording_observation(&mut state);
        let mut store = ExtensionInvocationAggregateStore::new(recording.day());
        for index in 0..MAX_PUBLIC_ROWS_PER_DAY {
            store
                .select(
                    &recording,
                    ExtensionInvocationAgent::Claude,
                    public(&format!("skill-{index}")),
                )
                .unwrap();
        }
        store
    };

    state.reset_identifiers(day(3)).unwrap();
    store.reset_identifier_epoch();
    let recording = recording_observation(&mut state);
    let selected = store
        .select(
            &recording,
            ExtensionInvocationAgent::Claude,
            public("after-reset"),
        )
        .unwrap();

    assert_eq!(selected.bucket().scope(), ExtensionTargetScope::Overflow);
    assert_eq!(store.len(), 1);
    assert_eq!(store.admitted_public_rows(), MAX_PUBLIC_ROWS_PER_DAY);
}

#[test]
fn clear_restores_the_daily_allowance() {
    let mut state = state();
    let recording = recording_observation(&mut state);
    let mut store = ExtensionInvocationAggregateStore::new(recording.day());
    for index in 0..MAX_PUBLIC_ROWS_PER_DAY {
        store
            .select(
                &recording,
                ExtensionInvocationAgent::Claude,
                public(&format!("skill-{index}")),
            )
            .unwrap();
    }

    store.clear();
    let selected = store
        .select(
            &recording,
            ExtensionInvocationAgent::Claude,
            public("after-clear"),
        )
        .unwrap();

    assert_eq!(selected.bucket().scope(), ExtensionTargetScope::Public);
    assert_eq!(store.len(), 1);
    assert_eq!(store.admitted_public_rows(), 1);
}

#[test]
fn day_rollover_clears_entries_and_restores_the_allowance() {
    let mut state = state();
    let mut store = {
        let recording = recording_observation(&mut state);
        let mut store = ExtensionInvocationAggregateStore::new(recording.day());
        store
            .select(
                &recording,
                ExtensionInvocationAgent::Claude,
                public("day-three"),
            )
            .unwrap();
        store
    };
    let recording = recording_at(&mut state, 4, 10);

    let selected = store
        .select(
            &recording,
            ExtensionInvocationAgent::Claude,
            public("day-four"),
        )
        .unwrap();

    assert_eq!(selected.day(), day(4));
    assert_eq!(selected.bucket().scope(), ExtensionTargetScope::Public);
    assert_eq!(store.len(), 1);
    assert_eq!(store.admitted_public_rows(), 1);
}

#[test]
fn an_older_observation_is_rejected_without_changing_the_store() {
    let mut state = state();
    let recording = recording_observation(&mut state);
    let mut store = ExtensionInvocationAggregateStore::new(day(4));
    let before = store.clone();

    let result = store.select(
        &recording,
        ExtensionInvocationAgent::Claude,
        public("older-observation"),
    );

    assert_eq!(
        result.err(),
        Some(ExtensionInvocationAdmissionError::DayBeforeCurrent(
            DayBeforeCurrent {
                current: day(4),
                observed: day(3),
            },
        ))
    );
    assert_eq!(store, before);
}

#[test]
fn private_state_rejects_another_aggregate_key() {
    let mut state = state();
    let recording = recording_observation(&mut state);
    let first_bucket = AdmittedExtensionBucket::from_attribution(
        &recording,
        ExtensionInvocationAttribution::Unnamed(UnnamedExtensionReason::Ineligible),
    );
    let first_key = ExtensionInvocationAggregateKey::new(
        &recording,
        ExtensionInvocationAgent::Claude,
        &first_bucket,
    );
    let other_bucket = AdmittedExtensionBucket::from_attribution(
        &recording,
        ExtensionInvocationAttribution::Unnamed(UnnamedExtensionReason::Ambiguous),
    );
    let other_key = ExtensionInvocationAggregateKey::new(
        &recording,
        ExtensionInvocationAgent::Claude,
        &other_bucket,
    );
    let mut entry = ExtensionInvocationAggregateState::new(&first_key, first_bucket);

    let result = entry.select(&other_key);

    assert_eq!(result.err(), Some(ExtensionAggregateSelectionError));
}

#[test]
fn private_state_rejects_a_bucket_that_disagrees_with_its_key() {
    let mut state = state();
    let recording = recording_observation(&mut state);
    let selected_bucket = AdmittedExtensionBucket::from_attribution(
        &recording,
        ExtensionInvocationAttribution::Unnamed(UnnamedExtensionReason::Ineligible),
    );
    let selected_key = ExtensionInvocationAggregateKey::new(
        &recording,
        ExtensionInvocationAgent::Claude,
        &selected_bucket,
    );
    let other_bucket = AdmittedExtensionBucket::from_attribution(
        &recording,
        ExtensionInvocationAttribution::Unnamed(UnnamedExtensionReason::Ambiguous),
    );
    let mut entry = ExtensionInvocationAggregateState::new(&selected_key, other_bucket);

    let result = entry.select(&selected_key);

    assert_eq!(result.err(), Some(ExtensionAggregateSelectionError));
}
