//! What a result records of a Symposium trial's expectations.

use std::collections::BTreeMap;

use serde::Serialize;

use super::concern::ConcernRecord;
use super::session::LoadRecord;
use crate::client::HookEvent;
use crate::expect::decide::{Checked, Expectations, HasVerdict, HookExpectation};
use crate::expect::installed::{InstallAssertion, InstallEvidence, SkillInstall};
use crate::expect::log::CallEvidence;
use crate::expect::sync::SyncEvidence;
use crate::expect::{SkillUse, UseAssertion, Verdict};

#[derive(Debug, Serialize)]
pub(super) struct ExpectationsRecord {
    skills: BTreeMap<String, SkillRecord>,
    hooks: BTreeMap<String, HookRecord>,
    sync: SyncRecord,
}

#[derive(Debug, Serialize)]
struct SyncRecord {
    session_start: EvidenceRecord,
    sync_recorded: EvidenceRecord,
}

/// An expectation's reported verdict, the verdict its evidence gives alone, and every
/// concern about that evidence.
#[derive(Debug, Serialize)]
struct EvidenceRecord {
    verdict: VerdictRecord,
    evidence: EvidenceKind,
    evidence_verdict: VerdictRecord,
    concerns: Vec<ConcernRecord>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum VerdictRecord {
    Pass,
    Fail,
    Inconclusive,
}

/// What the evidence for one expectation showed.
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum EvidenceKind {
    Succeeded,
    Failed,
    Missing,
    Unknown,
    Recorded,
    NotRecorded,
    Absent,
    Matching,
    Differing,
    Present,
    Unreadable,
    Unidentified,
    NoSource,
}

#[derive(Debug, Serialize)]
struct HookRecord {
    #[serde(flatten)]
    expectation: EvidenceRecord,
    event: String,
    records: usize,
}

/// Each assertion the task makes about one skill.
#[derive(Debug, Default, Serialize)]
struct SkillRecord {
    #[serde(skip_serializing_if = "Option::is_none")]
    installed: Option<EvidenceRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    not_installed: Option<EvidenceRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    used: Option<UseRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    not_used: Option<UseRecord>,
}

#[derive(Debug, Serialize)]
struct UseRecord {
    verdict: VerdictRecord,
    observed_load: LoadRecord,
}

impl From<&Expectations> for ExpectationsRecord {
    fn from(expectations: &Expectations) -> Self {
        Self {
            skills: skills(&expectations.installs, &expectations.uses),
            hooks: expectations
                .hooks
                .iter()
                .map(|checked| {
                    (
                        checked.decided.hook.name.to_string(),
                        HookRecord::from(checked),
                    )
                })
                .collect(),
            sync: SyncRecord {
                session_start: EvidenceRecord::new(
                    &expectations.session_start,
                    EvidenceKind::from(expectations.session_start.decided),
                ),
                sync_recorded: EvidenceRecord::new(
                    &expectations.sync,
                    EvidenceKind::from(expectations.sync.decided),
                ),
            },
        }
    }
}

/// Every skill's assertions, keyed by the skill.
fn skills(installs: &[Checked<SkillInstall>], uses: &[SkillUse]) -> BTreeMap<String, SkillRecord> {
    let mut skills: BTreeMap<String, SkillRecord> = BTreeMap::new();
    for checked in installs {
        let install = &checked.decided;
        let record = Some(EvidenceRecord::new(
            checked,
            EvidenceKind::from(install.evidence),
        ));
        let entry = skills.entry(install.skill.to_string()).or_default();
        match install.assertion {
            InstallAssertion::Installed => entry.installed = record,
            InstallAssertion::NotInstalled => entry.not_installed = record,
        }
    }
    for used in uses {
        let record = Some(UseRecord::from(used));
        let entry = skills.entry(used.skill.to_string()).or_default();
        match used.assertion {
            UseAssertion::Used => entry.used = record,
            UseAssertion::NotUsed => entry.not_used = record,
        }
    }
    skills
}

impl EvidenceRecord {
    fn new<T: HasVerdict>(checked: &Checked<T>, evidence: EvidenceKind) -> Self {
        Self {
            verdict: VerdictRecord::from(checked.verdict()),
            evidence,
            evidence_verdict: VerdictRecord::from(checked.evidence_verdict()),
            concerns: checked.concerns.iter().map(ConcernRecord::from).collect(),
        }
    }
}

impl From<&Checked<HookExpectation>> for HookRecord {
    fn from(checked: &Checked<HookExpectation>) -> Self {
        let hook = &checked.decided;
        Self {
            expectation: EvidenceRecord::new(checked, EvidenceKind::from(hook.evidence)),
            event: event_name(&hook.hook.event).to_owned(),
            records: hook.records,
        }
    }
}

impl From<&SkillUse> for UseRecord {
    fn from(used: &SkillUse) -> Self {
        Self {
            verdict: VerdictRecord::from(used.verdict),
            observed_load: LoadRecord::from(used.load),
        }
    }
}

impl From<Verdict> for VerdictRecord {
    fn from(verdict: Verdict) -> Self {
        match verdict {
            Verdict::Pass => Self::Pass,
            Verdict::Fail => Self::Fail,
            Verdict::Inconclusive => Self::Inconclusive,
        }
    }
}

impl From<CallEvidence> for EvidenceKind {
    fn from(evidence: CallEvidence) -> Self {
        match evidence {
            CallEvidence::Succeeded => Self::Succeeded,
            CallEvidence::Failed => Self::Failed,
            CallEvidence::Missing => Self::Missing,
            CallEvidence::Unknown => Self::Unknown,
        }
    }
}

impl From<SyncEvidence> for EvidenceKind {
    fn from(evidence: SyncEvidence) -> Self {
        match evidence {
            SyncEvidence::Recorded => Self::Recorded,
            SyncEvidence::NotRecorded => Self::NotRecorded,
            SyncEvidence::Unknown => Self::Unknown,
        }
    }
}

impl From<InstallEvidence> for EvidenceKind {
    fn from(evidence: InstallEvidence) -> Self {
        match evidence {
            InstallEvidence::Absent => Self::Absent,
            InstallEvidence::Matching => Self::Matching,
            InstallEvidence::Differing => Self::Differing,
            InstallEvidence::Present => Self::Present,
            InstallEvidence::Unreadable => Self::Unreadable,
            InstallEvidence::Unidentified => Self::Unidentified,
            InstallEvidence::NoSource => Self::NoSource,
        }
    }
}

/// An event by Symposium's name for it.
pub(super) fn event_name(event: &HookEvent) -> &str {
    match event {
        HookEvent::SessionStart => "SessionStart",
        HookEvent::Stop => "Stop",
        HookEvent::Other(name) => name,
    }
}
