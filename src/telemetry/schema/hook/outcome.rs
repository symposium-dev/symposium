//! Hook outcomes and checked counters.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Final outcome assigned to one completed top-level hook observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum HookOutcome {
    Ok,
    Blocked,
    PluginError,
    InternalError,
}

impl HookOutcome {
    /// Select the final outcome using the version 1 precedence rule.
    #[must_use]
    pub(in crate::telemetry) const fn from_signals(signals: HookOutcomeSignals) -> Self {
        if signals.internal_error {
            Self::InternalError
        } else if signals.blocked {
            Self::Blocked
        } else if signals.plugin_error {
            Self::PluginError
        } else {
            Self::Ok
        }
    }

    /// Return the counter name associated with this outcome.
    #[must_use]
    pub(super) const fn counter_name(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Blocked => "blocked",
            Self::PluginError => "plugin_error",
            Self::InternalError => "internal_error",
        }
    }
}

/// Signals used to select one final top-level hook outcome.
///
/// Named fields prevent the three precedence inputs from being transposed at
/// call sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) struct HookOutcomeSignals {
    pub(in crate::telemetry) internal_error: bool,
    pub(in crate::telemetry) blocked: bool,
    pub(in crate::telemetry) plugin_error: bool,
}

/// Mutually exclusive outcomes of completed top-level hook observations.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HookOutcomeCounters {
    pub(super) ok: u64,
    blocked: u64,
    plugin_error: u64,
    internal_error: u64,
}

impl HookOutcomeCounters {
    /// Increment exactly one outcome counter.
    ///
    /// # Errors
    ///
    /// Returns [`HookOutcomeCounterOverflow`] when the selected counter cannot
    /// be incremented. The counters are unchanged on failure.
    #[must_use = "counter overflow must drop the containing telemetry update"]
    pub(super) fn checked_record(
        &mut self,
        outcome: HookOutcome,
    ) -> Result<(), HookOutcomeCounterOverflow> {
        let counter = match outcome {
            HookOutcome::Ok => &mut self.ok,
            HookOutcome::Blocked => &mut self.blocked,
            HookOutcome::PluginError => &mut self.plugin_error,
            HookOutcome::InternalError => &mut self.internal_error,
        };
        let next = counter
            .checked_add(1)
            .ok_or(HookOutcomeCounterOverflow { outcome })?;

        *counter = next;
        Ok(())
    }

    /// Return the sum of every outcome counter, or `None` on overflow.
    #[must_use]
    pub(super) fn checked_total(&self) -> Option<u64> {
        [
            self.ok,
            self.blocked,
            self.plugin_error,
            self.internal_error,
        ]
        .into_iter()
        .try_fold(0_u64, u64::checked_add)
    }
}

/// A selected hook outcome counter that cannot be incremented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct HookOutcomeCounterOverflow {
    pub(super) outcome: HookOutcome,
}

impl fmt::Display for HookOutcomeCounterOverflow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} hook outcome counter overflows u64",
            self.outcome.counter_name()
        )
    }
}

impl std::error::Error for HookOutcomeCounterOverflow {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_outcome_counters_round_trip_in_contract_order() {
        let counters = HookOutcomeCounters {
            ok: 1,
            blocked: 2,
            plugin_error: 3,
            internal_error: 4,
        };
        let expected = r#"{"ok":1,"blocked":2,"plugin_error":3,"internal_error":4}"#;

        let json = serde_json::to_string(&counters).unwrap();
        let decoded = serde_json::from_str::<HookOutcomeCounters>(&json).unwrap();

        assert_eq!(json, expected);
        assert_eq!(decoded, counters);
    }

    #[test]
    fn recording_each_hook_outcome_increments_only_its_counter() {
        let cases = [
            (
                HookOutcome::Ok,
                r#"{"ok":1,"blocked":0,"plugin_error":0,"internal_error":0}"#,
            ),
            (
                HookOutcome::Blocked,
                r#"{"ok":0,"blocked":1,"plugin_error":0,"internal_error":0}"#,
            ),
            (
                HookOutcome::PluginError,
                r#"{"ok":0,"blocked":0,"plugin_error":1,"internal_error":0}"#,
            ),
            (
                HookOutcome::InternalError,
                r#"{"ok":0,"blocked":0,"plugin_error":0,"internal_error":1}"#,
            ),
        ];

        for (outcome, expected) in cases {
            let mut counters = HookOutcomeCounters::default();

            counters.checked_record(outcome).unwrap();

            let json = serde_json::to_string(&counters).unwrap();
            let value = serde_json::to_value(counters).unwrap();
            let active_counter = value
                .as_object()
                .unwrap()
                .iter()
                .find_map(|(name, count)| (count.as_u64() == Some(1)).then_some(name.as_str()))
                .unwrap();

            assert_eq!(json, expected);
            assert_eq!(active_counter, outcome.counter_name());
        }
    }

    #[test]
    fn hook_outcome_uses_the_contract_precedence() {
        let cases = [
            (
                HookOutcomeSignals {
                    internal_error: false,
                    blocked: false,
                    plugin_error: false,
                },
                HookOutcome::Ok,
            ),
            (
                HookOutcomeSignals {
                    internal_error: false,
                    blocked: false,
                    plugin_error: true,
                },
                HookOutcome::PluginError,
            ),
            (
                HookOutcomeSignals {
                    internal_error: false,
                    blocked: true,
                    plugin_error: true,
                },
                HookOutcome::Blocked,
            ),
            (
                HookOutcomeSignals {
                    internal_error: true,
                    blocked: true,
                    plugin_error: true,
                },
                HookOutcome::InternalError,
            ),
        ];

        for (signals, expected) in cases {
            assert_eq!(HookOutcome::from_signals(signals), expected);
        }
    }

    #[test]
    fn hook_outcome_counters_reject_missing_unknown_and_invalid_fields() {
        let cases = [
            r#"{"ok":0,"blocked":0,"plugin_error":0}"#,
            r#"{"ok":0,"blocked":0,"plugin_error":0,"internal_error":0,"future":0}"#,
            r#"{"ok":"none","blocked":0,"plugin_error":0,"internal_error":0}"#,
            r#"{"ok":-1,"blocked":0,"plugin_error":0,"internal_error":0}"#,
        ];

        for json in cases {
            assert!(
                serde_json::from_str::<HookOutcomeCounters>(json).is_err(),
                "accepted invalid hook outcome counters {json}"
            );
        }
    }

    #[test]
    fn recording_an_outcome_rejects_overflow_without_mutation() {
        let outcomes = [
            HookOutcome::Ok,
            HookOutcome::Blocked,
            HookOutcome::PluginError,
            HookOutcome::InternalError,
        ];

        for outcome in outcomes {
            let mut counters = HookOutcomeCounters {
                ok: u64::MAX,
                blocked: u64::MAX,
                plugin_error: u64::MAX,
                internal_error: u64::MAX,
            };
            let before = counters;

            let result = counters.checked_record(outcome);

            assert_eq!(result, Err(HookOutcomeCounterOverflow { outcome }));
            assert_eq!(counters, before);
        }
    }

    #[test]
    fn hook_outcome_counter_total_is_checked_for_overflow() {
        let representable = HookOutcomeCounters {
            ok: 1,
            blocked: 2,
            plugin_error: 3,
            internal_error: 4,
        };
        let overflowing = HookOutcomeCounters {
            ok: u64::MAX,
            blocked: 1,
            plugin_error: 0,
            internal_error: 0,
        };

        assert_eq!(representable.checked_total(), Some(10));
        assert_eq!(overflowing.checked_total(), None);
    }
}
