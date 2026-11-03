//! Cross-checking agent-writable evidence with records from outside the trial (RFD
//! reference, "What the evidence can and cannot show").
//!
//! Each check compares what a piece of evidence claims with the transcript, which the
//! agent cannot write, and the config snapshot. A [`Concern`] is a reason to distrust
//! the evidence, and makes the expectation it decides inconclusive. No concern means
//! only that: no reason was found, which proves nothing about the evidence.
//!
//! A gap in the transcript weakens only a claim that something happened. Evidence that
//! it did not happen already fails its expectation, and stands unless the transcript
//! shows the opposite.
//!
//! Paths are compared by component, a protected artifact under every name the batch
//! has for its root. Its names are never found by resolving links, since the agent can
//! replace any directory below that root. What this cannot see: a command that reaches
//! an artifact by a relative path or through an unrecognized variable, a glob, a script
//! or other shell indirection, a write through a link the agent made, and a match
//! differing only in case, which is only an uncertainty since the filesystem's rules
//! are not known.

use std::path::{Path, PathBuf};

use super::config::ConfigChange;
use super::log::CallEvidence;
use super::sync::SyncEvidence;
use crate::client::{
    CappedText, HookEvent, HookEvidence, HookResult, HookRuns, ObservedPath, ToolActivity,
    ToolResult,
};
use crate::run::Batch;

/// A reason to distrust a piece of evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Concern {
    /// A record from outside the trial says otherwise.
    Contradiction(Reason),
    /// It could be wrong, and nothing from outside the trial settles it.
    Uncertainty(Reason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Reason {
    /// A complete transcript shows no run of the event the evidence says ran.
    HookAbsent {
        event: HookEvent,
    },
    /// The transcript shows the event ran without succeeding, which the evidence says
    /// it did.
    HookDidNotSucceed {
        event: HookEvent,
        results: Vec<HookResult>,
    },
    /// The transcript shows the event succeeding, which the evidence does not record.
    HookSucceeded {
        event: HookEvent,
    },
    /// The transcript could have hidden a run.
    HookRecordIncomplete {
        event: HookEvent,
    },
    /// A complete transcript shows fewer runs of the event than the hook left records,
    /// so at least one was written by something else.
    ExtraHookRecords {
        event: HookEvent,
        records: usize,
        runs: usize,
    },
    /// The transcript shows only outcomes the adapter does not know.
    HookOutcomeUnknown {
        event: HookEvent,
    },
    /// A call that succeeded changed the artifact.
    Mutated {
        protected: Protected,
        path: PathBuf,
    },
    /// A call that may have changed the artifact failed or has no result.
    MutationUnsettled {
        protected: Protected,
        path: PathBuf,
        result: ToolResult,
    },
    /// A call may have changed a file the transcript does not place: it could be any
    /// file.
    UnplacedMutation {
        reported: Option<String>,
        result: ToolResult,
    },
    /// A call changed a path that differs from the artifact's only in case.
    CaseOnlyMatch {
        protected: Protected,
        path: PathBuf,
        result: ToolResult,
    },
    /// The tool activity could have hidden a call.
    ActivityIncomplete,
    /// A command names the artifact; its effect is not known.
    CommandNamesProtected {
        protected: Protected,
        command: CappedText,
        result: ToolResult,
    },
    /// A command too long to keep whole could name the artifact past what was kept.
    CommandTruncated {
        command: CappedText,
        result: ToolResult,
    },
    ConfigChanged(ConfigChange),
}

/// An artifact the agent could alter that some evidence is read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Protected {
    HookTrace,
    HookRecords,
    SyncRecords,
    InstalledSkills,
}

/// Whether a protected artifact is one file or everything under a directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Extent {
    File,
    Tree,
}

/// A protected artifact where the trial keeps it.
#[derive(Debug, Clone)]
pub(crate) struct ProtectedRoot {
    protected: Protected,
    extent: Extent,
    /// The path under each name of the evaluation root.
    forms: Vec<PathBuf>,
    /// Variables a command can reach it through.
    variables: &'static [&'static str],
}

impl ProtectedRoot {
    /// `protected` at `path`, which the trial built under `batch`'s canonical root. Reads
    /// nothing from disk, so it can be built after the session.
    pub(crate) fn new(batch: &Batch, protected: Protected, extent: Extent, path: &Path) -> Self {
        Self::under(
            protected,
            extent,
            path,
            batch.eval_root(),
            batch.eval_root_names(),
        )
    }

    /// `path` under `root`, spelled under each of `names` for that root.
    fn under(
        protected: Protected,
        extent: Extent,
        path: &Path,
        root: &Path,
        names: Vec<PathBuf>,
    ) -> Self {
        let relative = path
            .strip_prefix(root)
            .expect("protected artifact belongs to the evaluation batch");
        let forms = names.into_iter().map(|name| name.join(relative)).collect();
        let variables: &[&str] = match protected {
            Protected::HookTrace => &["SYMPOSIUM_HOOK_TRACE", "EVAL_EVIDENCE_DIR"],
            Protected::HookRecords => &["EVAL_EVIDENCE_DIR"],
            Protected::SyncRecords | Protected::InstalledSkills => &[],
        };
        Self {
            protected,
            extent,
            forms,
            variables,
        }
    }

    fn forms(&self) -> impl Iterator<Item = &Path> {
        self.forms.iter().map(PathBuf::as_path)
    }

    fn holds(&self, path: &Path, form: &Path) -> bool {
        match self.extent {
            Extent::File => path == form,
            Extent::Tree => path.starts_with(form),
        }
    }

    /// Whether a command's text names the artifact, by either form or a variable.
    fn named_in(&self, command: &str) -> bool {
        let spellings = self.forms().flat_map(|form| {
            let text = form.to_string_lossy();
            [text.replace('\\', "/"), text.into_owned()]
        });
        spellings
            .filter(|spelling| !spelling.is_empty())
            .any(|spelling| command.contains(&spelling))
            || self.variables.iter().any(|name| command.contains(name))
    }
}

/// What the transcript shows for one hook event.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Seen {
    Succeeded,
    /// It may have run, unseen.
    Incomplete,
    Absent,
    /// It ran, with only outcomes the adapter does not know.
    Unknown,
    NotSucceeded(Vec<HookResult>),
}

fn seen(runs: &HookRuns, event: &HookEvent) -> Seen {
    let results: Vec<HookResult> = runs
        .runs
        .iter()
        .filter(|run| &run.event == event)
        .map(|run| run.result.clone())
        .collect();
    if results
        .iter()
        .any(|result| matches!(result, HookResult::Succeeded { .. }))
    {
        Seen::Succeeded
    } else if !runs.complete {
        Seen::Incomplete
    } else if results.is_empty() {
        Seen::Absent
    } else if results
        .iter()
        .any(|result| matches!(result, HookResult::Other { .. }))
    {
        Seen::Unknown
    } else {
        Seen::NotSucceeded(results)
    }
}

/// The `SessionStart` call in the hook trace, against the client's own report of it:
/// the two describe the same invocation, so each can contradict the other. A missing or
/// failed trace stands unless the transcript shows the hook succeeding.
pub(crate) fn session_start_trace(trace: CallEvidence, hooks: &HookEvidence) -> Vec<Concern> {
    let event = HookEvent::SessionStart;
    let Some(runs) = observed(hooks, &event) else {
        return Vec::new();
    };
    let concern = match (trace, seen(runs, &event)) {
        (CallEvidence::Succeeded, Seen::Succeeded) => return Vec::new(),
        (CallEvidence::Succeeded, Seen::Absent) => {
            Concern::Contradiction(Reason::HookAbsent { event })
        }
        (CallEvidence::Succeeded, Seen::NotSucceeded(results)) => {
            Concern::Contradiction(Reason::HookDidNotSucceed { event, results })
        }
        (CallEvidence::Succeeded, Seen::Incomplete) => {
            Concern::Uncertainty(Reason::HookRecordIncomplete { event })
        }
        (CallEvidence::Succeeded, Seen::Unknown) => {
            Concern::Uncertainty(Reason::HookOutcomeUnknown { event })
        }
        // A call that responded leaves a trace line, so a missing one was removed.
        (CallEvidence::Failed | CallEvidence::Missing, Seen::Succeeded) => {
            Concern::Contradiction(Reason::HookSucceeded { event })
        }
        (CallEvidence::Failed | CallEvidence::Missing | CallEvidence::Unknown, _) => {
            return Vec::new();
        }
    };
    vec![concern]
}

/// Symposium's sync record, against the client's report of the `SessionStart` it is
/// written in. A sync can finish before the hook fails, and the hook can succeed after
/// a sync fails, so only a hook that never ran contradicts a record.
pub(crate) fn sync_record(sync: SyncEvidence, hooks: &HookEvidence) -> Vec<Concern> {
    let event = HookEvent::SessionStart;
    match observed(hooks, &event) {
        Some(runs) if sync == SyncEvidence::Recorded => record_of_a_run(seen(runs, &event), event),
        _ => Vec::new(),
    }
}

/// A plugin hook's record, and how many records it left, against the client's report
/// of the event Symposium dispatched it in. The plugin's hook can succeed while the
/// dispatch fails after it, and the dispatch can succeed when the plugin's hook did not,
/// so only an event that never ran contradicts a successful record. Each run dispatches
/// the hook once, so a record beyond the runs was written by something else, whatever
/// the records say.
pub(crate) fn plugin_hook_record(
    record: CallEvidence,
    records: usize,
    event: &HookEvent,
    hooks: &HookEvidence,
) -> Vec<Concern> {
    let Some(runs) = observed(hooks, event) else {
        return Vec::new();
    };
    if record == CallEvidence::Succeeded {
        let concerns = record_of_a_run(seen(runs, event), event.clone());
        // Already a contradiction, or a transcript too incomplete to count runs in.
        if !concerns.is_empty() {
            return concerns;
        }
    }
    let ran = runs.runs.iter().filter(|run| run.event == *event).count();
    let mut concerns = Vec::new();
    if runs.complete && records > ran {
        concerns.push(Concern::Contradiction(Reason::ExtraHookRecords {
            event: event.clone(),
            records,
            runs: ran,
        }));
    }
    concerns
}

/// The runs, when the client reports every run of `event`. Otherwise its absence
/// decides nothing.
fn observed<'a>(hooks: &'a HookEvidence, event: &HookEvent) -> Option<&'a HookRuns> {
    match hooks {
        HookEvidence::Reported(runs) if runs.observes(event) => Some(runs),
        HookEvidence::Reported(_) | HookEvidence::Unsupported => None,
    }
}

/// Concerns for a record written inside a run of `event`, as the transcript saw it.
fn record_of_a_run(seen: Seen, event: HookEvent) -> Vec<Concern> {
    match seen {
        Seen::Absent => vec![Concern::Contradiction(Reason::HookAbsent { event })],
        Seen::Incomplete => vec![Concern::Uncertainty(Reason::HookRecordIncomplete { event })],
        Seen::Succeeded | Seen::Unknown | Seen::NotSucceeded(_) => Vec::new(),
    }
}

/// What the agent's tool activity says about `root`.
pub(crate) fn activity(root: &ProtectedRoot, activity: &ToolActivity) -> Vec<Concern> {
    let protected = root.protected;
    let mut concerns = Vec::new();
    if !activity.complete {
        concerns.push(Concern::Uncertainty(Reason::ActivityIncomplete));
    }
    for mutation in &activity.mutations {
        let result = mutation.result;
        let path = match &mutation.path {
            ObservedPath::Readable {
                resolved: Some(path),
                ..
            } => path,
            ObservedPath::Readable { reported, .. } => {
                let reported = Some(reported.clone());
                concerns.push(Concern::Uncertainty(Reason::UnplacedMutation {
                    reported,
                    result,
                }));
                continue;
            }
            ObservedPath::Unreadable => {
                let reported = None;
                concerns.push(Concern::Uncertainty(Reason::UnplacedMutation {
                    reported,
                    result,
                }));
                continue;
            }
        };
        let path = path.clone();
        if root.forms().any(|form| root.holds(&path, form)) {
            concerns.push(match mutation.result {
                ToolResult::Succeeded => {
                    Concern::Contradiction(Reason::Mutated { protected, path })
                }
                result => Concern::Uncertainty(Reason::MutationUnsettled {
                    protected,
                    path,
                    result,
                }),
            });
        } else if root.forms().any(|form| case_only(root, &path, form)) {
            concerns.push(Concern::Uncertainty(Reason::CaseOnlyMatch {
                protected,
                path,
                result: mutation.result,
            }));
        }
    }
    for command in &activity.commands {
        let text = &command.text;
        if root.named_in(&text.prefix) {
            concerns.push(Concern::Uncertainty(Reason::CommandNamesProtected {
                protected,
                command: text.clone(),
                result: command.result,
            }));
        }
        if text.original_bytes > text.prefix.len() {
            concerns.push(Concern::Uncertainty(Reason::CommandTruncated {
                command: text.clone(),
                result: command.result,
            }));
        }
    }
    concerns
}

/// Whether `path` would be held by `form` if case were ignored, ASCII only.
fn case_only(root: &ProtectedRoot, path: &Path, form: &Path) -> bool {
    let folded = |path: &Path| {
        path.to_str()
            .map(str::to_ascii_lowercase)
            .map(PathBuf::from)
    };
    match (folded(path), folded(form)) {
        (Some(path), Some(form)) => root.holds(&path, &form),
        _ => false,
    }
}

/// A concern for each config change. Every watched file can make the client or Symposium
/// run a command outside the agent's tool calls, which can write any evidence, so each
/// change bears on every expectation.
pub(crate) fn config(changes: &[ConfigChange]) -> Vec<Concern> {
    changes
        .iter()
        .map(|change| Concern::Uncertainty(Reason::ConfigChanged(*change)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{FileMutation, HookRun, ShellCommand};
    use crate::expect::config::{Change, Root};
    use crate::run::setup::SYMPOSIUM_CONFIG;
    use crate::run::test_support::Host;

    const OBSERVED: &[HookEvent] = &[HookEvent::SessionStart, HookEvent::Stop];
    const SUCCESS: HookResult = HookResult::Succeeded { exit_code: Some(0) };
    const FAILURE: HookResult = HookResult::Failed { exit_code: Some(1) };

    fn other() -> HookResult {
        HookResult::Other {
            outcome: "blocked".to_owned(),
            exit_code: None,
        }
    }

    /// A transcript reporting `results` for `event`.
    fn reported(event: HookEvent, results: &[HookResult], complete: bool) -> HookEvidence {
        let runs = results
            .iter()
            .map(|result| HookRun {
                event: event.clone(),
                result: result.clone(),
            })
            .collect();
        HookEvidence::Reported(HookRuns {
            runs,
            observed: OBSERVED,
            complete,
        })
    }

    /// The kind of each concern, for tables.
    #[derive(Debug, PartialEq)]
    enum Kind {
        Contradiction,
        Uncertainty,
    }

    fn kinds(concerns: &[Concern]) -> Vec<Kind> {
        concerns
            .iter()
            .map(|concern| match concern {
                Concern::Contradiction(_) => Kind::Contradiction,
                Concern::Uncertainty(_) => Kind::Uncertainty,
            })
            .collect()
    }

    use CallEvidence::{Failed, Missing, Succeeded, Unknown};
    use Kind::{Contradiction, Uncertainty};

    #[test]
    fn the_trace_is_checked_against_claude_codes_session_start() {
        let event = HookEvent::SessionStart;
        let rows: [(CallEvidence, HookEvidence, &[Kind]); 9] = [
            (Succeeded, reported(event.clone(), &[SUCCESS], true), &[]),
            (
                Succeeded,
                reported(event.clone(), &[], true),
                &[Contradiction],
            ),
            (
                Succeeded,
                reported(event.clone(), &[FAILURE], true),
                &[Contradiction],
            ),
            (
                Succeeded,
                reported(event.clone(), &[], false),
                &[Uncertainty],
            ),
            (
                Succeeded,
                reported(event.clone(), &[other()], true),
                &[Uncertainty],
            ),
            // A call that responded leaves a trace line.
            (
                Missing,
                reported(event.clone(), &[SUCCESS], true),
                &[Contradiction],
            ),
            (
                Failed,
                reported(event.clone(), &[SUCCESS], true),
                &[Contradiction],
            ),
            (Missing, reported(event.clone(), &[], true), &[]),
            (Unknown, reported(event.clone(), &[], true), &[]),
        ];
        for (trace, hooks, expected) in rows {
            let concerns = session_start_trace(trace, &hooks);

            assert_eq!(kinds(&concerns), expected, "{trace:?} {hooks:?}");
        }
    }

    #[test]
    fn a_sync_record_is_contradicted_only_by_a_session_start_that_never_ran() {
        let event = HookEvent::SessionStart;
        let recorded = SyncEvidence::Recorded;
        let rows: [(SyncEvidence, HookEvidence, &[Kind]); 6] = [
            (recorded, reported(event.clone(), &[SUCCESS], true), &[]),
            (
                recorded,
                reported(event.clone(), &[], true),
                &[Contradiction],
            ),
            (
                recorded,
                reported(event.clone(), &[], false),
                &[Uncertainty],
            ),
            // The sync can finish before the hook fails.
            (recorded, reported(event.clone(), &[FAILURE], true), &[]),
            // A hook can succeed after its sync failed.
            (
                SyncEvidence::NotRecorded,
                reported(event.clone(), &[SUCCESS], true),
                &[],
            ),
            (
                SyncEvidence::Unknown,
                reported(event.clone(), &[], true),
                &[],
            ),
        ];
        for (sync, hooks, expected) in rows {
            let concerns = sync_record(sync, &hooks);

            assert_eq!(kinds(&concerns), expected, "{sync:?} {hooks:?}");
        }
    }

    #[test]
    fn a_plugin_hook_record_is_contradicted_only_by_an_event_that_never_ran() {
        let stop = HookEvent::Stop;
        let rows: [(CallEvidence, HookEvidence, &[Kind]); 5] = [
            (Succeeded, reported(stop.clone(), &[SUCCESS], true), &[]),
            (
                Succeeded,
                reported(stop.clone(), &[], true),
                &[Contradiction],
            ),
            (
                Succeeded,
                reported(stop.clone(), &[], false),
                &[Uncertainty],
            ),
            // The plugin's hook can succeed while the dispatch fails after it.
            (Succeeded, reported(stop.clone(), &[FAILURE], true), &[]),
            // The dispatch succeeding does not mean the plugin's hook did.
            (Missing, reported(stop.clone(), &[SUCCESS], true), &[]),
        ];
        for (record, hooks, expected) in rows {
            let concerns = plugin_hook_record(record, records(record), &stop, &hooks);

            assert_eq!(kinds(&concerns), expected, "{record:?} {hooks:?}");
        }
    }

    /// One record for evidence that the hook ran, none for evidence that it did not.
    fn records(record: CallEvidence) -> usize {
        usize::from(matches!(record, Succeeded | Failed))
    }

    #[test]
    fn more_records_than_runs_contradicts_them_whatever_they_say() {
        let stop = HookEvent::Stop;
        let once = reported(stop.clone(), &[FAILURE], true);

        for record in [Succeeded, Failed] {
            assert_eq!(
                plugin_hook_record(record, 2, &stop, &once),
                [Concern::Contradiction(Reason::ExtraHookRecords {
                    event: stop.clone(),
                    records: 2,
                    runs: 1,
                })]
            );
        }
        let twice = reported(stop.clone(), &[FAILURE, SUCCESS], true);
        assert_eq!(plugin_hook_record(Succeeded, 2, &stop, &twice), []);
    }

    #[test]
    fn extra_records_decide_nothing_when_the_transcript_could_hide_a_run() {
        let stop = HookEvent::Stop;
        let incomplete = reported(stop.clone(), &[SUCCESS], false);

        assert_eq!(plugin_hook_record(Failed, 2, &stop, &incomplete), []);
    }

    #[test]
    fn an_event_the_adapter_does_not_observe_decides_nothing() {
        let notification = HookEvent::Other("Notification".to_owned());
        let hooks = reported(HookEvent::Stop, &[], true);

        assert_eq!(plugin_hook_record(Succeeded, 1, &notification, &hooks), []);
    }

    #[test]
    fn an_adapter_decides_which_events_it_observes() {
        let HookEvidence::Reported(mut runs) = reported(HookEvent::Stop, &[], true) else {
            unreachable!();
        };
        runs.observed = &[HookEvent::Stop];
        let only_stop = HookEvidence::Reported(runs);

        assert_eq!(session_start_trace(Succeeded, &only_stop), []);
        assert_eq!(sync_record(SyncEvidence::Recorded, &only_stop), []);
        assert_eq!(
            kinds(&plugin_hook_record(
                Succeeded,
                1,
                &HookEvent::Stop,
                &only_stop
            )),
            [Contradiction]
        );
    }

    #[test]
    fn a_client_that_does_not_report_hooks_raises_no_concern() {
        let unsupported = HookEvidence::Unsupported;

        assert_eq!(session_start_trace(Succeeded, &unsupported), []);
        assert_eq!(sync_record(SyncEvidence::Recorded, &unsupported), []);
        assert_eq!(
            plugin_hook_record(Succeeded, 1, &HookEvent::Stop, &unsupported),
            []
        );
    }

    fn eval_root() -> PathBuf {
        PathBuf::from(if cfg!(windows) { "C:\\eval" } else { "/eval" })
    }

    fn trace() -> PathBuf {
        eval_root().join("batch/trial-1/evidence/hook-trace.jsonl")
    }

    /// The trial's hook trace, under an evaluation root with one name.
    fn trace_root() -> ProtectedRoot {
        let (path, root) = (trace(), eval_root());
        ProtectedRoot::under(
            Protected::HookTrace,
            Extent::File,
            &path,
            &root,
            vec![root.clone()],
        )
    }

    fn at(path: &Path, result: ToolResult) -> FileMutation {
        FileMutation {
            path: ObservedPath::Readable {
                reported: path.to_string_lossy().into_owned(),
                resolved: Some(path.to_owned()),
            },
            result,
        }
    }

    fn of(mutations: Vec<FileMutation>, commands: Vec<ShellCommand>) -> ToolActivity {
        ToolActivity {
            mutations,
            commands,
            complete: true,
        }
    }

    fn command(text: &str) -> ShellCommand {
        ShellCommand {
            text: CappedText::new(text, 4096),
            result: ToolResult::Succeeded,
        }
    }

    #[test]
    fn a_mutation_of_the_artifact_contradicts_it_and_an_unsettled_one_is_uncertain() {
        let root = trace_root();
        let trace = trace();

        let succeeded = activity(&root, &of(vec![at(&trace, ToolResult::Succeeded)], vec![]));
        let failed = activity(&root, &of(vec![at(&trace, ToolResult::Failed)], vec![]));
        let missing = activity(&root, &of(vec![at(&trace, ToolResult::Missing)], vec![]));

        assert_eq!(kinds(&succeeded), [Contradiction]);
        assert_eq!(kinds(&failed), [Uncertainty]);
        assert_eq!(kinds(&missing), [Uncertainty]);
    }

    #[test]
    fn a_file_is_protected_alone_and_a_tree_with_everything_under_it() {
        let trace_root = trace_root();
        let beside = trace().with_file_name("notes.txt");
        let records_dir = trace().with_file_name("records");
        let root = eval_root();
        let records = ProtectedRoot::under(
            Protected::SyncRecords,
            Extent::Tree,
            &records_dir,
            &root,
            vec![root.clone()],
        );
        let nested = records_dir.join("a/state.json");

        let sibling = activity(
            &trace_root,
            &of(vec![at(&beside, ToolResult::Succeeded)], vec![]),
        );
        let inside = activity(
            &records,
            &of(vec![at(&nested, ToolResult::Succeeded)], vec![]),
        );

        assert_eq!(sibling, []);
        assert_eq!(kinds(&inside), [Contradiction]);
    }

    #[test]
    fn the_artifact_is_matched_under_every_name_of_the_evaluation_root() {
        let root = eval_root();
        let given = PathBuf::from(if cfg!(windows) { "C:\\link" } else { "/link" });
        let names = vec![given.clone(), root.clone()];
        let trace_root =
            ProtectedRoot::under(Protected::HookTrace, Extent::File, &trace(), &root, names);
        let through_given = given.join("batch/trial-1/evidence/hook-trace.jsonl");

        let concerns = activity(
            &trace_root,
            &of(vec![at(&through_given, ToolResult::Succeeded)], vec![]),
        );

        assert_eq!(kinds(&concerns), [Contradiction]);
    }

    #[test]
    #[should_panic(expected = "protected artifact belongs to the evaluation batch")]
    fn an_artifact_outside_the_batch_is_a_wiring_bug() {
        let outside = PathBuf::from(if cfg!(windows) {
            "C:\\elsewhere"
        } else {
            "/elsewhere"
        });
        let root = eval_root();

        ProtectedRoot::under(
            Protected::HookTrace,
            Extent::File,
            &outside,
            &root,
            vec![root.clone()],
        );
    }

    #[test]
    fn a_root_from_the_batch_needs_nothing_on_disk() {
        let host = Host::new();
        let batch = host.batch();
        let path = batch
            .eval_root()
            .join("batch/trial-1/evidence/hook-trace.jsonl");

        let root = ProtectedRoot::new(&batch, Protected::HookTrace, Extent::File, &path);

        assert_eq!(root.forms, [path]);
    }

    #[test]
    fn a_mutation_the_transcript_cannot_place_could_be_any_artifact() {
        let unreadable = FileMutation {
            path: ObservedPath::Unreadable,
            result: ToolResult::Succeeded,
        };
        let rootless = FileMutation {
            path: ObservedPath::Readable {
                reported: "x".to_owned(),
                resolved: None,
            },
            result: ToolResult::Succeeded,
        };

        let rootless = FileMutation {
            result: ToolResult::Failed,
            ..rootless
        };

        let concerns = activity(&trace_root(), &of(vec![unreadable, rootless], vec![]));

        assert_eq!(
            concerns,
            [
                Concern::Uncertainty(Reason::UnplacedMutation {
                    reported: None,
                    result: ToolResult::Succeeded,
                }),
                Concern::Uncertainty(Reason::UnplacedMutation {
                    reported: Some("x".to_owned()),
                    result: ToolResult::Failed,
                }),
            ]
        );
    }

    #[test]
    fn a_path_differing_only_in_case_is_uncertain() {
        let root = trace_root();
        let shouted = PathBuf::from(trace().to_str().unwrap().to_ascii_uppercase());

        let concerns = activity(&root, &of(vec![at(&shouted, ToolResult::Failed)], vec![]));

        assert_eq!(
            concerns,
            [Concern::Uncertainty(Reason::CaseOnlyMatch {
                protected: Protected::HookTrace,
                path: shouted,
                result: ToolResult::Failed,
            })]
        );
    }

    #[test]
    fn an_incomplete_record_cannot_clear_the_artifact() {
        let mut incomplete = of(vec![], vec![]);
        incomplete.complete = false;

        let concerns = activity(&trace_root(), &incomplete);

        assert_eq!(concerns, [Concern::Uncertainty(Reason::ActivityIncomplete)]);
    }

    #[test]
    fn a_command_naming_the_artifact_by_path_or_variable_is_uncertain() {
        let root = trace_root();
        let by_path = format!("echo x >> {}", trace().display());
        let by_slashes = by_path.replace('\\', "/");

        for text in [
            by_path.as_str(),
            &by_slashes,
            "echo x >> $SYMPOSIUM_HOOK_TRACE",
        ] {
            let concerns = activity(&root, &of(vec![], vec![command(text)]));

            assert_eq!(kinds(&concerns), [Uncertainty], "{text}");
        }
        // A bare name is too common in harmless commands to count.
        assert_eq!(
            activity(&root, &of(vec![], vec![command("ls evidence")])),
            []
        );
    }

    #[test]
    fn a_command_longer_than_what_was_kept_is_uncertain() {
        let long = ShellCommand {
            text: CappedText::new(&"x".repeat(5000), 4096),
            result: ToolResult::Missing,
        };

        let concerns = activity(&trace_root(), &of(vec![], vec![long.clone()]));

        assert_eq!(
            concerns,
            [Concern::Uncertainty(Reason::CommandTruncated {
                command: long.text,
                result: ToolResult::Missing,
            })]
        );
    }

    #[test]
    fn a_long_command_naming_the_artifact_keeps_both_reasons() {
        let text = format!("echo x >> {} {}", trace().display(), "x".repeat(5000));
        let long = ShellCommand {
            text: CappedText::new(&text, 4096),
            result: ToolResult::Failed,
        };

        let concerns = activity(&trace_root(), &of(vec![], vec![long.clone()]));

        assert_eq!(
            concerns,
            [
                Concern::Uncertainty(Reason::CommandNamesProtected {
                    protected: Protected::HookTrace,
                    command: long.text.clone(),
                    result: ToolResult::Failed,
                }),
                Concern::Uncertainty(Reason::CommandTruncated {
                    command: long.text,
                    result: ToolResult::Failed,
                }),
            ]
        );
    }

    #[test]
    fn each_config_change_is_a_concern() {
        let change = |root, file| ConfigChange {
            root,
            file,
            change: Change::Modified,
        };
        let client = change(Root::Project, ".claude/settings.local.json");
        let symposium = change(Root::Home, SYMPOSIUM_CONFIG);

        assert_eq!(
            config(&[client, symposium]),
            [
                Concern::Uncertainty(Reason::ConfigChanged(client)),
                Concern::Uncertainty(Reason::ConfigChanged(symposium)),
            ]
        );
    }
}
