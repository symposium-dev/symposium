//! Daily admission budget shared by named aggregate families.

use crate::telemetry::schema::UtcDay;

use super::open_day::{DayBeforeCurrent, OpenDay, OpenDayUpdate};

/// Maximum public rows one aggregate family may admit in a UTC day.
pub(in crate::telemetry) const MAX_PUBLIC_ROWS_PER_DAY: u64 = 128;

/// Whether a new public aggregate keeps its identity or joins overflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PublicRowAdmission {
    Public,
    Overflow,
}

/// Independent daily allowance for one family of public aggregate rows.
///
/// Callers check for an existing aggregate before consuming this budget.
/// Identifier reset deliberately does not mutate it. Day rollover and clear
/// are the only operations that restore the full allowance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DailyPublicRowBudget {
    day: OpenDay,
    admitted: u64,
}

impl DailyPublicRowBudget {
    #[must_use]
    pub(super) const fn new(day: UtcDay) -> Self {
        Self {
            day: OpenDay::new(day),
            admitted: 0,
        }
    }

    /// Select a day, restoring the allowance after forward UTC-day rollover.
    ///
    /// # Errors
    ///
    /// Returns [`DayBeforeCurrent`] if the observed day precedes the
    /// current open UTC day.
    pub(super) fn select_day(&mut self, day: UtcDay) -> Result<OpenDayUpdate, DayBeforeCurrent> {
        let update = self.day.select(day)?;
        if update == OpenDayUpdate::Advanced {
            self.admitted = 0;
        }

        Ok(update)
    }

    /// Consume one allowance slot for a new public aggregate.
    #[must_use]
    pub(super) fn admit_new(&mut self) -> PublicRowAdmission {
        if self.admitted >= MAX_PUBLIC_ROWS_PER_DAY {
            return PublicRowAdmission::Overflow;
        }

        self.admitted = self
            .admitted
            .checked_add(1)
            .expect("BUG: the public-row allowance is bounded before incrementing");
        PublicRowAdmission::Public
    }

    /// Return whether a new public aggregate must join overflow.
    #[must_use]
    pub(super) const fn is_exhausted(self) -> bool {
        self.admitted >= MAX_PUBLIC_ROWS_PER_DAY
    }

    /// Restore the current day's allowance after telemetry clear.
    pub(super) fn clear(&mut self) {
        self.admitted = 0;
    }

    #[cfg(test)]
    #[must_use]
    pub(super) const fn admitted(self) -> u64 {
        self.admitted
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    fn day(day: u32) -> UtcDay {
        UtcDay::from_date(NaiveDate::from_ymd_opt(2026, 8, day).unwrap())
    }

    #[test]
    fn the_128th_public_row_is_admitted_and_the_129th_overflows() {
        let mut budget = DailyPublicRowBudget::new(day(3));

        for admitted in 1..=MAX_PUBLIC_ROWS_PER_DAY {
            assert_eq!(budget.admit_new(), PublicRowAdmission::Public);
            assert_eq!(budget.admitted(), admitted);
        }
        assert_eq!(budget.admit_new(), PublicRowAdmission::Overflow);

        assert_eq!(budget.admitted(), MAX_PUBLIC_ROWS_PER_DAY);
    }

    #[test]
    fn a_forward_day_restores_the_allowance_but_the_current_day_does_not() {
        let mut budget = DailyPublicRowBudget::new(day(3));
        assert_eq!(budget.admit_new(), PublicRowAdmission::Public);

        assert_eq!(budget.select_day(day(3)), Ok(OpenDayUpdate::Current));
        assert_eq!(budget.admitted(), 1);
        assert_eq!(budget.select_day(day(4)), Ok(OpenDayUpdate::Advanced));

        assert_eq!(budget.admitted(), 0);
    }

    #[test]
    fn an_older_day_is_rejected_without_changing_the_budget() {
        let mut budget = DailyPublicRowBudget::new(day(4));
        assert_eq!(budget.admit_new(), PublicRowAdmission::Public);
        let before = budget;

        let result = budget.select_day(day(3));

        assert_eq!(
            result,
            Err(DayBeforeCurrent {
                current: day(4),
                observed: day(3),
            })
        );
        assert_eq!(budget, before);
    }

    #[test]
    fn clear_restores_the_current_days_allowance() {
        let mut budget = DailyPublicRowBudget::new(day(3));
        assert_eq!(budget.admit_new(), PublicRowAdmission::Public);

        budget.clear();

        assert_eq!(budget.admitted(), 0);
        assert_eq!(budget.select_day(day(3)), Ok(OpenDayUpdate::Current));
    }
}
