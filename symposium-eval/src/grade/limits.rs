//! How long grading's commands may take: a cap per command, and one budget for all of
//! them together, so a task with many hidden checks still finishes in bounded time.
//!
//! Only commands are counted. Copying, listing, and comparing files is not: it is bounded
//! by entry-count and size quotas once grading accepts untrusted submissions, through the
//! `srt` backend.

use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub(crate) struct GradeLimits {
    /// For all of grading's commands together.
    pub(crate) commands: Duration,
    pub(crate) metadata: Duration,
    #[expect(dead_code, reason = "the build step reads this in the next commit")]
    pub(crate) build: Duration,
}

/// The command time left for one grading.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Budget {
    ends: Instant,
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
    pub(crate) fn start(commands: Duration) -> Self {
        Self {
            ends: Instant::now() + commands,
        }
    }

    /// A command's deadline: its own cap, or what is left of the budget if that is less.
    pub(crate) fn deadline(&self, cap: Duration) -> Duration {
        cap.min(self.ends.saturating_duration_since(Instant::now()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_gets_the_smaller_of_its_cap_and_the_time_left() {
        let budget = Budget::start(Duration::from_secs(10));

        assert_eq!(
            budget.deadline(Duration::from_secs(1)),
            Duration::from_secs(1)
        );
        assert!(budget.deadline(Duration::from_secs(60)) <= Duration::from_secs(10));
    }

    #[test]
    fn a_spent_budget_leaves_no_time() {
        let budget = Budget::start(Duration::ZERO);

        assert_eq!(budget.deadline(Duration::from_secs(60)), Duration::ZERO);
    }
}
