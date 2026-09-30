//! Hook aggregate identity dimension and lookup key.

use super::surface::HookSurface;
use crate::telemetry::{
    identity::{DimensionWriter, HookDomain, HookSubject, IdentityDimension},
    schema::{UtcDay, agent::HookAgent},
    state::BoundRecordingObservation,
};

/// Typed inputs to one `hook_subject` derivation.
struct HookDimension {
    agent: HookAgent,
    hook: HookSurface,
}

impl HookDimension {
    #[must_use]
    const fn new(agent: HookAgent, hook: HookSurface) -> Self {
        Self { agent, hook }
    }
}

impl IdentityDimension for HookDimension {
    type Domain = HookDomain;

    /// Write agent and hook surface in version 1 contract order.
    fn write(&self, writer: &mut DimensionWriter<'_>) {
        writer.field(self.agent.as_str().as_bytes());
        writer.field(self.hook.as_str().as_bytes());
    }
}

/// Stable lookup key shared by one hook aggregate row and its private state.
///
/// The subject binds the agent, hook surface, identity key, and identifier
/// window. The day keeps daily aggregates separate within one window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::telemetry) struct HookMetricsKey {
    pub(super) day: UtcDay,
    pub(super) hook_subject: HookSubject,
}

impl HookMetricsKey {
    /// Select the aggregate for one bound recording, agent, and hook surface.
    #[must_use]
    pub(in crate::telemetry) fn new(
        recording: &BoundRecordingObservation<'_>,
        agent: HookAgent,
        hook: HookSurface,
    ) -> Self {
        let hook_subject = recording
            .identifier_window_scope()
            .derive(&HookDimension::new(agent, hook));

        Self {
            day: recording.day(),
            hook_subject,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::{
        identity::{HookSubject, encode_dimension_for_test},
        schema::{IDENTIFIER_WINDOW_TEST_STATE, recording_observation},
        state::TelemetryStateV1,
    };

    fn hook_subject(agent: HookAgent, hook: HookSurface) -> HookSubject {
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let observation = recording_observation(&mut state);
        let dimension = HookDimension::new(agent, hook);

        observation.identifier_window_scope().derive(&dimension)
    }

    #[test]
    fn hook_subject_dimension_uses_agent_then_hook_surface() {
        let dimension = HookDimension::new(HookAgent::Claude, HookSurface::PreToolUse);
        let expected = [
            [0, 0, 0, 0, 0, 0, 0, 6].as_slice(),
            b"claude".as_slice(),
            [0, 0, 0, 0, 0, 0, 0, 12].as_slice(),
            b"pre_tool_use".as_slice(),
        ]
        .concat();

        let encoded = encode_dimension_for_test(&dimension);

        assert_eq!(encoded, expected);
    }

    #[test]
    fn hook_subject_derivation_matches_independent_vector() {
        // Cross-checked with .NET's HMACSHA256 over the contract header,
        // identifier window, agent, and hook surface. The complete digest is
        // 99106b457209912cf0023c562a72dafb225707f0718f26e3a379c42fedf1cf55.
        let expected = "hok_99106b457209912cf0023c562a72dafb".parse().unwrap();

        let subject = hook_subject(HookAgent::Claude, HookSurface::PreToolUse);

        assert_eq!(subject, expected);
    }

    #[test]
    fn hook_subject_changes_with_agent_or_hook_surface() {
        let baseline = hook_subject(HookAgent::Claude, HookSurface::PreToolUse);
        let other_agent = hook_subject(HookAgent::Codex, HookSurface::PreToolUse);
        let other_surface = hook_subject(HookAgent::Claude, HookSurface::PostToolUse);

        assert_ne!(baseline, other_agent);
        assert_ne!(baseline, other_surface);
    }
}
