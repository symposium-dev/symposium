use std::time::Duration;

use super::*;
use crate::telemetry::schema::UnnamedExtensionReason;

fn plugin(terminal: bool) -> PluginHookInvocationObservation {
    PluginHookInvocationObservation {
        attribution: PluginHookAttribution::Unnamed,
        terminal: terminal.then_some(PluginHookAttempt::NotExecuted {
            prepare_duration: Duration::from_millis(2),
        }),
    }
}

fn extension(phase: ExtensionInvocationPhase) -> ExtensionInvocationObservation {
    ExtensionInvocationObservation {
        attribution: ExtensionInvocationAttribution::Unnamed(
            UnnamedExtensionReason::AttributionUnavailable,
        ),
        phase,
    }
}

fn observation(
    agent: HookAgent,
    hook: HookSurface,
    extension: Option<ExtensionInvocationObservation>,
) -> HookInvocationMetricObservation<'static> {
    HookInvocationMetricObservation {
        agent,
        hook,
        outcome: HookOutcome::Ok,
        duration: Duration::from_millis(12),
        vendor_session_id: None,
        plugin_attempts: vec![plugin(true), plugin(false), plugin(true)],
        extension,
    }
}

#[test]
fn normalization_derives_plugin_totals_from_the_complete_collection() {
    let normalized = NormalizedHookInvocation::new(observation(
        HookAgent::Claude,
        HookSurface::PreToolUse,
        None,
    ))
    .unwrap();

    assert_eq!(normalized.plugins_attempted, 3);
    assert_eq!(normalized.plugins_completed, 2);
    assert_eq!(normalized.plugin_attempts.len(), 3);
    assert_eq!(normalized.target.agent, HookAgent::Claude);
    assert_eq!(normalized.target.hook, HookSurface::PreToolUse);
    assert!(normalized.target.vendor_session_id.is_none());
    assert_eq!(normalized.outcome, HookOutcome::Ok);
    assert_eq!(normalized.duration, Duration::from_millis(12));
}

#[test]
fn only_claude_can_supply_an_extension_invocation() {
    let result = NormalizedHookInvocation::new(observation(
        HookAgent::Codex,
        HookSurface::PreToolUse,
        Some(extension(ExtensionInvocationPhase::Attempted)),
    ));

    assert!(matches!(
        result,
        Err(AggregateRecordingError::UnsupportedExtensionAgent(
            HookAgent::Codex
        ))
    ));
}

#[test]
fn extension_phase_must_match_the_top_level_hook_surface() {
    let result = NormalizedHookInvocation::new(observation(
        HookAgent::Claude,
        HookSurface::PreToolUse,
        Some(extension(ExtensionInvocationPhase::Completed)),
    ));

    assert!(matches!(
        result,
        Err(AggregateRecordingError::ExtensionPhaseDoesNotMatchHook {
            hook: HookSurface::PreToolUse,
            phase: ExtensionInvocationPhase::Completed,
        })
    ));
}

#[test]
fn supported_hook_phase_pairs_keep_the_extension_input() {
    let cases = [
        (HookSurface::PreToolUse, ExtensionInvocationPhase::Attempted),
        (
            HookSurface::PostToolUse,
            ExtensionInvocationPhase::Completed,
        ),
    ];

    for (hook, phase) in cases {
        let normalized = NormalizedHookInvocation::new(observation(
            HookAgent::Claude,
            hook,
            Some(extension(phase)),
        ))
        .unwrap();

        assert!(matches!(
            normalized.extension,
            Some((ExtensionInvocationAgent::Claude, observation))
                if observation.phase == phase
        ));
    }
}

#[test]
fn normalization_errors_use_wire_labels() {
    let unsupported = AggregateRecordingError::UnsupportedExtensionAgent(HookAgent::Codex);
    let mismatch = AggregateRecordingError::ExtensionPhaseDoesNotMatchHook {
        hook: HookSurface::PreToolUse,
        phase: ExtensionInvocationPhase::Completed,
    };

    assert_eq!(
        unsupported.to_string(),
        "hook agent codex cannot contribute extension_invocation_metrics telemetry"
    );
    assert_eq!(
        mismatch.to_string(),
        "extension-invocation phase completed cannot come from hook surface pre_tool_use"
    );
}
