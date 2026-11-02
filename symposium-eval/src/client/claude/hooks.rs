//! The hook runs a Claude Code transcript reports, with `--include-hook-events`.
//!
//! A `hook_response` says everything about its run, so each with a readable event and
//! outcome is a run, whether or not its `hook_started` was seen. Starts matter only for a
//! hook that never answered: one left over when reading ends is
//! [`HookResult::NoResponse`]. Claude Code does not promise unique `hook_id`s, so a start
//! pairs with a response by its ID, event and name together, and a repeated ID damages
//! nothing.

use std::collections::HashMap;

use serde_json::value::RawValue;

use crate::client::{HookEvent, HookEvidence, HookResult, HookRun, HookRuns};

/// The events whose every run Claude Code reports, `SessionStart` before `init` and
/// `Stop` before `result`.
pub(super) const OBSERVED: &[HookEvent] = &[HookEvent::SessionStart, HookEvent::Stop];

/// Larger than the hook runs of any session; past it, runs are no longer kept.
const MAX_KEPT: usize = 64 * 1024 * 1024;

/// A hook event's fields, raw from its line. Its output is skipped unread: it can be
/// large, and nothing reads it.
#[derive(Debug, Default)]
pub(super) struct HookFields<'a> {
    pub(super) hook_id: Option<&'a RawValue>,
    pub(super) hook_name: Option<&'a RawValue>,
    pub(super) hook_event: Option<&'a RawValue>,
    pub(super) outcome: Option<&'a RawValue>,
    pub(super) exit_code: Option<&'a RawValue>,
}

/// What pairs a start with its response: its ID, name and event.
type Key = (String, String, String);

/// The hook runs a transcript showed.
#[derive(Debug)]
pub(super) struct Hooks {
    runs: Vec<HookRun>,
    /// Starts awaiting their response, counted.
    open: HashMap<Key, usize>,
    /// The bytes of what is kept, at most `limit`.
    kept: usize,
    limit: usize,
    /// Whether something that could hide a run was unreadable, not this session's, or
    /// too much to keep.
    damaged: bool,
    finished: bool,
}

impl Hooks {
    pub(super) fn new() -> Self {
        Self::keeping(MAX_KEPT)
    }

    fn keeping(limit: usize) -> Self {
        Self {
            runs: Vec::new(),
            open: HashMap::new(),
            kept: 0,
            limit,
            damaged: false,
            finished: false,
        }
    }

    /// Reads a hook event of `subtype`. A `hook_started` or `hook_response` that cannot
    /// be read could hide a run; other hook events, such as `hook_progress`, say nothing
    /// about how one ended.
    pub(super) fn event(&mut self, subtype: &str, fields: &HookFields<'_>) {
        let text = |raw: Option<&RawValue>| serde_json::from_str::<String>(raw?.get()).ok();
        let (id, name, event) = (
            text(fields.hook_id),
            text(fields.hook_name),
            text(fields.hook_event),
        );
        match subtype {
            "hook_started" => match (id, name, event) {
                (Some(id), Some(name), Some(event)) => self.started((id, name, event)),
                _ => self.damage(),
            },
            "hook_response" => match (event, text(fields.outcome)) {
                (Some(event), Some(outcome)) => {
                    // Detail, so one that cannot be read is left out, not the run.
                    let exit_code = fields
                        .exit_code
                        .and_then(|raw| serde_json::from_str(raw.get()).ok());
                    let key = id.zip(name).map(|(id, name)| (id, name, event.clone()));
                    self.responded(key, &event, &outcome, exit_code);
                }
                _ => self.damage(),
            },
            _ => {}
        }
    }

    fn started(&mut self, key: Key) {
        // An entry costs its size even when its fields are empty.
        let bytes = size_of::<(Key, usize)>() + key.0.len() + key.1.len() + key.2.len();
        if self.keep(bytes) {
            *self.open.entry(key).or_default() += 1;
        }
    }

    /// A response, paired with a start by `key` when its ID and name could be read.
    fn responded(&mut self, key: Option<Key>, event: &str, outcome: &str, exit_code: Option<i32>) {
        if let Some(key) = key
            && let Some(open) = self.open.get_mut(&key)
        {
            *open -= 1;
            if *open == 0 {
                self.open.remove(&key);
            }
        }
        if !self.keep(size_of::<HookRun>() + event.len() + outcome.len()) {
            return;
        }
        let result = match outcome {
            "success" => HookResult::Succeeded { exit_code },
            "error" => HookResult::Failed { exit_code },
            "cancelled" => HookResult::Cancelled { exit_code },
            other => HookResult::Other {
                outcome: other.to_owned(),
                exit_code,
            },
        };
        self.runs.push(HookRun {
            event: HookEvent::named(event),
            result,
        });
    }

    /// Whether `bytes` more can be kept. Past the limit, the record is damaged.
    fn keep(&mut self, bytes: usize) -> bool {
        if self.kept + bytes > self.limit {
            self.damaged = true;
            return false;
        }
        self.kept += bytes;
        true
    }

    /// This session's `result` arrived.
    pub(super) fn finish(&mut self) {
        self.finished = true;
    }

    /// Something in the transcript could hide a run.
    pub(super) fn damage(&mut self) {
        self.damaged = true;
    }

    pub(super) fn evidence(mut self) -> HookEvidence {
        let mut unanswered: Vec<_> = self.open.into_iter().collect();
        // A map's order is arbitrary; the runs should not be.
        unanswered.sort();
        for ((_, _, event), count) in unanswered {
            let run = HookRun {
                event: HookEvent::named(&event),
                result: HookResult::NoResponse,
            };
            self.runs.extend(std::iter::repeat_n(run, count));
        }
        HookEvidence::Reported(HookRuns {
            runs: self.runs,
            observed: OBSERVED,
            complete: self.finished && !self.damaged,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    enum Line {
        Started(&'static str, &'static str),
        Response(&'static str, &'static str, &'static str),
    }

    fn key(id: &str, event: &str) -> Key {
        (id.to_owned(), event.to_owned(), event.to_owned())
    }

    fn read(hooks: &mut Hooks, lines: &[Line]) {
        for line in lines {
            match *line {
                Line::Started(id, event) => hooks.started(key(id, event)),
                Line::Response(id, event, outcome) => {
                    hooks.responded(Some(key(id, event)), event, outcome, Some(0));
                }
            }
        }
    }

    fn reported(hooks: Hooks) -> HookRuns {
        let HookEvidence::Reported(runs) = hooks.evidence() else {
            unreachable!("Claude Code reports hooks");
        };
        runs
    }

    /// The runs `lines` show, read to the session's end.
    fn runs(lines: &[Line]) -> Vec<HookRun> {
        let mut hooks = Hooks::new();
        read(&mut hooks, lines);
        hooks.finish();
        let runs = reported(hooks);
        assert!(runs.complete);
        runs.runs
    }

    fn run(event: HookEvent, result: HookResult) -> HookRun {
        HookRun { event, result }
    }

    const SUCCEEDED: HookResult = HookResult::Succeeded { exit_code: Some(0) };

    #[test]
    fn a_response_is_a_run_with_or_without_its_start() {
        let paired = [
            Line::Started("1", "Stop"),
            Line::Response("1", "Stop", "success"),
        ];
        let alone = [Line::Response("1", "Stop", "success")];

        assert_eq!(runs(&paired), [run(HookEvent::Stop, SUCCEEDED)]);
        assert_eq!(runs(&alone), [run(HookEvent::Stop, SUCCEEDED)]);
    }

    #[test]
    fn a_start_without_its_response_is_unanswered() {
        let lines = [Line::Started("1", "Stop")];

        assert_eq!(runs(&lines), [run(HookEvent::Stop, HookResult::NoResponse)]);
    }

    #[test]
    fn a_repeated_id_pairs_one_start_with_each_response() {
        let two_starts = [
            Line::Started("1", "Stop"),
            Line::Started("1", "Stop"),
            Line::Response("1", "Stop", "success"),
        ];
        let two_responses = [
            Line::Response("1", "Stop", "success"),
            Line::Response("1", "Stop", "success"),
        ];

        assert_eq!(
            runs(&two_starts),
            [
                run(HookEvent::Stop, SUCCEEDED),
                run(HookEvent::Stop, HookResult::NoResponse)
            ]
        );
        assert_eq!(
            runs(&two_responses),
            vec![run(HookEvent::Stop, SUCCEEDED); 2]
        );
    }

    #[test]
    fn a_response_for_another_event_leaves_its_start_unanswered() {
        let lines = [
            Line::Started("1", "Stop"),
            Line::Response("1", "SessionStart", "success"),
        ];

        assert_eq!(
            runs(&lines),
            [
                run(HookEvent::SessionStart, SUCCEEDED),
                run(HookEvent::Stop, HookResult::NoResponse)
            ]
        );
    }

    #[test]
    fn a_response_that_cannot_pair_is_still_a_run() {
        let mut hooks = Hooks::new();
        hooks.started(key("1", "Stop"));
        hooks.responded(None, "Stop", "success", None);
        hooks.finish();

        assert_eq!(
            reported(hooks).runs,
            [
                run(HookEvent::Stop, HookResult::Succeeded { exit_code: None }),
                run(HookEvent::Stop, HookResult::NoResponse)
            ]
        );
    }

    #[test]
    fn each_outcome_is_kept_and_an_unknown_one_decides_nothing() {
        let result_of = |outcome| {
            runs(&[Line::Response("1", "Stop", outcome)])[0]
                .result
                .clone()
        };
        let exit_code = Some(0);

        assert_eq!(result_of("error"), HookResult::Failed { exit_code });
        assert_eq!(result_of("cancelled"), HookResult::Cancelled { exit_code });
        assert_eq!(
            result_of("blocked"),
            HookResult::Other {
                outcome: "blocked".to_owned(),
                exit_code
            }
        );
    }

    #[test]
    fn an_event_the_adapter_does_not_observe_is_kept() {
        let mut hooks = Hooks::new();
        read(
            &mut hooks,
            &[Line::Response("1", "Notification", "success")],
        );
        hooks.finish();
        let runs = reported(hooks);
        let notification = HookEvent::Other("Notification".to_owned());

        assert_eq!(runs.runs[0].event, notification);
        assert!(!runs.observes(&notification));
        assert!(runs.observes(&HookEvent::Stop));
    }

    #[test]
    fn damage_or_no_ending_leaves_the_record_incomplete() {
        let mut damaged = Hooks::new();
        damaged.damage();
        damaged.finish();

        for hooks in [damaged, Hooks::new()] {
            assert!(!reported(hooks).complete);
        }
    }

    #[test]
    fn starts_and_responses_past_the_limit_are_not_kept() {
        // Room for one entry each: its size, then "1" and "Stop" twice, or "Stop" and
        // "success".
        let mut starts = Hooks::keeping(size_of::<(Key, usize)>() + 9);
        read(
            &mut starts,
            &[Line::Started("1", "Stop"), Line::Started("2", "Stop")],
        );
        starts.finish();
        let mut responses = Hooks::keeping(size_of::<HookRun>() + 11);
        let lines = [
            Line::Response("1", "Stop", "success"),
            Line::Response("2", "Stop", "success"),
        ];
        read(&mut responses, &lines);
        responses.finish();

        let starts = reported(starts);
        assert_eq!(starts.runs, [run(HookEvent::Stop, HookResult::NoResponse)]);
        assert!(!starts.complete);
        // The success already kept still stands.
        let responses = reported(responses);
        assert_eq!(responses.runs, [run(HookEvent::Stop, SUCCEEDED)]);
        assert!(!responses.complete);
    }
    #[test]
    fn entries_with_empty_fields_still_use_up_the_limit() {
        let mut hooks = Hooks::keeping(size_of::<HookRun>());
        for _ in 0..3 {
            hooks.started((String::new(), String::new(), String::new()));
            hooks.responded(None, "", "", None);
        }
        hooks.finish();

        let runs = reported(hooks);
        assert_eq!(runs.runs.len(), 1);
        assert!(!runs.complete);
    }
}

/// Hook events read from whole transcripts, beside the events they share a stream with.
#[cfg(test)]
mod reading {
    use std::path::Path;

    use serde_json::json;
    use uuid::Uuid;

    use crate::client::claude::fixtures::{FINISHED, SESSION, before, from, recorded_session};
    use crate::client::claude::transcript::read_text;
    use crate::client::claude::version;
    use crate::client::{Completion, HookEvent, HookEvidence, HookResult, HookRun, Reading};

    fn read(transcript: &str) -> Reading {
        read_text(
            transcript,
            Path::new("/trial/project"),
            recorded_session(),
            version::MINIMUM,
            &[],
        )
    }

    /// The recorded session, with `lines` just before its `result`.
    fn with(lines: &[serde_json::Value]) -> String {
        let added: String = lines.iter().map(|line| format!("{line}\n")).collect();
        format!(
            "{}{added}{}",
            before(FINISHED, "result"),
            from(FINISHED, "result")
        )
    }

    fn system(subtype: serde_json::Value, rest: serde_json::Value) -> serde_json::Value {
        let mut line = json!({ "type": "system", "subtype": subtype, "session_id": SESSION });
        line.as_object_mut()
            .unwrap()
            .extend(rest.as_object().unwrap().clone());
        line
    }

    fn stop_response(outcome: &str) -> serde_json::Value {
        system(
            "hook_response".into(),
            json!({ "hook_id": "2", "hook_name": "Stop", "hook_event": "Stop",
                    "stdout": "", "stderr": "", "exit_code": 0, "outcome": outcome }),
        )
    }

    fn hooks(reading: &Reading) -> (&[HookRun], bool) {
        let HookEvidence::Reported(runs) = &reading.hooks else {
            panic!("Claude Code reports hooks");
        };
        (&runs.runs, runs.complete)
    }

    fn finished(reading: &Reading) -> bool {
        matches!(reading.completion, Ok(Completion::Finished))
    }

    const SESSION_START: HookRun = HookRun {
        event: HookEvent::SessionStart,
        result: HookResult::Succeeded { exit_code: Some(0) },
    };

    const STOP: HookRun = HookRun {
        event: HookEvent::Stop,
        result: HookResult::Succeeded { exit_code: Some(0) },
    };

    #[test]
    fn the_recorded_session_start_hook_is_a_complete_record() {
        let reading = read(FINISHED);

        assert_eq!(hooks(&reading), (&[SESSION_START][..], true));
    }

    #[test]
    fn a_stop_hook_before_the_result_is_read() {
        let reading = read(&with(&[stop_response("success")]));

        assert_eq!(hooks(&reading), (&[SESSION_START, STOP][..], true));
    }

    #[test]
    fn progress_is_skipped_whatever_session_it_names() {
        let mut unscoped = system("hook_progress".into(), json!({ "hook_id": "2" }));
        unscoped.as_object_mut().unwrap().remove("session_id");
        let mut foreign = system("hook_progress".into(), json!({ "hook_id": "3" }));
        foreign["session_id"] = Uuid::new_v4().to_string().into();

        let reading = read(&with(&[unscoped, foreign]));

        assert!(finished(&reading));
        assert_eq!(hooks(&reading), (&[SESSION_START][..], true));
    }

    #[test]
    fn progress_and_unknown_subtypes_are_skipped() {
        let progress = system(
            "hook_progress".into(),
            json!({ "hook_id": "2", "stdout": "x" }),
        );
        let odd = system(5.into(), json!({}));

        let reading = read(&with(&[progress, odd]));

        assert!(finished(&reading));
        assert_eq!(hooks(&reading), (&[SESSION_START][..], true));
    }

    #[test]
    fn an_exit_code_that_cannot_be_read_is_left_out() {
        let mut response = stop_response("success");
        response["exit_code"] = "0".into();

        let reading = read(&with(&[response]));

        let stop = HookRun {
            event: HookEvent::Stop,
            result: HookResult::Succeeded { exit_code: None },
        };
        assert_eq!(hooks(&reading), (&[SESSION_START, stop][..], true));
    }

    #[test]
    fn a_hook_line_that_cannot_be_read_damages_only_the_hook_record() {
        let mut broken = stop_response("success");
        broken.as_object_mut().unwrap().remove("outcome");

        let reading = read(&with(&[broken]));

        assert!(finished(&reading));
        // A success already read still stands beside the damage.
        assert_eq!(hooks(&reading), (&[SESSION_START][..], false));
    }

    #[test]
    fn another_sessions_hook_line_is_a_mismatch() {
        let mut foreign = stop_response("success");
        foreign["session_id"] = Uuid::new_v4().to_string().into();

        let reading = read(&with(&[foreign]));

        assert!(reading.completion.is_err());
        assert!(!hooks(&reading).1);
    }

    #[test]
    fn a_session_without_its_result_cannot_be_a_complete_record() {
        let started = system(
            "hook_started".into(),
            json!({ "hook_id": "2", "hook_name": "Stop", "hook_event": "Stop" }),
        );
        let unfinished = format!("{}{started}\n", before(FINISHED, "result"));

        let reading = read(&unfinished);

        let stop = HookRun {
            event: HookEvent::Stop,
            result: HookResult::NoResponse,
        };
        assert_eq!(hooks(&reading), (&[SESSION_START, stop][..], false));
    }
}
