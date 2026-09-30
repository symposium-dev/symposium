//! Shared schema types for cumulative telemetry metrics.

use std::{fmt, time::Duration};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

const LATENCY_BOUNDS_MS: [u64; 8] = [5, 10, 25, 50, 100, 250, 500, 1_000];
const LATENCY_BUCKET_COUNT: usize = LATENCY_BOUNDS_MS.len() + 1;

/// Maximum number of distinct sessions retained by one aggregate session set.
pub(in crate::telemetry) const MAX_IDENTIFIED_SESSIONS: u64 = 256;

/// Convert a duration to the contract's whole-millisecond representation.
///
/// Sub-millisecond precision is truncated. Durations outside the wire type's
/// range saturate instead of wrapping to an unrelated smaller value.
#[must_use]
pub(in crate::telemetry) fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// Fixed millisecond histogram shared by cumulative telemetry rows.
///
/// The bounds are part of the version 1 wire contract rather than runtime
/// state. Keeping only the counters in this type makes changed bounds
/// unrepresentable after deserialization.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) struct LatencyHistogram {
    counts: [u64; LATENCY_BUCKET_COUNT],
}

impl LatencyHistogram {
    /// Increment the bucket containing `duration` after converting it to the
    /// contract's whole-millisecond representation.
    ///
    /// # Errors
    ///
    /// Returns [`LatencyHistogramError::BucketCountOverflow`] when the
    /// selected counter cannot be incremented. The histogram is unchanged on
    /// failure.
    #[must_use = "counter overflow must drop the containing telemetry update"]
    pub(in crate::telemetry) fn checked_record(
        &mut self,
        duration: Duration,
    ) -> Result<(), LatencyHistogramError> {
        let duration_ms = duration_millis(duration);
        let bucket = LATENCY_BOUNDS_MS.partition_point(|bound| duration_ms > *bound);
        let next = self.counts[bucket]
            .checked_add(1)
            .ok_or(LatencyHistogramError::BucketCountOverflow { bucket })?;

        self.counts[bucket] = next;
        Ok(())
    }

    /// Return the sum of every bucket, or `None` if the total exceeds `u64`.
    #[must_use]
    pub(in crate::telemetry) fn checked_total(&self) -> Option<u64> {
        self.counts
            .iter()
            .try_fold(0_u64, |total, count| total.checked_add(*count))
    }
}

/// A cumulative latency counter that cannot be represented in `u64`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum LatencyHistogramError {
    BucketCountOverflow { bucket: usize },
}

impl fmt::Display for LatencyHistogramError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BucketCountOverflow { bucket } => {
                write!(formatter, "latency histogram bucket {bucket} overflows u64")
            }
        }
    }
}

impl std::error::Error for LatencyHistogramError {}

impl Serialize for LatencyHistogram {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RawLatencyHistogram {
            bounds: LATENCY_BOUNDS_MS,
            counts: self.counts,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for LatencyHistogram {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawLatencyHistogram::deserialize(deserializer)?;

        if raw.bounds != LATENCY_BOUNDS_MS {
            return Err(D::Error::custom(format_args!(
                "expected latency histogram bounds {LATENCY_BOUNDS_MS:?}, found {:?}",
                raw.bounds
            )));
        }

        Ok(Self { counts: raw.counts })
    }
}

/// Strict wire representation of a latency histogram.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLatencyHistogram {
    bounds: [u64; LATENCY_BOUNDS_MS.len()],
    counts: [u64; LATENCY_BUCKET_COUNT],
}

/// Session set whose count is being checked against its observations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum SessionSet {
    Identified,
    NonOk,
    Attempted,
    Completed,
}

impl SessionSet {
    const fn session_label(self) -> &'static str {
        match self {
            Self::Identified => "identified session",
            Self::NonOk => "non-ok identified session",
            Self::Attempted => "attempted identified session",
            Self::Completed => "completed identified session",
        }
    }

    const fn observation_label(self) -> &'static str {
        match self {
            Self::Identified => "observation",
            Self::NonOk => "non-ok observation",
            Self::Attempted => "attempted observation",
            Self::Completed => "completed observation",
        }
    }
}

const fn validate_session_set(
    set: SessionSet,
    identified: u64,
    observations: u64,
) -> Result<(), SessionSetError> {
    if observations > 0 && identified == 0 {
        return Err(SessionSetError::NoSessions { set, observations });
    }
    if identified > MAX_IDENTIFIED_SESSIONS {
        return Err(SessionSetError::ExceedsLimit {
            set,
            identified,
            maximum: MAX_IDENTIFIED_SESSIONS,
        });
    }
    if identified > observations {
        return Err(SessionSetError::ExceedsObservations {
            set,
            identified,
            observations,
        });
    }

    Ok(())
}

/// Invalid relationship between one identified-session set and its
/// contributing observations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum SessionSetError {
    NoSessions {
        set: SessionSet,
        observations: u64,
    },
    ExceedsLimit {
        set: SessionSet,
        identified: u64,
        maximum: u64,
    },
    ExceedsObservations {
        set: SessionSet,
        identified: u64,
        observations: u64,
    },
}

impl fmt::Display for SessionSetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSessions { set, observations } => write!(
                formatter,
                "{} count {observations} requires a non-zero {} count",
                set.observation_label(),
                set.session_label()
            ),
            Self::ExceedsLimit {
                set,
                identified,
                maximum,
            } => write!(
                formatter,
                "{} count {identified} exceeds the version 1 limit {maximum}",
                set.session_label()
            ),
            Self::ExceedsObservations {
                set,
                identified,
                observations,
            } => write!(
                formatter,
                "{} count {identified} exceeds {} count {observations}",
                set.session_label(),
                set.observation_label()
            ),
        }
    }
}

impl std::error::Error for SessionSetError {}

/// Inputs to the session-count rules shared by hook aggregate rows.
///
/// A named input keeps the two optional counters and the two observation
/// totals from being transposed at call sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) struct SessionCountInput {
    pub(in crate::telemetry) counts_complete: bool,
    pub(in crate::telemetry) identified_sessions: Option<u64>,
    pub(in crate::telemetry) identified_sessions_non_ok: Option<u64>,
    pub(in crate::telemetry) observations: u64,
    pub(in crate::telemetry) ok_observations: u64,
}

/// Validate the complete-session rules shared by hook aggregate rows.
///
/// # Errors
///
/// Returns [`SessionCountError`] when the presence or value of a session
/// counter is inconsistent with the aggregate's observation counters.
pub(in crate::telemetry) fn validate_session_counts(
    input: SessionCountInput,
) -> Result<(), SessionCountError> {
    match (
        input.counts_complete,
        input.identified_sessions,
        input.identified_sessions_non_ok,
    ) {
        (true, Some(identified), Some(non_ok)) => {
            validate_session_set(SessionSet::Identified, identified, input.observations)?;
            if non_ok > identified {
                return Err(SessionCountError::NonOkSessionsExceedIdentified {
                    identified,
                    non_ok,
                });
            }

            let non_ok_observations = input
                .observations
                .checked_sub(input.ok_observations)
                .ok_or(SessionCountError::OkObservationsExceedObservations {
                    observations: input.observations,
                    ok_observations: input.ok_observations,
                })?;
            validate_session_set(SessionSet::NonOk, non_ok, non_ok_observations)?;

            // The subset check above proves that this subtraction cannot
            // underflow. Every remaining session contributed an `ok` result.
            let all_ok_sessions = identified - non_ok;
            if all_ok_sessions > input.ok_observations {
                return Err(SessionCountError::AllOkSessionsExceedOkObservations {
                    sessions: all_ok_sessions,
                    observations: input.ok_observations,
                });
            }

            Ok(())
        }
        (false, None, None) => Ok(()),
        (true, _, _) => Err(SessionCountError::CompleteCountsMissing),
        (false, _, _) => Err(SessionCountError::IncompleteCountsPresent),
    }
}

/// Invalid relationship between session counters in a hook aggregate row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum SessionCountError {
    CompleteCountsMissing,
    IncompleteCountsPresent,
    SessionSet(SessionSetError),
    NonOkSessionsExceedIdentified {
        identified: u64,
        non_ok: u64,
    },
    OkObservationsExceedObservations {
        observations: u64,
        ok_observations: u64,
    },
    AllOkSessionsExceedOkObservations {
        sessions: u64,
        observations: u64,
    },
}

impl From<SessionSetError> for SessionCountError {
    fn from(error: SessionSetError) -> Self {
        Self::SessionSet(error)
    }
}

impl fmt::Display for SessionCountError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CompleteCountsMissing => formatter
                .write_str("complete session counts require both identified session counters"),
            Self::IncompleteCountsPresent => formatter
                .write_str("incomplete session counts must omit both identified session counters"),
            Self::SessionSet(error) => error.fmt(formatter),
            Self::NonOkSessionsExceedIdentified { identified, non_ok } => write!(
                formatter,
                "non-ok identified sessions {non_ok} exceed {identified} identified sessions"
            ),
            Self::OkObservationsExceedObservations {
                observations,
                ok_observations,
            } => write!(
                formatter,
                "ok observations {ok_observations} exceed {observations} observations"
            ),
            Self::AllOkSessionsExceedOkObservations {
                sessions,
                observations,
            } => write!(
                formatter,
                "all-ok identified sessions {sessions} exceed {observations} ok observations"
            ),
        }
    }
}

impl std::error::Error for SessionCountError {}

/// Inputs to the independent session-count rules for skill invocations.
///
/// Attempted and completed observations are deliberately separate: a lost
/// update may leave either counter or identified-session set ahead of the
/// other. Naming every input keeps those parallel values from being
/// transposed at call sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) struct ExtensionSessionCountInput {
    pub(in crate::telemetry) counts_complete: bool,
    pub(in crate::telemetry) identified_sessions: Option<u64>,
    pub(in crate::telemetry) identified_sessions_completed: Option<u64>,
    pub(in crate::telemetry) attempted: u64,
    pub(in crate::telemetry) completed: u64,
}

/// Validate the independent session sets for extension-invocation aggregates.
///
/// This intentionally imposes no relationship between attempted and
/// completed observations or between their session sets. Each set is bounded
/// only by its own phase counter and the version 1 distinct-session limit.
///
/// # Errors
///
/// Returns [`ExtensionSessionCountError`] when the presence or value of a
/// session counter is inconsistent with its matching phase counter.
pub(in crate::telemetry) fn validate_extension_session_counts(
    input: ExtensionSessionCountInput,
) -> Result<(), ExtensionSessionCountError> {
    match (
        input.counts_complete,
        input.identified_sessions,
        input.identified_sessions_completed,
    ) {
        (true, Some(identified), Some(identified_completed)) => {
            validate_session_set(SessionSet::Attempted, identified, input.attempted)?;
            validate_session_set(SessionSet::Completed, identified_completed, input.completed)?;

            Ok(())
        }
        (false, None, None) => Ok(()),
        (true, _, _) => Err(ExtensionSessionCountError::CompleteCountsMissing),
        (false, _, _) => Err(ExtensionSessionCountError::IncompleteCountsPresent),
    }
}

/// Invalid relationship between session counters in an extension-invocation
/// aggregate row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum ExtensionSessionCountError {
    CompleteCountsMissing,
    IncompleteCountsPresent,
    SessionSet(SessionSetError),
}

impl From<SessionSetError> for ExtensionSessionCountError {
    fn from(error: SessionSetError) -> Self {
        Self::SessionSet(error)
    }
}

impl fmt::Display for ExtensionSessionCountError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CompleteCountsMissing => formatter.write_str(
                "complete extension session counts require both identified session counters",
            ),
            Self::IncompleteCountsPresent => formatter.write_str(
                "incomplete extension session counts must omit both identified session counters",
            ),
            Self::SessionSet(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ExtensionSessionCountError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_histogram_matches_the_recorded_data_contract() {
        let example = super::super::recorded_data_example_block(
            "### Rules shared by hook aggregates",
            "```json",
        )
        .trim();

        let histogram = serde_json::from_str::<LatencyHistogram>(example).unwrap();
        let encoded = serde_json::to_string(&histogram).unwrap();

        assert_eq!(histogram, LatencyHistogram::default());
        assert_eq!(encoded, example);
    }

    #[test]
    fn durations_use_the_contract_bucket_boundaries() {
        let mut histogram = LatencyHistogram::default();
        let durations_ms = [
            0,
            5,
            6,
            10,
            11,
            25,
            26,
            50,
            51,
            100,
            101,
            250,
            251,
            500,
            501,
            1_000,
            1_001,
            u64::MAX,
        ];

        for duration_ms in durations_ms {
            histogram
                .checked_record(Duration::from_millis(duration_ms))
                .unwrap();
        }

        assert_eq!(histogram.counts, [2; LATENCY_BUCKET_COUNT]);
    }

    #[test]
    fn duration_conversion_truncates_and_saturates() {
        let sub_millisecond = Duration::from_nanos(999_999);
        let fractional_millisecond = Duration::from_micros(5_999);
        let outside_u64_milliseconds = Duration::from_secs(u64::MAX / 1_000 + 1);

        assert_eq!(duration_millis(sub_millisecond), 0);
        assert_eq!(duration_millis(fractional_millisecond), 5);
        assert_eq!(duration_millis(outside_u64_milliseconds), u64::MAX);
    }

    #[test]
    fn histogram_rejects_changed_bounds() {
        let json = r#"{"bounds":[4,10,25,50,100,250,500,1000],"counts":[0,0,0,0,0,0,0,0,0]}"#;

        let error = serde_json::from_str::<LatencyHistogram>(json).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("expected latency histogram bounds"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn histogram_rejects_variable_array_lengths() {
        let cases = [
            r#"{"bounds":[5,10,25,50,100,250,500],"counts":[0,0,0,0,0,0,0,0,0]}"#,
            r#"{"bounds":[5,10,25,50,100,250,500,1000,2000],"counts":[0,0,0,0,0,0,0,0,0]}"#,
            r#"{"bounds":[5,10,25,50,100,250,500,1000],"counts":[0,0,0,0,0,0,0,0]}"#,
            r#"{"bounds":[5,10,25,50,100,250,500,1000],"counts":[0,0,0,0,0,0,0,0,0,0]}"#,
        ];

        for json in cases {
            assert!(
                serde_json::from_str::<LatencyHistogram>(json).is_err(),
                "accepted variable histogram shape {json}"
            );
        }
    }

    #[test]
    fn histogram_rejects_missing_and_unknown_fields() {
        let cases = [
            r#"{"counts":[0,0,0,0,0,0,0,0,0]}"#,
            r#"{"bounds":[5,10,25,50,100,250,500,1000]}"#,
            r#"{"bounds":[5,10,25,50,100,250,500,1000],"counts":[0,0,0,0,0,0,0,0,0],"future":true}"#,
        ];

        for json in cases {
            assert!(
                serde_json::from_str::<LatencyHistogram>(json).is_err(),
                "accepted invalid histogram object {json}"
            );
        }
    }

    #[test]
    fn recording_rejects_overflow_without_mutation() {
        let mut histogram = LatencyHistogram {
            counts: [u64::MAX, 1, 2, 3, 4, 5, 6, 7, 8],
        };
        let before = histogram;

        let result = histogram.checked_record(Duration::from_millis(5));

        assert_eq!(
            result,
            Err(LatencyHistogramError::BucketCountOverflow { bucket: 0 })
        );
        assert_eq!(histogram, before);
    }

    #[test]
    fn histogram_total_is_checked_for_overflow() {
        let representable = LatencyHistogram {
            counts: [1, 2, 3, 4, 5, 6, 7, 8, 9],
        };
        let overflowing = LatencyHistogram {
            counts: [u64::MAX, 1, 0, 0, 0, 0, 0, 0, 0],
        };

        assert_eq!(representable.checked_total(), Some(45));
        assert_eq!(overflowing.checked_total(), None);
    }

    #[test]
    fn session_set_errors_name_each_counter_without_plural_grammar() {
        let cases = [
            (
                SessionSet::Identified,
                "observation count 1 requires a non-zero identified session count",
            ),
            (
                SessionSet::NonOk,
                "non-ok observation count 1 requires a non-zero non-ok identified session count",
            ),
            (
                SessionSet::Attempted,
                "attempted observation count 1 requires a non-zero attempted identified session count",
            ),
            (
                SessionSet::Completed,
                "completed observation count 1 requires a non-zero completed identified session count",
            ),
        ];

        for (set, expected) in cases {
            let error = SessionSetError::NoSessions {
                set,
                observations: 1,
            };

            assert_eq!(error.to_string(), expected);
        }
    }

    #[test]
    fn session_counts_reject_more_ok_than_total_observations() {
        let input = SessionCountInput {
            counts_complete: true,
            identified_sessions: Some(1),
            identified_sessions_non_ok: Some(0),
            observations: 1,
            ok_observations: 2,
        };

        let result = validate_session_counts(input);

        assert_eq!(
            result,
            Err(SessionCountError::OkObservationsExceedObservations {
                observations: 1,
                ok_observations: 2,
            })
        );
    }

    #[test]
    fn extension_session_counts_allow_independent_phase_totals() {
        let input = ExtensionSessionCountInput {
            counts_complete: true,
            identified_sessions: Some(1),
            identified_sessions_completed: Some(2),
            attempted: 1,
            completed: 2,
        };

        let result = validate_extension_session_counts(input);

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn extension_session_counts_allow_a_failed_only_row() {
        let input = ExtensionSessionCountInput {
            counts_complete: true,
            identified_sessions: Some(0),
            identified_sessions_completed: Some(0),
            attempted: 0,
            completed: 0,
        };

        let result = validate_extension_session_counts(input);

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn extension_session_counts_accept_the_exact_limits() {
        let input = ExtensionSessionCountInput {
            counts_complete: true,
            identified_sessions: Some(MAX_IDENTIFIED_SESSIONS),
            identified_sessions_completed: Some(MAX_IDENTIFIED_SESSIONS),
            attempted: MAX_IDENTIFIED_SESSIONS,
            completed: MAX_IDENTIFIED_SESSIONS,
        };

        let result = validate_extension_session_counts(input);

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn extension_session_counts_require_matching_presence() {
        let missing_counts = [
            ExtensionSessionCountInput {
                counts_complete: true,
                identified_sessions: None,
                identified_sessions_completed: Some(1),
                attempted: 1,
                completed: 1,
            },
            ExtensionSessionCountInput {
                counts_complete: true,
                identified_sessions: Some(1),
                identified_sessions_completed: None,
                attempted: 1,
                completed: 1,
            },
        ];

        for input in missing_counts {
            assert_eq!(
                validate_extension_session_counts(input),
                Err(ExtensionSessionCountError::CompleteCountsMissing)
            );
        }

        let incomplete_with_counts = ExtensionSessionCountInput {
            counts_complete: false,
            identified_sessions: Some(1),
            identified_sessions_completed: Some(1),
            attempted: 1,
            completed: 1,
        };
        assert_eq!(
            validate_extension_session_counts(incomplete_with_counts),
            Err(ExtensionSessionCountError::IncompleteCountsPresent)
        );
    }

    #[test]
    fn extension_session_counts_require_a_session_for_each_positive_phase() {
        let attempted_without_session = ExtensionSessionCountInput {
            counts_complete: true,
            identified_sessions: Some(0),
            identified_sessions_completed: Some(0),
            attempted: 1,
            completed: 0,
        };
        let completed_without_session = ExtensionSessionCountInput {
            counts_complete: true,
            identified_sessions: Some(0),
            identified_sessions_completed: Some(0),
            attempted: 0,
            completed: 1,
        };

        assert_eq!(
            validate_extension_session_counts(attempted_without_session),
            Err(ExtensionSessionCountError::SessionSet(
                SessionSetError::NoSessions {
                    set: SessionSet::Attempted,
                    observations: 1,
                }
            ))
        );
        assert_eq!(
            validate_extension_session_counts(completed_without_session),
            Err(ExtensionSessionCountError::SessionSet(
                SessionSetError::NoSessions {
                    set: SessionSet::Completed,
                    observations: 1,
                }
            ))
        );
    }

    #[test]
    fn extension_session_counts_reject_phase_set_overflow() {
        let attempted = ExtensionSessionCountInput {
            counts_complete: true,
            identified_sessions: Some(MAX_IDENTIFIED_SESSIONS + 1),
            identified_sessions_completed: Some(0),
            attempted: MAX_IDENTIFIED_SESSIONS + 1,
            completed: 0,
        };
        let completed = ExtensionSessionCountInput {
            counts_complete: true,
            identified_sessions: Some(0),
            identified_sessions_completed: Some(MAX_IDENTIFIED_SESSIONS + 1),
            attempted: 0,
            completed: MAX_IDENTIFIED_SESSIONS + 1,
        };

        assert_eq!(
            validate_extension_session_counts(attempted),
            Err(ExtensionSessionCountError::SessionSet(
                SessionSetError::ExceedsLimit {
                    set: SessionSet::Attempted,
                    identified: MAX_IDENTIFIED_SESSIONS + 1,
                    maximum: MAX_IDENTIFIED_SESSIONS,
                }
            ))
        );
        assert_eq!(
            validate_extension_session_counts(completed),
            Err(ExtensionSessionCountError::SessionSet(
                SessionSetError::ExceedsLimit {
                    set: SessionSet::Completed,
                    identified: MAX_IDENTIFIED_SESSIONS + 1,
                    maximum: MAX_IDENTIFIED_SESSIONS,
                }
            ))
        );
    }

    #[test]
    fn extension_session_counts_cannot_exceed_their_phase_totals() {
        let attempted = ExtensionSessionCountInput {
            counts_complete: true,
            identified_sessions: Some(2),
            identified_sessions_completed: Some(0),
            attempted: 1,
            completed: 0,
        };
        let completed = ExtensionSessionCountInput {
            counts_complete: true,
            identified_sessions: Some(0),
            identified_sessions_completed: Some(2),
            attempted: 0,
            completed: 1,
        };

        assert_eq!(
            validate_extension_session_counts(attempted),
            Err(ExtensionSessionCountError::SessionSet(
                SessionSetError::ExceedsObservations {
                    set: SessionSet::Attempted,
                    identified: 2,
                    observations: 1,
                }
            ))
        );
        assert_eq!(
            validate_extension_session_counts(completed),
            Err(ExtensionSessionCountError::SessionSet(
                SessionSetError::ExceedsObservations {
                    set: SessionSet::Completed,
                    identified: 2,
                    observations: 1,
                }
            ))
        );
    }
}
