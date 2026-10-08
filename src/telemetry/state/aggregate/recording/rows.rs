//! Row selection and mutation for one staged hook invocation.

use crate::telemetry::{
    schema::{
        AggregateRow, EventId, ExtensionInvocationAgent, ExtensionInvocationAttribution,
        ExtensionInvocationMetricObservation, ExtensionInvocationMetricsV1, HookMetricObservation,
        HookMetricsV1, PluginHookAttempt, PluginHookAttribution, PluginHookMetricObservation,
        PluginHookMetricsV1,
    },
    state::{
        BoundRecordingObservation, extension_invocation::ExtensionInvocationAggregateStage,
        hook::HookAggregateStage, plugin_hook::PluginHookAggregateStage,
    },
    storage::metrics::{AggregateRecoveryIndex, MetricSnapshot, MetricSnapshotEditError},
};

use super::{AggregateRecordingError, HookInvocationTarget};
use crate::telemetry::state::aggregate::AggregateFamily;

pub(super) fn record_hook_row(
    snapshot: &mut MetricSnapshot,
    recovery: &AggregateRecoveryIndex,
    recording: &BoundRecordingObservation<'_>,
    stage: &mut HookAggregateStage<'_, '_, '_>,
    observation: HookMetricObservation<'_>,
) -> Result<(), AggregateRecordingError> {
    let session_counts = stage
        .select(observation.agent, observation.hook)
        .map_err(AggregateRecordingError::HookState)?;
    let key = *session_counts.key();

    let row = match recovery.hook_event_id(key) {
        Some(event_id) => {
            let mut row = hook_row(snapshot, event_id)?;
            row.checked_record(recording, observation, session_counts)
                .map_err(AggregateRecordingError::HookRow)?;
            return snapshot
                .replace(AggregateRow::Hook(Box::new(row)))
                .map_err(AggregateRecordingError::Snapshot);
        }
        None => HookMetricsV1::new(recording, observation, session_counts)
            .map_err(AggregateRecordingError::HookRow)?,
    };

    snapshot
        .insert(AggregateRow::Hook(Box::new(row)))
        .map_err(AggregateRecordingError::Snapshot)
}

pub(super) fn record_plugin_hook_row(
    snapshot: &mut MetricSnapshot,
    recording: &BoundRecordingObservation<'_>,
    stage: &mut PluginHookAggregateStage<'_, '_, '_>,
    target: HookInvocationTarget<'_>,
    attribution: PluginHookAttribution,
    attempt: PluginHookAttempt,
) -> Result<(), AggregateRecordingError> {
    let selected = stage
        .select(target.agent, target.hook, attribution)
        .map_err(AggregateRecordingError::PluginHookState)?;
    let event_id = selected.event_id();
    let observation = PluginHookMetricObservation {
        attempt,
        vendor_session_id: target.vendor_session_id,
    };

    let row = match plugin_hook_row(snapshot, event_id)? {
        Some(mut row) => {
            row.checked_record(recording, observation, selected)
                .map_err(AggregateRecordingError::PluginHookRow)?;
            return snapshot
                .replace(AggregateRow::PluginHook(Box::new(row)))
                .map_err(AggregateRecordingError::Snapshot);
        }
        None => PluginHookMetricsV1::new(recording, observation, selected)
            .map_err(AggregateRecordingError::PluginHookRow)?,
    };

    snapshot
        .insert(AggregateRow::PluginHook(Box::new(row)))
        .map_err(AggregateRecordingError::Snapshot)
}

pub(super) fn record_extension_row(
    snapshot: &mut MetricSnapshot,
    recording: &BoundRecordingObservation<'_>,
    stage: &mut ExtensionInvocationAggregateStage<'_, '_, '_>,
    agent: ExtensionInvocationAgent,
    attribution: ExtensionInvocationAttribution,
    observation: ExtensionInvocationMetricObservation<'_>,
) -> Result<(), AggregateRecordingError> {
    let selected = stage
        .select(agent, attribution)
        .map_err(AggregateRecordingError::ExtensionState)?;
    let event_id = selected.event_id();

    let row = match extension_row(snapshot, event_id)? {
        Some(mut row) => {
            row.checked_record(recording, observation, selected)
                .map_err(AggregateRecordingError::ExtensionRow)?;
            return snapshot
                .replace(AggregateRow::ExtensionInvocation(Box::new(row)))
                .map_err(AggregateRecordingError::Snapshot);
        }
        None => ExtensionInvocationMetricsV1::new(recording, observation, selected)
            .map_err(AggregateRecordingError::ExtensionRow)?,
    };

    snapshot
        .insert(AggregateRow::ExtensionInvocation(Box::new(row)))
        .map_err(AggregateRecordingError::Snapshot)
}

fn hook_row(
    snapshot: &MetricSnapshot,
    event_id: EventId,
) -> Result<HookMetricsV1, AggregateRecordingError> {
    match snapshot.row(event_id) {
        Some(AggregateRow::Hook(row)) => Ok((**row).clone()),
        Some(_) => Err(AggregateRecordingError::SnapshotRowKindChanged {
            event_id,
            expected: AggregateFamily::Hook,
        }),
        None => Err(AggregateRecordingError::Snapshot(
            MetricSnapshotEditError::MissingEventId(event_id),
        )),
    }
}

fn plugin_hook_row(
    snapshot: &MetricSnapshot,
    event_id: EventId,
) -> Result<Option<PluginHookMetricsV1>, AggregateRecordingError> {
    match snapshot.row(event_id) {
        Some(AggregateRow::PluginHook(row)) => Ok(Some((**row).clone())),
        Some(_) => Err(AggregateRecordingError::SnapshotRowKindChanged {
            event_id,
            expected: AggregateFamily::PluginHook,
        }),
        None => Ok(None),
    }
}

fn extension_row(
    snapshot: &MetricSnapshot,
    event_id: EventId,
) -> Result<Option<ExtensionInvocationMetricsV1>, AggregateRecordingError> {
    match snapshot.row(event_id) {
        Some(AggregateRow::ExtensionInvocation(row)) => Ok(Some((**row).clone())),
        Some(_) => Err(AggregateRecordingError::SnapshotRowKindChanged {
            event_id,
            expected: AggregateFamily::ExtensionInvocation,
        }),
        None => Ok(None),
    }
}
