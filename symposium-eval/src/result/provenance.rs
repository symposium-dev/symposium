//! What a result records of the trial's plan and of what ran it.

use std::collections::BTreeMap;

use serde::Serialize;

use super::{Deadlines, IsolationMode, SymposiumBuild};
use crate::digest::Digest;
use crate::expect::config::{Change, ConfigChange, Root};
use crate::run::client::Client;
use crate::task::{Task, TaskKind};

#[derive(Debug, Serialize)]
pub(super) struct TaskRecord {
    name: String,
    kind: TaskKindRecord,
    content_hash: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum TaskKindRecord {
    Regression,
    Capability,
}

#[derive(Debug, Serialize)]
pub(super) struct ClientRecord {
    name: &'static str,
    version: Option<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct ModelRecord {
    /// As requested.
    name: String,
    /// None are set yet.
    settings: BTreeMap<String, String>,
    /// What the client says it ran, which resolves an alias.
    reported: Option<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct SymposiumRecord {
    version: String,
    sha256: String,
    revision: Option<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct DeadlinesRecord {
    setup: u64,
    agent: u64,
    grading: u64,
}

#[derive(Debug, Serialize)]
pub(super) struct IsolationRecord {
    mode: IsolationMode,
    srt_version: Option<String>,
    config_changes: Vec<ConfigChangeRecord>,
}

#[derive(Debug, Serialize)]
struct ConfigChangeRecord {
    root: ConfigRootRecord,
    file: &'static str,
    change: ConfigChangeKind,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum ConfigRootRecord {
    Home,
    Project,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum ConfigChangeKind {
    Created,
    Modified,
    Removed,
    Unreadable,
}

impl TaskRecord {
    pub(super) fn new(task: &Task, content_hash: Digest) -> Self {
        Self {
            name: task.name().to_string(),
            kind: match task.kind() {
                TaskKind::Regression => TaskKindRecord::Regression,
                TaskKind::Capability => TaskKindRecord::Capability,
            },
            content_hash: content_hash.to_string(),
        }
    }
}

impl ClientRecord {
    pub(super) fn new(client: Client, version: Option<String>) -> Self {
        Self {
            name: client.symposium_name(),
            version,
        }
    }
}

impl ModelRecord {
    pub(super) fn new(requested: &str, reported: Option<String>) -> Self {
        Self {
            name: requested.to_owned(),
            settings: BTreeMap::new(),
            reported,
        }
    }
}

impl From<SymposiumBuild> for SymposiumRecord {
    fn from(build: SymposiumBuild) -> Self {
        Self {
            version: build.version,
            sha256: build.sha256.to_string(),
            revision: build.revision,
        }
    }
}

impl From<Deadlines> for DeadlinesRecord {
    fn from(deadlines: Deadlines) -> Self {
        Self {
            setup: deadlines.setup.as_secs(),
            agent: deadlines.agent.as_secs(),
            grading: deadlines.grading.as_secs(),
        }
    }
}

impl IsolationRecord {
    pub(super) fn new(
        mode: IsolationMode,
        srt_version: Option<String>,
        changes: &[ConfigChange],
    ) -> Self {
        Self {
            mode,
            srt_version,
            config_changes: changes.iter().map(ConfigChangeRecord::from).collect(),
        }
    }
}

impl From<&ConfigChange> for ConfigChangeRecord {
    fn from(change: &ConfigChange) -> Self {
        Self {
            root: match change.root {
                Root::Home => ConfigRootRecord::Home,
                Root::Project => ConfigRootRecord::Project,
            },
            file: change.file,
            change: match change.change {
                Change::Created => ConfigChangeKind::Created,
                Change::Modified => ConfigChangeKind::Modified,
                Change::Removed => ConfigChangeKind::Removed,
                Change::Unreadable => ConfigChangeKind::Unreadable,
            },
        }
    }
}
