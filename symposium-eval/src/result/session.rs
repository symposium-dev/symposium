//! What a result records of the client's session.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::Serialize;

use super::{ResultError, sanitized};
use crate::client::{Ended, Execution, Load, Usage};
use crate::task::Task;

#[derive(Debug, Serialize)]
pub(super) struct SessionRecord {
    ended: EndedRecord,
    elapsed_secs: f64,
    usage: UsageRecord,
    /// What the transcript showed of each skill the task names.
    observed_loads: BTreeMap<String, LoadRecord>,
}

#[derive(Debug, Serialize)]
struct EndedRecord {
    status: EndedStatus,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum EndedStatus {
    Completed,
    TimedOut,
    Failed,
    NotLaunched,
    Unsupervised,
    Unreadable,
    Mismatched,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum UsageRecord {
    Reported {
        input_tokens: u64,
        output_tokens: u64,
        cache_read_tokens: u64,
        cache_write_tokens: u64,
    },
    /// The client never reports usage.
    Unsupported,
    /// The client reports usage, but the transcript does not show this session's.
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum LoadRecord {
    Loaded,
    Attempted,
    NotObserved,
    Unknown,
}

impl SessionRecord {
    /// The session of `execution`, which looked for each skill `task` names in that
    /// order, as `installed_skills` gives them.
    ///
    /// # Errors
    ///
    /// When it shows a different number of loads than skills, so none can be matched
    /// to its skill.
    pub(super) fn new(
        task: &Task,
        execution: &Execution,
        elapsed: Duration,
    ) -> Result<Self, ResultError> {
        Ok(Self {
            ended: EndedRecord::from(&execution.ended),
            elapsed_secs: elapsed.as_secs_f64(),
            usage: UsageRecord::from(execution.summary.usage),
            observed_loads: observed_loads(task, &execution.loads)?,
        })
    }
}

/// Each skill `task` names with its load, from `loads` in the same order.
fn observed_loads(
    task: &Task,
    loads: &[Load],
) -> Result<BTreeMap<String, LoadRecord>, ResultError> {
    let named: Vec<_> = task.expected_skills().names().collect();
    if named.len() != loads.len() {
        return Err(ResultError::Loads {
            named: named.len(),
            observed: loads.len(),
        });
    }
    Ok(named
        .into_iter()
        .zip(loads)
        .map(|(skill, load)| (skill.to_string(), LoadRecord::from(*load)))
        .collect())
}

impl From<&Ended> for EndedRecord {
    fn from(ended: &Ended) -> Self {
        let failed = |status, error: &dyn std::error::Error| Self {
            status,
            error: Some(sanitized(error)),
        };
        match ended {
            Ended::Completed => Self {
                status: EndedStatus::Completed,
                error: None,
            },
            Ended::TimedOut { after_deadline } => Self {
                status: EndedStatus::TimedOut,
                error: after_deadline.as_ref().map(|error| sanitized(error)),
            },
            Ended::Failed(error) => failed(EndedStatus::Failed, error),
            Ended::NotLaunched(error) => failed(EndedStatus::NotLaunched, error),
            Ended::Unsupervised(error) => failed(EndedStatus::Unsupervised, error),
            Ended::Unreadable(error) => failed(EndedStatus::Unreadable, error),
            Ended::Mismatched(error) => failed(EndedStatus::Mismatched, error),
        }
    }
}

impl From<Usage> for UsageRecord {
    fn from(usage: Usage) -> Self {
        match usage {
            Usage::Reported(tokens) => Self::Reported {
                input_tokens: tokens.input,
                output_tokens: tokens.output,
                cache_read_tokens: tokens.cache_read,
                cache_write_tokens: tokens.cache_write,
            },
            Usage::Unsupported => Self::Unsupported,
            Usage::Unknown => Self::Unknown,
        }
    }
}

impl From<Load> for LoadRecord {
    fn from(load: Load) -> Self {
        match load {
            Load::Loaded => Self::Loaded,
            Load::Attempted => Self::Attempted,
            Load::NotObserved => Self::NotObserved,
            Load::Unknown => Self::Unknown,
        }
    }
}
