//! Shared monotonic transition for state scoped to one open UTC day.

use std::{cmp::Ordering, fmt};

use crate::telemetry::schema::UtcDay;

/// Change observed while selecting the open UTC day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OpenDayUpdate {
    Current,
    Advanced,
}

/// Monotonic UTC day shared by daily private-state components.
///
/// This type centralizes transition semantics; it is not another persisted
/// clock. Persistence must initialize or validate each consumer from the
/// latest-opened-day high-water mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct OpenDay(UtcDay);

impl OpenDay {
    #[must_use]
    pub(super) const fn new(day: UtcDay) -> Self {
        Self(day)
    }

    /// Select `observed`, advancing this value only when it is later.
    ///
    /// # Errors
    ///
    /// Returns [`DayBeforeCurrent`] when `observed` belongs to a closed day.
    pub(super) fn select(&mut self, observed: UtcDay) -> Result<OpenDayUpdate, DayBeforeCurrent> {
        match observed.cmp(&self.0) {
            Ordering::Less => Err(DayBeforeCurrent {
                current: self.0,
                observed,
            }),
            Ordering::Equal => Ok(OpenDayUpdate::Current),
            Ordering::Greater => {
                self.0 = observed;
                Ok(OpenDayUpdate::Advanced)
            }
        }
    }
}

/// An observation dated before the current open UTC day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) struct DayBeforeCurrent {
    pub(super) current: UtcDay,
    pub(super) observed: UtcDay,
}

impl fmt::Display for DayBeforeCurrent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "observed day {} precedes open day {}",
            self.observed, self.current
        )
    }
}

impl std::error::Error for DayBeforeCurrent {}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    fn day(day: u32) -> UtcDay {
        UtcDay::from_date(NaiveDate::from_ymd_opt(2026, 8, day).unwrap())
    }

    #[test]
    fn current_day_leaves_the_open_day_unchanged() {
        let mut open_day = OpenDay::new(day(3));

        let update = open_day.select(day(3));

        assert_eq!(update, Ok(OpenDayUpdate::Current));
        assert_eq!(open_day, OpenDay::new(day(3)));
    }

    #[test]
    fn later_day_advances_the_open_day() {
        let mut open_day = OpenDay::new(day(3));

        let update = open_day.select(day(4));

        assert_eq!(update, Ok(OpenDayUpdate::Advanced));
        assert_eq!(open_day, OpenDay::new(day(4)));
    }

    #[test]
    fn earlier_day_is_rejected_without_changing_the_open_day() {
        let mut open_day = OpenDay::new(day(4));
        let before = open_day;

        let update = open_day.select(day(3));

        assert_eq!(
            update,
            Err(DayBeforeCurrent {
                current: day(4),
                observed: day(3),
            })
        );
        assert_eq!(open_day, before);
    }
}
