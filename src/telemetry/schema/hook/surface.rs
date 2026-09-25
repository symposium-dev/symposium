//! Hook-surface vocabulary and SDK event mapping.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::hook_schema::HookEvent;

/// Hook surface included in version 1 aggregate telemetry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::telemetry) enum HookSurface {
    PreToolUse,
    PostToolUse,
    UserPromptSubmit,
    SessionStart,
    Stop,
}

impl HookSurface {
    /// Return the frozen version 1 wire label.
    #[must_use]
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::PreToolUse => "pre_tool_use",
            Self::PostToolUse => "post_tool_use",
            Self::UserPromptSubmit => "user_prompt_submit",
            Self::SessionStart => "session_start",
            Self::Stop => "stop",
        }
    }
}

/// A hook event that version 1 telemetry does not aggregate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) struct UnsupportedHookEvent;

impl fmt::Display for UnsupportedHookEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("hook event is not supported by telemetry consent version 1")
    }
}

impl std::error::Error for UnsupportedHookEvent {}

impl TryFrom<HookEvent> for HookSurface {
    type Error = UnsupportedHookEvent;

    fn try_from(event: HookEvent) -> Result<Self, Self::Error> {
        match event {
            HookEvent::PreToolUse => Ok(Self::PreToolUse),
            HookEvent::PostToolUse => Ok(Self::PostToolUse),
            HookEvent::UserPromptSubmit => Ok(Self::UserPromptSubmit),
            HookEvent::SessionStart => Ok(Self::SessionStart),
            HookEvent::Stop => Ok(Self::Stop),
            // HookEvent is non-exhaustive. New SDK events need an explicit
            // consent-contract decision before telemetry records them.
            _ => Err(UnsupportedHookEvent),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::schema::assert_contract_names_with_labels;

    #[test]
    fn hook_surfaces_round_trip_with_contract_names() {
        let cases = [
            (HookSurface::PreToolUse, "pre_tool_use"),
            (HookSurface::PostToolUse, "post_tool_use"),
            (HookSurface::UserPromptSubmit, "user_prompt_submit"),
            (HookSurface::SessionStart, "session_start"),
            (HookSurface::Stop, "stop"),
        ];

        assert_contract_names_with_labels(&cases, HookSurface::as_str);
    }

    #[test]
    fn sdk_hook_events_map_to_contract_surfaces() {
        let cases = [
            (HookEvent::PreToolUse, HookSurface::PreToolUse),
            (HookEvent::PostToolUse, HookSurface::PostToolUse),
            (HookEvent::UserPromptSubmit, HookSurface::UserPromptSubmit),
            (HookEvent::SessionStart, HookSurface::SessionStart),
            (HookEvent::Stop, HookSurface::Stop),
        ];

        for (event, expected) in cases {
            assert_eq!(HookSurface::try_from(event), Ok(expected));
        }
    }

    #[test]
    fn hook_surface_rejects_unknown_contract_names() {
        let result = serde_json::from_str::<HookSurface>(r#""before_tool_use""#);

        assert!(result.is_err());
    }
}
