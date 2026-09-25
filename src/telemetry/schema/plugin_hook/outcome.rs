//! Plugin-hook outcomes, attempt lifecycle, and checked counters.

use std::{fmt, time::Duration};

use serde::{Deserialize, Serialize};

/// Final outcome assigned to one completed plugin-hook attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum PluginHookOutcome {
    Ok,
    Blocked,
    Error,
}

/// Whether one plugin-hook attempt started its child process.
///
/// A child that never starts always contributes an `error` outcome. An
/// executed child carries its final outcome and both durations, so impossible
/// combinations such as an unexecuted `ok` attempt cannot reach the row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum PluginHookAttempt {
    NotExecuted {
        prepare_duration: Duration,
    },
    Executed {
        signals: PluginHookOutcomeSignals,
        prepare_duration: Duration,
        execute_duration: Duration,
    },
}

impl PluginHookAttempt {
    #[must_use]
    pub(super) const fn outcome(self) -> PluginHookOutcome {
        match self {
            Self::NotExecuted { .. } => PluginHookOutcome::Error,
            Self::Executed { signals, .. } => PluginHookOutcome::from_signals(signals),
        }
    }

    #[must_use]
    pub(super) const fn prepare_duration(self) -> Duration {
        match self {
            Self::NotExecuted { prepare_duration }
            | Self::Executed {
                prepare_duration, ..
            } => prepare_duration,
        }
    }

    #[must_use]
    pub(super) const fn execute_duration(self) -> Option<Duration> {
        match self {
            Self::NotExecuted { .. } => None,
            Self::Executed {
                execute_duration, ..
            } => Some(execute_duration),
        }
    }
}

impl PluginHookOutcome {
    /// Select the final outcome using the version 1 precedence rule.
    #[must_use]
    pub(in crate::telemetry) const fn from_signals(signals: PluginHookOutcomeSignals) -> Self {
        if signals.error {
            Self::Error
        } else if signals.blocked {
            Self::Blocked
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
            Self::Error => "error",
        }
    }
}

/// Signals used to select one final plugin-hook outcome.
///
/// Named fields prevent the precedence inputs from being transposed at call
/// sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) struct PluginHookOutcomeSignals {
    pub(in crate::telemetry) error: bool,
    pub(in crate::telemetry) blocked: bool,
}

/// Mutually exclusive outcomes of completed plugin-hook attempts.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PluginHookOutcomeCounters {
    pub(super) ok: u64,
    pub(super) blocked: u64,
    pub(super) error: u64,
}

impl PluginHookOutcomeCounters {
    /// Increment exactly one outcome counter.
    ///
    /// # Errors
    ///
    /// Returns [`PluginHookOutcomeCounterOverflow`] when the selected counter
    /// cannot be incremented. The counters are unchanged on failure.
    #[must_use = "counter overflow must drop the containing telemetry update"]
    pub(super) fn checked_record(
        &mut self,
        outcome: PluginHookOutcome,
    ) -> Result<(), PluginHookOutcomeCounterOverflow> {
        let counter = match outcome {
            PluginHookOutcome::Ok => &mut self.ok,
            PluginHookOutcome::Blocked => &mut self.blocked,
            PluginHookOutcome::Error => &mut self.error,
        };
        let next = counter
            .checked_add(1)
            .ok_or(PluginHookOutcomeCounterOverflow { outcome })?;

        *counter = next;
        Ok(())
    }

    /// Return the sum of every outcome counter, or `None` on overflow.
    #[must_use]
    pub(super) fn checked_total(&self) -> Option<u64> {
        [self.ok, self.blocked, self.error]
            .into_iter()
            .try_fold(0_u64, u64::checked_add)
    }
}

/// A selected plugin-hook outcome counter that cannot be incremented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PluginHookOutcomeCounterOverflow {
    pub(super) outcome: PluginHookOutcome,
}

impl fmt::Display for PluginHookOutcomeCounterOverflow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} plugin-hook outcome counter overflows u64",
            self.outcome.counter_name()
        )
    }
}

impl std::error::Error for PluginHookOutcomeCounterOverflow {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plugin_hook_outcomes_follow_error_then_blocked_precedence() {
        let cases = [
            (
                PluginHookOutcomeSignals {
                    error: false,
                    blocked: false,
                },
                PluginHookOutcome::Ok,
            ),
            (
                PluginHookOutcomeSignals {
                    error: false,
                    blocked: true,
                },
                PluginHookOutcome::Blocked,
            ),
            (
                PluginHookOutcomeSignals {
                    error: true,
                    blocked: false,
                },
                PluginHookOutcome::Error,
            ),
            (
                PluginHookOutcomeSignals {
                    error: true,
                    blocked: true,
                },
                PluginHookOutcome::Error,
            ),
        ];

        for (signals, expected) in cases {
            let outcome = PluginHookOutcome::from_signals(signals);

            assert_eq!(outcome, expected);
        }
    }

    #[test]
    fn executed_attempt_applies_outcome_signal_precedence() {
        let attempt = PluginHookAttempt::Executed {
            signals: PluginHookOutcomeSignals {
                error: true,
                blocked: true,
            },
            prepare_duration: Duration::from_millis(1),
            execute_duration: Duration::from_millis(1),
        };

        assert_eq!(attempt.outcome(), PluginHookOutcome::Error);
    }

    #[test]
    fn plugin_hook_outcome_counters_round_trip_in_contract_order() {
        let counters = PluginHookOutcomeCounters {
            ok: 1,
            blocked: 2,
            error: 3,
        };
        let expected = r#"{"ok":1,"blocked":2,"error":3}"#;

        let json = serde_json::to_string(&counters).unwrap();
        let decoded = serde_json::from_str::<PluginHookOutcomeCounters>(&json).unwrap();

        assert_eq!(json, expected);
        assert_eq!(decoded, counters);
    }

    #[test]
    fn recording_each_plugin_hook_outcome_increments_only_its_counter() {
        let cases = [
            (PluginHookOutcome::Ok, r#"{"ok":1,"blocked":0,"error":0}"#),
            (
                PluginHookOutcome::Blocked,
                r#"{"ok":0,"blocked":1,"error":0}"#,
            ),
            (
                PluginHookOutcome::Error,
                r#"{"ok":0,"blocked":0,"error":1}"#,
            ),
        ];

        for (outcome, expected) in cases {
            let mut counters = PluginHookOutcomeCounters::default();

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
    fn plugin_hook_outcome_counters_reject_missing_unknown_and_invalid_fields() {
        let cases = [
            r#"{"ok":0,"blocked":0}"#,
            r#"{"ok":0,"blocked":0,"error":0,"future":0}"#,
            r#"{"ok":"none","blocked":0,"error":0}"#,
            r#"{"ok":-1,"blocked":0,"error":0}"#,
        ];

        for json in cases {
            assert!(
                serde_json::from_str::<PluginHookOutcomeCounters>(json).is_err(),
                "accepted invalid plugin-hook outcome counters {json}"
            );
        }
    }

    #[test]
    fn recording_a_plugin_hook_outcome_rejects_overflow_without_mutation() {
        let outcomes = [
            PluginHookOutcome::Ok,
            PluginHookOutcome::Blocked,
            PluginHookOutcome::Error,
        ];

        for outcome in outcomes {
            let mut counters = PluginHookOutcomeCounters {
                ok: u64::MAX,
                blocked: u64::MAX,
                error: u64::MAX,
            };
            let before = counters;

            let result = counters.checked_record(outcome);

            assert_eq!(result, Err(PluginHookOutcomeCounterOverflow { outcome }));
            assert_eq!(counters, before);
        }
    }

    #[test]
    fn plugin_hook_outcome_counter_total_is_checked_for_overflow() {
        let representable = PluginHookOutcomeCounters {
            ok: 1,
            blocked: 2,
            error: 3,
        };
        let overflowing = PluginHookOutcomeCounters {
            ok: u64::MAX,
            blocked: 1,
            error: 0,
        };

        assert_eq!(representable.checked_total(), Some(6));
        assert_eq!(overflowing.checked_total(), None);
    }
}
