//! Running one client session in a trial (RFD reference, "Running the client").
//!
//! An adapter builds its client's command line and reads its transcript once; the
//! [`Backend`] sandboxes, times out and logs the process. So a new client is one small
//! [`ClientRunner`].
//!
//! Only a client's own failures are [`Ended::Failed`]. A transcript from another session
//! or client is [`Ended::Mismatched`], which outranks a timeout and the exit status.

pub(crate) mod claude;
mod error;
pub(crate) mod paths;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::time::{Duration, Instant};

pub(crate) use error::{
    ClientError, LaunchError, Malformation, ProvenanceError, TranscriptError, Unusable,
};

use crate::grade::ProcessError;
use crate::grade::process::{Backend, Finished, Invocation};
use crate::run::Trial;
use crate::run::client::Client;
use crate::task::SkillName;

/// One client, as the runner drives it.
pub(crate) trait ClientRunner {
    /// Which client this is, the only place a trial's client is named.
    fn client(&self) -> Client;

    /// The model its sessions are asked to run, as requested: the only place a trial's
    /// model is named.
    fn model(&self) -> &str;

    /// The command that starts a new session in `trial` and runs `prompt`, with `env` as
    /// its whole environment.
    fn invocation(&self, trial: &Trial, prompt: &str, env: Vec<(OsString, OsString)>)
    -> Invocation;

    /// What the transcript of this session in `trial` says, looking for `skills`. Each
    /// adapter documents which loads it detects.
    fn read(&self, trial: &Trial, transcript: &Path, skills: &[InstalledSkill]) -> Reading;

    /// What a session's hooks are known to be when its transcript cannot be read: none
    /// seen, from a client that reports them, or unsupported.
    fn unread_hooks(&self) -> HookEvidence;

    /// What a session's usage is known to be when its transcript cannot be read.
    fn unread_usage(&self) -> Usage;
}

/// What one transcript says.
#[derive(Debug)]
pub(crate) struct Reading {
    /// Whether the adapter observed the event that introduces a session. This says
    /// only what was observed; transcript errors decide whether absence is reliable.
    pub(crate) introduction: SessionIntroduction,
    /// How the session ended, unless the transcript cannot be read, an event the adapter
    /// needs is malformed, or it is not this session's, from the client checked before
    /// launch.
    pub(crate) completion: Result<Completion, Unusable>,
    /// How each skill looked for reached the agent, in the same order.
    pub(crate) loads: Vec<Load>,
    pub(crate) hooks: HookEvidence,
    pub(crate) activity: ToolActivity,
    pub(crate) summary: Summary,
}

/// Whether a client's transcript introduced its session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionIntroduction {
    Observed,
    NotObserved,
}

/// What a transcript says about the hooks its client ran, a record the agent cannot
/// write (RFD reference, "What the evidence can and cannot show").
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HookEvidence {
    /// The client's output does not report hook runs.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "a client that reports no hooks comes with Codex")
    )]
    Unsupported,
    Reported(HookRuns),
}

/// The hook runs a transcript reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HookRuns {
    pub(crate) runs: Vec<HookRun>,
    /// The events the adapter promises to report every run of. Absence of another
    /// event never decides anything.
    pub(crate) observed: &'static [HookEvent],
    /// Every line was read, the session reached its ending, and nothing unreadable
    /// could have hidden a run of an observed event.
    pub(crate) complete: bool,
}

impl HookRuns {
    /// No runs, from a transcript that could not be read.
    pub(crate) fn unread(observed: &'static [HookEvent]) -> Self {
        Self {
            runs: Vec::new(),
            observed,
            complete: false,
        }
    }

    /// Whether every run of `event` would be reported.
    pub(crate) fn observes(&self, event: &HookEvent) -> bool {
        self.observed.contains(event)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HookRun {
    pub(crate) event: HookEvent,
    pub(crate) result: HookResult,
}

/// A hook event, by Symposium's names, which the clients share.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HookEvent {
    SessionStart,
    Stop,
    /// Kept for diagnosis; no adapter observes these.
    Other(String),
}

impl HookEvent {
    pub(crate) fn named(name: &str) -> Self {
        match name {
            "SessionStart" => Self::SessionStart,
            "Stop" => Self::Stop,
            other => Self::Other(other.to_owned()),
        }
    }
}

/// How one hook run ended, as its client reported it. Exit codes are detail: the
/// client's outcome decides, and a missing code is never taken for 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HookResult {
    Succeeded {
        exit_code: Option<i32>,
    },
    Failed {
        exit_code: Option<i32>,
    },
    Cancelled {
        exit_code: Option<i32>,
    },
    /// It started, and the transcript ended without its response.
    NoResponse,
    /// An outcome the adapter does not know, which decides nothing.
    Other {
        outcome: String,
        exit_code: Option<i32>,
    },
}

/// The file mutations and shell commands a transcript shows the agent asking for, as
/// facts to cross-check evidence with, not proof of what reached the disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolActivity {
    pub(crate) mutations: Vec<FileMutation>,
    pub(crate) commands: Vec<ShellCommand>,
    /// Nothing could have hidden a mutation or command: the record was not damaged,
    /// the session reached its ending, and every such call was read well enough to
    /// keep. Only then does the absence of one say anything.
    pub(crate) complete: bool,
}

impl ToolActivity {
    /// No activity, from a transcript that could not be read.
    pub(crate) fn unread() -> Self {
        Self {
            mutations: Vec::new(),
            commands: Vec::new(),
            complete: false,
        }
    }
}

/// A call to a tool that creates, changes or deletes the file at `path`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileMutation {
    pub(crate) path: ObservedPath,
    pub(crate) result: ToolResult,
}

/// The path a call named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ObservedPath {
    Readable {
        reported: String,
        /// Resolved by name against where the session ran (see [`paths`]); `None` when
        /// that leaves it without a root.
        resolved: Option<PathBuf>,
    },
    /// The call named no path that could be read, so it could have been any file.
    Unreadable,
}

/// A shell command the agent asked to run. Only its text is known, never its effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShellCommand {
    pub(crate) text: CappedText,
    pub(crate) result: ToolResult,
}

/// Text the agent chose, kept only up to a limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CappedText {
    /// Its first bytes, ending on a character boundary.
    pub(crate) prefix: String,
    pub(crate) original_bytes: usize,
}

impl CappedText {
    pub(crate) fn new(text: &str, limit: usize) -> Self {
        Self {
            prefix: text[..text.floor_char_boundary(limit)].to_owned(),
            original_bytes: text.len(),
        }
    }
}

/// How a tool call's result arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolResult {
    Succeeded,
    /// An error, which is also how a denial arrives.
    Failed,
    /// No result in the transcript.
    Missing,
}

/// A skill the runner looks for in a transcript, with the `SKILL.md` sync installs for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstalledSkill {
    pub(crate) name: SkillName,
    pub(crate) skill_md: PathBuf,
}

/// How a transcript shows one skill reaching the agent (RFD reference, "Skills used").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Load {
    /// A load that delivered the skill's content.
    Loaded,
    /// A load that failed or was denied.
    Attempted,
    /// Neither, in a transcript the adapter fully read.
    NotObserved,
    /// The transcript cannot say.
    Unknown,
}

/// What a transcript the adapter fully read says about how its session ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Completion {
    Finished,
    /// The client reported an error, in its own words.
    Reported(String),
    /// No completion event.
    Missing,
}

/// What a client says about its own session, besides how it ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Summary {
    /// The model the client says it ran, which resolves an alias the runner passed.
    pub(crate) model: Option<String>,
    pub(crate) usage: Usage,
    /// The agent's last message, from a session that finished.
    pub(crate) response: Option<String>,
}

impl Summary {
    /// Nothing but `usage`, from a transcript that could not be read.
    pub(crate) fn unread(usage: Usage) -> Self {
        Self {
            model: None,
            usage,
            response: None,
        }
    }
}

/// The tokens a session used, as its client reports them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Usage {
    /// The client's output never reports usage.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "a client that reports no usage comes with Codex")
    )]
    Unsupported,
    Reported(Tokens),
    /// The client reports usage, but the transcript does not show this session's.
    Unknown,
}

/// Token counts, over every model a session called.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Tokens {
    pub(crate) input: u64,
    pub(crate) output: u64,
    pub(crate) cache_read: u64,
    pub(crate) cache_write: u64,
}

impl Tokens {
    pub(crate) fn checked_add(self, other: Self) -> Option<Self> {
        Some(Self {
            input: self.input.checked_add(other.input)?,
            output: self.output.checked_add(other.output)?,
            cache_read: self.cache_read.checked_add(other.cache_read)?,
            cache_write: self.cache_write.checked_add(other.cache_write)?,
        })
    }
}

/// One session's run: where its transcript is, how it ended, and which skills reached
/// the agent.
#[derive(Debug)]
pub(crate) struct Execution {
    /// The client's standard output, under the trial's `session/`. Missing when the
    /// client never started, and not evidence when the backend lost hold of the output.
    pub(crate) transcript: PathBuf,
    /// The client's standard error, beside the transcript.
    pub(crate) stderr: PathBuf,
    pub(crate) ended: Ended,
    /// How long the backend ran the client, transcript reading left out.
    pub(crate) elapsed: Duration,
    pub(crate) loads: Vec<Load>,
    pub(crate) hooks: HookEvidence,
    pub(crate) activity: ToolActivity,
    pub(crate) summary: Summary,
}

#[derive(Debug)]
pub(crate) enum Ended {
    /// The client finished without an error. This says nothing about the code.
    Completed,
    /// The deadline passed, and the runner killed the process tree.
    TimedOut {
        /// A problem collecting the output afterwards; the timeout still stands.
        after_deadline: Option<ProcessError>,
    },
    Failed(ClientError),
    /// The client never started: a setup failure, not a trial.
    NotLaunched(LaunchError),
    /// The runner could not supervise the client once it started.
    Unsupervised(ProcessError),
    /// The client exited, but its transcript cannot say how the session ended, whatever
    /// the exit status.
    Unreadable(TranscriptError),
    /// The client started, but its transcript is not this session's, from the client
    /// checked before launch: an isolation exclusion, whether it exited or timed out.
    Mismatched(ProvenanceError),
}

/// Runs `prompt` as a new session of `runner`'s client in `trial`, through `backend`,
/// until it ends or `deadline` passes, looking for `skills` in its transcript. Never an
/// error: whatever goes wrong is how the session ended.
///
/// A timed-out transcript is still read, since it can show a mismatch or a load. One the
/// backend lost hold of is not: whatever holds the pipe may still be writing. The exit
/// status counts only once the transcript was read, so a client whose event format
/// changed is not a failed session.
pub(crate) fn execute(
    runner: &impl ClientRunner,
    backend: &impl Backend,
    trial: &Trial,
    env: Vec<(OsString, OsString)>,
    prompt: &str,
    skills: &[InstalledSkill],
    deadline: Duration,
) -> Execution {
    let invocation = runner.invocation(trial, prompt, env);
    let logs = trial.session();
    let transcript = invocation.log(&logs, "stdout");
    let stderr = invocation.log(&logs, "stderr");
    let started = Instant::now();
    let ran = backend.run(&invocation, deadline, &logs);
    let elapsed = started.elapsed();
    let unread = |ended| Execution {
        transcript: transcript.clone(),
        stderr: stderr.clone(),
        ended,
        elapsed,
        loads: vec![Load::Unknown; skills.len()],
        hooks: runner.unread_hooks(),
        activity: ToolActivity::unread(),
        summary: Summary::unread(runner.unread_usage()),
    };
    let finished = match ran {
        Ok(finished) => finished,
        Err(error @ ProcessError::Spawn { .. }) => {
            return unread(Ended::NotLaunched(error.into()));
        }
        // Every other error comes after the client started. Output the runner lost hold
        // of may still be written, so the transcript is not evidence.
        Err(error) => return unread(Ended::Unsupervised(error)),
    };
    if finished.status.is_none() && finished.after_deadline.is_some() {
        // For the same reason: the deadline stands, but the transcript may still change.
        return unread(Ended::TimedOut {
            after_deadline: finished.after_deadline,
        });
    }
    let reading = runner.read(trial, &transcript, skills);
    let ended = match (finished, reading.introduction, reading.completion) {
        (_, _, Err(Unusable::Provenance(error))) => Ended::Mismatched(error),
        (
            Finished {
                status: None,
                after_deadline,
                ..
            },
            _,
            _,
        ) => Ended::TimedOut { after_deadline },
        (_, _, Err(Unusable::Transcript(error))) => Ended::Unreadable(error),
        (
            Finished {
                status: Some(status),
                stderr,
                ..
            },
            SessionIntroduction::NotObserved,
            _,
        ) => Ended::NotLaunched(LaunchError::Exited {
            status,
            stderr: stderr.tail().trim().to_owned(),
        }),
        (
            Finished {
                status: Some(status),
                stderr,
                ..
            },
            SessionIntroduction::Observed,
            Ok(completion),
        ) => ending(status, completion, || stderr.tail().trim().to_owned()),
    };
    Execution {
        transcript,
        stderr,
        ended,
        elapsed,
        loads: reading.loads,
        hooks: reading.hooks,
        activity: reading.activity,
        summary: reading.summary,
    }
}

/// How a session that exited with `status` ended, by its transcript's `completion`. The
/// client's own error message, when it gave one, says more than its exit status.
fn ending(status: ExitStatus, completion: Completion, stderr: impl FnOnce() -> String) -> Ended {
    match (completion, status.success()) {
        (Completion::Reported(message), _) => Ended::Failed(ClientError::Reported(message)),
        (Completion::Finished, true) => Ended::Completed,
        (Completion::Missing, true) => Ended::Failed(ClientError::NoCompletion),
        (_, false) => Ended::Failed(ClientError::Exited {
            status,
            stderr: stderr(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::run::test_support::{Host, trial};
    use crate::run::{Configuration, Reach};

    /// A client whose transcript is one line: `finished`, an error message, `garbled` for
    /// one it cannot read, `foreign` for another session's, or anything else for a
    /// missing completion. A line that says `loaded` loads every skill.
    struct OneLine;

    impl ClientRunner for OneLine {
        fn client(&self) -> Client {
            Client::ClaudeCode
        }

        fn model(&self) -> &str {
            "a-model"
        }

        fn invocation(
            &self,
            trial: &Trial,
            prompt: &str,
            env: Vec<(OsString, OsString)>,
        ) -> Invocation {
            Invocation {
                program: "one-line".into(),
                args: vec![prompt.into()],
                dir: trial.project(),
                env,
                log_name: "client".to_owned(),
            }
        }

        fn unread_hooks(&self) -> HookEvidence {
            HookEvidence::Unsupported
        }

        fn unread_usage(&self) -> Usage {
            Usage::Unsupported
        }

        fn read(&self, _: &Trial, transcript: &Path, skills: &[InstalledSkill]) -> Reading {
            let text = fs::read_to_string(transcript);
            let load = match &text {
                Ok(text) if text.contains("loaded") => Load::Loaded,
                _ => Load::Unknown,
            };
            Reading {
                introduction: match text.as_deref().map(str::trim) {
                    Ok("") | Ok("foreign") | Ok("garbled") | Ok("result-without-introduction") => {
                        SessionIntroduction::NotObserved
                    }
                    _ => SessionIntroduction::Observed,
                },
                completion: text
                    .map_err(|source| {
                        TranscriptError::Read {
                            path: transcript.to_owned(),
                            source,
                        }
                        .into()
                    })
                    .and_then(|text| one_line(&text)),
                loads: vec![load; skills.len()],
                hooks: HookEvidence::Unsupported,
                activity: ToolActivity::unread(),
                summary: Summary::unread(Usage::Unsupported),
            }
        }
    }

    fn one_line(text: &str) -> Result<Completion, Unusable> {
        Ok(match text.trim() {
            "finished" => Completion::Finished,
            line if line.starts_with("error: ") => {
                Completion::Reported(line.trim_start_matches("error: ").to_owned())
            }
            "garbled" => {
                return Err(TranscriptError::Malformed {
                    line: 1,
                    problem: Malformation::Syntax { column: 1 },
                }
                .into());
            }
            "foreign" => {
                return Err(ProvenanceError::Session {
                    requested: "this-session".to_owned(),
                    found: "another-session".to_owned(),
                }
                .into());
            }
            "result-without-introduction" => Completion::Missing,
            _ => Completion::Missing,
        })
    }

    /// Writes `transcript` where a real backend writes standard output, then ends as
    /// `status` says; `None` for the deadline, after which the output may stay open.
    struct Replay {
        transcript: &'static str,
        status: Option<i32>,
        output_stayed_open: bool,
    }

    impl Backend for Replay {
        fn run(
            &self,
            invocation: &Invocation,
            _deadline: Duration,
            logs: &Path,
        ) -> Result<Finished, ProcessError> {
            fs::write(invocation.log(logs, "stdout"), self.transcript).unwrap();
            let mut finished = Finished::never_started();
            finished.status = self.status.map(exit_status);
            finished.after_deadline = self
                .output_stayed_open
                .then_some(ProcessError::OutputNotClosed);
            Ok(finished)
        }
    }

    /// Exits with its status without writing a transcript.
    struct Silent(i32);

    impl Backend for Silent {
        fn run(&self, _: &Invocation, _: Duration, _: &Path) -> Result<Finished, ProcessError> {
            let mut finished = Finished::never_started();
            finished.status = Some(exit_status(self.0));
            Ok(finished)
        }
    }

    /// Writes `transcript`, then loses hold of the client.
    struct LostHold(&'static str);

    impl Backend for LostHold {
        fn run(
            &self,
            invocation: &Invocation,
            _: Duration,
            logs: &Path,
        ) -> Result<Finished, ProcessError> {
            fs::write(invocation.log(logs, "stdout"), self.0).unwrap();
            Err(ProcessError::OutputNotClosed)
        }
    }

    /// Fails with the error its function makes.
    struct Failing(fn() -> ProcessError);

    impl Backend for Failing {
        fn run(&self, _: &Invocation, _: Duration, _: &Path) -> Result<Finished, ProcessError> {
            Err(self.0())
        }
    }

    #[cfg(unix)]
    fn exit_status(code: i32) -> ExitStatus {
        std::os::unix::process::ExitStatusExt::from_raw(code << 8)
    }

    #[cfg(windows)]
    fn exit_status(code: i32) -> ExitStatus {
        std::os::windows::process::ExitStatusExt::from_raw(code as u32)
    }

    /// The host comes back too: dropping it removes the trial.
    fn run(backend: &impl Backend) -> (Host, Trial, Execution) {
        let host = Host::new();
        let trial = trial(&host.batch(), 1, Configuration::Baseline);
        let execution = execute(
            &OneLine,
            backend,
            &trial,
            Vec::new(),
            "do the task",
            &[greeting()],
            Duration::from_secs(60),
        );
        (host, trial, execution)
    }

    fn greeting() -> InstalledSkill {
        InstalledSkill {
            name: SkillName::new("fixture-greeting").unwrap(),
            skill_md: PathBuf::from("/trial/project/.claude/skills/fixture-greeting/SKILL.md"),
        }
    }

    fn replay(transcript: &'static str, status: Option<i32>) -> Ended {
        run(&Replay {
            transcript,
            status,
            output_stayed_open: false,
        })
        .2
        .ended
    }

    /// Runs for its duration, then writes a finished transcript.
    struct Slow(Duration);

    impl Backend for Slow {
        fn run(
            &self,
            invocation: &Invocation,
            _: Duration,
            logs: &Path,
        ) -> Result<Finished, ProcessError> {
            std::thread::sleep(self.0);
            Replay {
                transcript: "finished\n",
                status: Some(0),
                output_stayed_open: false,
            }
            .run(invocation, Duration::ZERO, logs)
        }
    }

    /// [`OneLine`], taking its duration to read the transcript.
    struct SlowReader(Duration);

    impl ClientRunner for SlowReader {
        fn client(&self) -> Client {
            OneLine.client()
        }

        fn model(&self) -> &str {
            "a-model"
        }

        fn invocation(
            &self,
            trial: &Trial,
            prompt: &str,
            env: Vec<(OsString, OsString)>,
        ) -> Invocation {
            OneLine.invocation(trial, prompt, env)
        }

        fn unread_hooks(&self) -> HookEvidence {
            OneLine.unread_hooks()
        }

        fn unread_usage(&self) -> Usage {
            OneLine.unread_usage()
        }

        fn read(&self, trial: &Trial, transcript: &Path, skills: &[InstalledSkill]) -> Reading {
            std::thread::sleep(self.0);
            OneLine.read(trial, transcript, skills)
        }
    }

    #[test]
    fn a_sessions_time_is_the_clients_and_not_its_reading() {
        let host = Host::new();
        let trial = trial(&host.batch(), 1, Configuration::Baseline);
        let (running, reading) = (Duration::from_millis(100), Duration::from_millis(100));

        let started = Instant::now();
        let execution = execute(
            &SlowReader(reading),
            &Slow(running),
            &trial,
            Vec::new(),
            "do the task",
            &[greeting()],
            Duration::from_secs(60),
        );
        let total = started.elapsed();

        assert!(matches!(execution.ended, Ended::Completed), "{execution:?}");
        assert!(execution.elapsed >= running, "{:?}", execution.elapsed);
        assert!(
            total.saturating_sub(execution.elapsed) >= reading,
            "{total:?}"
        );
    }

    #[test]
    fn a_finished_session_is_completed_and_its_transcript_kept() {
        let (_host, trial, execution) = run(&Replay {
            transcript: "finished\n",
            status: Some(0),
            output_stayed_open: false,
        });

        assert!(matches!(execution.ended, Ended::Completed), "{execution:?}");
        assert!(execution.transcript.starts_with(trial.session()));
        assert_eq!(
            fs::read_to_string(&execution.transcript).unwrap(),
            "finished\n"
        );
    }

    #[test]
    fn exiting_0_without_a_completion_event_is_a_client_error() {
        let ended = replay("thinking\n", Some(0));

        assert!(
            matches!(ended, Ended::Failed(ClientError::NoCompletion)),
            "{ended:?}"
        );
    }

    #[test]
    fn a_reported_error_is_a_client_error_whatever_the_exit() {
        for status in [0, 1] {
            let ended = replay("error: rate limited\n", Some(status));

            assert!(
                matches!(&ended, Ended::Failed(ClientError::Reported(message)) if message == "rate limited"),
                "{ended:?}"
            );
        }
    }

    #[test]
    fn a_failed_exit_is_a_client_error_even_after_a_completion_event() {
        let ended = replay("finished\n", Some(1));

        assert!(
            matches!(ended, Ended::Failed(ClientError::Exited { .. })),
            "{ended:?}"
        );
    }

    #[test]
    fn the_deadline_is_a_timeout_and_the_transcript_is_still_kept() {
        let (_host, _, execution) = run(&Replay {
            transcript: "thinking\n",
            status: None,
            output_stayed_open: false,
        });

        assert!(
            matches!(
                execution.ended,
                Ended::TimedOut {
                    after_deadline: None
                }
            ),
            "{execution:?}"
        );
        assert!(execution.transcript.is_file());
    }

    #[test]
    fn a_collection_problem_after_the_deadline_is_kept_beside_the_timeout() {
        let (_host, _, execution) = run(&Replay {
            transcript: "thinking\n",
            status: None,
            output_stayed_open: true,
        });

        assert!(
            matches!(
                execution.ended,
                Ended::TimedOut {
                    after_deadline: Some(ProcessError::OutputNotClosed)
                }
            ),
            "{execution:?}"
        );
    }

    #[test]
    fn a_client_that_never_started_is_not_launched() {
        let (_host, _, execution) = run(&Failing(|| ProcessError::Spawn {
            program: "one-line".into(),
            source: std::io::ErrorKind::NotFound.into(),
        }));

        assert!(
            matches!(
                execution.ended,
                Ended::NotLaunched(LaunchError::Process(ProcessError::Spawn { .. }))
            ),
            "{execution:?}"
        );
        // No transcript was read, but the client still cannot report hooks.
        assert_eq!(execution.hooks, HookEvidence::Unsupported);
    }

    #[test]
    fn a_clean_exit_before_the_session_introduction_is_not_launched() {
        for status in [0, 126, 127] {
            let ended = replay("", Some(status));

            assert!(
                matches!(ended, Ended::NotLaunched(LaunchError::Exited { .. })),
                "{ended:?}"
            );
        }
    }

    #[test]
    fn a_matching_result_without_an_introduction_is_not_launched() {
        let ended = replay("result-without-introduction\n", Some(0));

        assert!(
            matches!(ended, Ended::NotLaunched(LaunchError::Exited { .. })),
            "{ended:?}"
        );
    }

    #[test]
    fn an_exit_after_the_session_introduction_is_a_session_failure() {
        for status in [126, 127] {
            let ended = replay("finished\n", Some(status));

            assert!(
                matches!(ended, Ended::Failed(ClientError::Exited { .. })),
                "{ended:?}"
            );
        }
    }

    #[test]
    fn losing_hold_of_a_started_client_is_the_runners_problem() {
        let (_host, _, execution) = run(&Failing(|| ProcessError::OutputNotClosed));

        assert!(
            matches!(
                execution.ended,
                Ended::Unsupervised(ProcessError::OutputNotClosed)
            ),
            "{execution:?}"
        );
    }

    #[test]
    fn a_malformed_transcript_is_unreadable_whatever_the_exit() {
        for status in [0, 1] {
            let ended = replay("garbled\n", Some(status));

            assert!(
                matches!(
                    ended,
                    Ended::Unreadable(TranscriptError::Malformed { line: 1, .. })
                ),
                "{ended:?}"
            );
        }
    }

    #[test]
    fn foreign_evidence_without_an_introduction_is_still_mismatched() {
        let ended = replay("foreign\n", Some(126));

        assert!(
            matches!(ended, Ended::Mismatched(ProvenanceError::Session { .. })),
            "{ended:?}"
        );
    }

    #[test]
    fn a_transcript_the_runner_cannot_read_is_unreadable_whatever_the_exit() {
        for status in [0, 1] {
            let (_host, _, execution) = run(&Silent(status));

            assert!(
                matches!(
                    execution.ended,
                    Ended::Unreadable(TranscriptError::Read { .. })
                ),
                "{execution:?}"
            );
        }
    }

    #[test]
    fn another_sessions_transcript_is_mismatched_whatever_the_exit() {
        for status in [0, 1] {
            let ended = replay("foreign\n", Some(status));

            assert!(
                matches!(ended, Ended::Mismatched(ProvenanceError::Session { .. })),
                "{ended:?}"
            );
        }
    }

    #[test]
    fn a_timed_out_session_still_reports_its_loads() {
        let (_host, _, execution) = run(&Replay {
            transcript: "loaded\n",
            status: None,
            output_stayed_open: false,
        });

        assert!(
            matches!(execution.ended, Ended::TimedOut { .. }),
            "{execution:?}"
        );
        assert_eq!(execution.loads, [Load::Loaded]);
    }

    #[test]
    fn a_timeout_outranks_a_malformed_transcript() {
        let ended = replay("garbled\n", None);

        assert!(matches!(ended, Ended::TimedOut { .. }), "{ended:?}");
    }

    #[test]
    fn a_mismatch_outranks_a_timeout() {
        let ended = replay("foreign\n", None);

        assert!(
            matches!(ended, Ended::Mismatched(ProvenanceError::Session { .. })),
            "{ended:?}"
        );
    }

    #[test]
    fn a_transcript_the_runner_lost_hold_of_is_not_read() {
        let (_host, _, execution) = run(&LostHold("foreign loaded\n"));

        assert!(
            matches!(
                execution.ended,
                Ended::Unsupervised(ProcessError::OutputNotClosed)
            ),
            "{execution:?}"
        );
        assert_eq!(execution.loads, [Load::Unknown]);
    }

    #[test]
    fn a_timeout_whose_output_stayed_open_is_not_read() {
        let (_host, _, execution) = run(&Replay {
            transcript: "foreign loaded\n",
            status: None,
            output_stayed_open: true,
        });

        assert!(
            matches!(
                execution.ended,
                Ended::TimedOut {
                    after_deadline: Some(ProcessError::OutputNotClosed)
                }
            ),
            "{execution:?}"
        );
        assert_eq!(execution.loads, [Load::Unknown]);
    }

    #[test]
    fn a_client_that_never_started_has_no_loads() {
        let (_host, _, execution) = run(&Failing(|| ProcessError::Spawn {
            program: "one-line".into(),
            source: std::io::ErrorKind::NotFound.into(),
        }));

        assert_eq!(execution.loads, [Load::Unknown]);
    }

    #[test]
    fn the_trial_cannot_reach_its_session() {
        let host = Host::new();
        let trial = trial(&host.batch(), 1, Configuration::Symposium);

        let reach = Reach::for_trial(&trial, &host.machine()).unwrap();

        let session = trial.session();
        assert!(session.is_dir());
        assert!(
            reach
                .allow_read
                .iter()
                .chain(&reach.allow_write)
                .all(|allowed| !session.starts_with(allowed) && !allowed.starts_with(&session)),
            "{reach:?}"
        );
        assert!(
            reach
                .deny_read
                .iter()
                .any(|denied| session.starts_with(denied))
        );
    }
}
