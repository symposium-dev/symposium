//! How long grading's commands may take: a cap per command, and one budget for all of
//! them together, so a task with many hidden checks still finishes in bounded time.
//!
//! Only commands are counted. Copying, listing, and comparing files is not: it is bounded
//! by entry-count and size quotas once grading accepts untrusted submissions, through the
//! `srt` backend.

use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub(crate) struct GradeLimits {
    /// For all of grading's commands together.
    pub(crate) commands: Duration,
    pub(crate) metadata: Duration,
    pub(crate) build: Duration,
}

/// The command time left for one grading. Only time spent running commands is charged,
/// so preparing files between commands never shortens the next one's allowance.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Budget {
    remaining: Duration,
}

impl Default for GradeLimits {
    fn default() -> Self {
        Self {
            commands: Duration::from_mins(5),
            metadata: Duration::from_mins(1),
            build: Duration::from_mins(5),
        }
    }
}

impl Budget {
    pub(crate) fn new(commands: Duration) -> Self {
        Self {
            remaining: commands,
        }
    }

    /// A command's allowance: its own cap, or what is left of the budget if that is less.
    pub(crate) fn allowance(&self, cap: Duration) -> Duration {
        cap.min(self.remaining)
    }

    /// Records time a command spent running.
    pub(crate) fn charge(&mut self, spent: Duration) {
        self.remaining = self.remaining.saturating_sub(spent);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_gets_the_smaller_of_its_cap_and_the_budget_left() {
        let mut budget = Budget::new(Duration::from_secs(10));

        assert_eq!(
            budget.allowance(Duration::from_secs(1)),
            Duration::from_secs(1)
        );
        budget.charge(Duration::from_secs(4));
        assert_eq!(
            budget.allowance(Duration::from_secs(60)),
            Duration::from_secs(6)
        );
    }

    #[test]
    fn a_spent_budget_leaves_no_time() {
        let mut budget = Budget::new(Duration::from_secs(1));

        budget.charge(Duration::from_secs(5));

        assert_eq!(budget.allowance(Duration::from_secs(60)), Duration::ZERO);
    }
}
