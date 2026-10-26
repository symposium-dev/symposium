//! Running one grading command with a deadline.
//!
//! Grading decides which command runs and what its result means; a [`Backend`] decides
//! how the process starts and stops. [`Local`] runs a command directly on this machine,
//! so it only ever runs repository-owned code; the `srt` backend will run agent
//! submissions.
//!
//! [`Local`] contains the whole process tree, so no descendant outlives the command. On
//! Unix the command leads its own process group, which is killed at the deadline and
//! again once the command exits. On Windows the command is assigned to a job object that
//! kills every process in it when its handle closes (`KILL_ON_JOB_CLOSE`), with neither
//! breakaway flag; the handle closes once the command exits, at the deadline, and on
//! every error path. Jobs nest, so this coexists with Cargo's own job. The command is
//! assigned just after it starts, so a process it starts in that first instant would
//! escape; closing that gap needs suspended process creation, which `std` does not offer.
//!
//! Once a command starts, a guard owns it: every return path, including an error or a
//! panic, stops the command and reaps it, and no wait is unbounded. Output readers are
//! joined once their pipes close. A pipe that a process outside the tree keeps open past
//! [`DRAIN_TIMEOUT`] is reported, and its reader abandoned rather than joined, since
//! joining it would wait without bound.

use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::ProcessError;

/// How much of each stream is kept in memory; the log files keep everything.
const TAIL_BYTES: usize = 64 * 1024;

/// How often a running command is checked against its deadline.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// How long a killed command may take to be reaped before a stronger kill is tried, and
/// before grading gives up on it.
const KILL_GRACE: Duration = Duration::from_secs(5);

/// How long output may stay open after the command ends.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

/// One command, as grading wants it run.
#[derive(Debug, Clone)]
pub(crate) struct Invocation {
    pub(crate) program: OsString,
    pub(crate) args: Vec<OsString>,
    pub(crate) dir: PathBuf,
    /// The command's whole environment; nothing is inherited.
    pub(crate) env: Vec<(OsString, OsString)>,
    /// Names the log files, such as `metadata` for `metadata.stdout.log`.
    pub(crate) log_name: String,
}

/// How a command ended.
#[derive(Debug)]
pub(crate) struct Finished {
    /// `None` when the deadline passed and the command was killed.
    pub(crate) status: Option<ExitStatus>,
    pub(crate) stdout: Output,
    pub(crate) stderr: Output,
}

/// The end of one output stream, and where all of it was written.
#[derive(Debug, Default)]
pub(crate) struct Output {
    tail: Vec<u8>,
    truncated: bool,
    log: PathBuf,
}

pub(crate) trait Backend {
    /// Runs `invocation` until it exits or `deadline` passes, writing its output to log
    /// files in `logs`.
    ///
    /// # Errors
    ///
    /// When the command cannot be started, stopped, or have its output collected: an
    /// infrastructure failure, never a result about the code under grading.
    fn run(
        &self,
        invocation: &Invocation,
        deadline: Duration,
        logs: &Path,
    ) -> Result<Finished, ProcessError>;
}

/// Runs commands directly on this machine. Only for repository-owned code.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Local;

/// A started command and the threads reading its output. Dropping it stops and reaps the
/// command if that has not happened yet.
struct Supervised {
    child: Child,
    /// Closing it kills the whole tree; taken once that has happened.
    #[cfg(windows)]
    job: Option<job::Job>,
    readers: Vec<JoinHandle<io::Result<Output>>>,
    drained: Receiver<()>,
    reaped: bool,
}

impl Finished {
    /// A command never started because no time was left for it; it counts as timed out.
    pub(crate) fn never_started() -> Self {
        Self {
            status: None,
            stdout: Output::default(),
            stderr: Output::default(),
        }
    }

    pub(crate) fn timed_out(&self) -> bool {
        self.status.is_none()
    }

    pub(crate) fn succeeded(&self) -> bool {
        self.status.is_some_and(|status| status.success())
    }
}

impl Output {
    /// The last bytes of the stream, lossily decoded.
    pub(crate) fn tail(&self) -> String {
        String::from_utf8_lossy(&self.tail).into_owned()
    }

    /// Whether earlier output was dropped from [`tail`](Self::tail).
    pub(crate) fn truncated(&self) -> bool {
        self.truncated
    }

    pub(crate) fn log(&self) -> &Path {
        &self.log
    }

    fn push(&mut self, bytes: &[u8]) {
        self.tail.extend_from_slice(bytes);
        if self.tail.len() > TAIL_BYTES {
            let excess = self.tail.len() - TAIL_BYTES;
            self.tail.drain(..excess);
            self.truncated = true;
        }
    }
}

impl Backend for Local {
    fn run(
        &self,
        invocation: &Invocation,
        deadline: Duration,
        logs: &Path,
    ) -> Result<Finished, ProcessError> {
        let mut command = command(invocation);
        let log = |stream: &str| logs.join(format!("{}.{stream}.log", invocation.log_name));
        let (stdout_log, stderr_log) = (log("stdout"), log("stderr"));
        let ends = Instant::now() + deadline;
        let mut process =
            Supervised::start(&mut command, &invocation.program, &stdout_log, &stderr_log)?;
        let status = process.wait_until(ends)?;
        let [stdout, stderr] = process.collect()?;
        Ok(Finished {
            status,
            stdout,
            stderr,
        })
    }
}

/// The command `invocation` describes, with only its own environment, no input, and its
/// output piped. On Unix it leads a new process group, so its tree can be killed whole.
fn command(invocation: &Invocation) -> Command {
    let mut command = Command::new(&invocation.program);
    command
        .args(&invocation.args)
        .current_dir(&invocation.dir)
        .env_clear()
        .envs(invocation.env.iter().map(|(name, value)| (name, value)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    command
}

impl Supervised {
    /// Starts the command, contains its tree, and starts one thread per output stream,
    /// each copying its pipe to a log file. The guard owns the command first, so a failure
    /// after it starts still leaves it stopped and reaped.
    fn start(
        command: &mut Command,
        program: &OsString,
        stdout_log: &Path,
        stderr_log: &Path,
    ) -> Result<Self, ProcessError> {
        let child = command.spawn().map_err(|source| ProcessError::Spawn {
            program: program.clone(),
            source,
        })?;
        let (done, drained) = mpsc::channel();
        let mut process = Self {
            child,
            #[cfg(windows)]
            job: None,
            readers: Vec::new(),
            drained,
            reaped: false,
        };
        #[cfg(windows)]
        {
            process.job =
                Some(job::Job::containing(&process.child).map_err(ProcessError::Contain)?);
        }
        if let Some(pipe) = process.child.stdout.take() {
            let reader = drain::<ChildStdout>(pipe, stdout_log, done.clone())?;
            process.readers.push(reader);
        }
        if let Some(pipe) = process.child.stderr.take() {
            let reader = drain::<ChildStderr>(pipe, stderr_log, done)?;
            process.readers.push(reader);
        }
        Ok(process)
    }

    /// Waits for the command, stopping it at `ends`. `None` means it was stopped.
    fn wait_until(&mut self, ends: Instant) -> Result<Option<ExitStatus>, ProcessError> {
        loop {
            if let Some(status) = self.child.try_wait().map_err(ProcessError::Wait)? {
                self.reaped = true;
                self.kill_descendants()?;
                return Ok(Some(status));
            }
            if Instant::now() >= ends {
                self.stop()?;
                return Ok(None);
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    /// Joins the readers once every pipe has closed.
    fn collect(mut self) -> Result<[Output; 2], ProcessError> {
        if !self.readers_finished() {
            return Err(ProcessError::OutputNotClosed);
        }
        let mut outputs = [Output::default(), Output::default()];
        for (slot, reader) in outputs.iter_mut().zip(std::mem::take(&mut self.readers)) {
            *slot = reader
                .join()
                .unwrap_or_else(|_| Err(io::Error::other("an output reader panicked")))
                .map_err(ProcessError::Output)?;
        }
        Ok(outputs)
    }

    /// Whether every reader finishes within `DRAIN_TIMEOUT`. A reader finishes when its pipe
    /// closes, so only a finished reader can be joined without blocking.
    fn readers_finished(&self) -> bool {
        let deadline = Instant::now() + DRAIN_TIMEOUT;
        (0..self.readers.len()).all(|_| {
            let left = deadline.saturating_duration_since(Instant::now());
            !matches!(
                self.drained.recv_timeout(left),
                Err(RecvTimeoutError::Timeout)
            )
        })
    }

    /// Kills the command's tree and reaps the command, falling back to killing the
    /// command alone. Every wait is bounded.
    fn stop(&mut self) -> Result<(), ProcessError> {
        let tree = self.kill_tree();
        if reap_within(&mut self.child, KILL_GRACE)? {
            self.reaped = true;
            return Ok(());
        }
        let direct = self.child.kill();
        if reap_within(&mut self.child, KILL_GRACE)? {
            self.reaped = true;
            return Ok(());
        }
        Err(ProcessError::Kill(
            direct
                .err()
                .or(tree.err())
                .unwrap_or_else(|| io::Error::other("the command survived being killed")),
        ))
    }

    #[cfg(unix)]
    fn kill_tree(&mut self) -> io::Result<()> {
        kill_group(&self.child)
    }

    #[cfg(windows)]
    fn kill_tree(&mut self) -> io::Result<()> {
        match self.job.take() {
            // Closing the job kills every process in it.
            Some(job) => {
                drop(job);
                Ok(())
            }
            None => self.child.kill(),
        }
    }

    #[cfg(not(any(unix, windows)))]
    fn kill_tree(&mut self) -> io::Result<()> {
        self.child.kill()
    }

    /// After the command exits, ends whatever it left running. An empty group is not an
    /// error, so a failure here means a descendant may still be running, which could have
    /// closed its output and so would go unnoticed otherwise.
    fn kill_descendants(&mut self) -> Result<(), ProcessError> {
        self.kill_tree().map_err(ProcessError::Kill)
    }
}

impl Drop for Supervised {
    fn drop(&mut self) {
        if !self.reaped {
            // Nothing is left to report an error to; the attempt is all that remains.
            let _ = self.stop();
        }
        if self.readers_finished() {
            for reader in self.readers.drain(..) {
                let _ = reader.join();
            }
        }
    }
}

/// Polls until the child is reaped or `timeout` passes.
fn reap_within(child: &mut Child, timeout: Duration) -> Result<bool, ProcessError> {
    let ends = Instant::now() + timeout;
    loop {
        if child.try_wait().map_err(ProcessError::Wait)?.is_some() {
            return Ok(true);
        }
        if Instant::now() >= ends {
            return Ok(false);
        }
        thread::sleep(POLL_INTERVAL);
    }
}

/// Copies a pipe to its log file, keeping the tail, then signals `done`.
fn drain<P: Read + Send + 'static>(
    pipe: P,
    log: &Path,
    done: Sender<()>,
) -> Result<JoinHandle<io::Result<Output>>, ProcessError> {
    let log_file = File::create(log).map_err(|source| ProcessError::Log {
        path: log.to_owned(),
        source,
    })?;
    let output = Output {
        log: log.to_owned(),
        ..Output::default()
    };
    thread::Builder::new()
        .name("grade-output".to_owned())
        .spawn(move || {
            let result = copy_to_log(pipe, log_file, output);
            // The receiver may have given up waiting; nothing is left to tell it.
            let _ = done.send(());
            result
        })
        .map_err(ProcessError::Thread)
}

/// Copies `pipe` to `log_file` until it closes, keeping the tail in `output`.
fn copy_to_log(mut pipe: impl Read, mut log_file: File, mut output: Output) -> io::Result<Output> {
    let mut buffer = [0; 8192];
    loop {
        match pipe.read(&mut buffer) {
            Ok(0) => return Ok(output),
            Ok(read) => {
                log_file.write_all(&buffer[..read])?;
                output.push(&buffer[..read]);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
}

#[cfg(unix)]
fn kill_group(child: &Child) -> io::Result<()> {
    let group = libc::pid_t::try_from(child.id()).map_err(io::Error::other)?;
    // SAFETY: `kill` takes no pointers. The child leads its own process group
    // (`process_group(0)`), and Linux and macOS never reuse a process group's ID while a
    // member is alive, so `-group` names only the command's tree.
    if unsafe { libc::kill(-group, libc::SIGKILL) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    // No process is left in the group.
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(error)
    }
}

#[cfg(windows)]
mod job {
    use std::io;
    use std::mem;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::process::Child;
    use std::ptr;

    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };

    /// A job object holding a command's tree. Dropping it closes the last handle, which
    /// kills every process in the job.
    #[derive(Debug)]
    pub(super) struct Job(OwnedHandle);

    impl Job {
        /// Creates a kill-on-close job and assigns `child` to it.
        pub(super) fn containing(child: &Child) -> io::Result<Self> {
            // SAFETY: null security attributes and a null name are documented as valid,
            // giving an unnamed job with a default descriptor.
            let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: `handle` is a new job handle that nothing else owns or closes.
            let job = Self(unsafe { OwnedHandle::from_raw_handle(handle) });

            // SAFETY: the structure is plain data, for which all zeros is a valid value.
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { mem::zeroed() };
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let size = u32::try_from(mem::size_of_val(&limits)).map_err(io::Error::other)?;
            // SAFETY: the pointer and size describe `limits`, which outlives the call, and
            // the information class names exactly that structure.
            let configured = unsafe {
                SetInformationJobObject(
                    job.0.as_raw_handle(),
                    JobObjectExtendedLimitInformation,
                    (&raw const limits).cast(),
                    size,
                )
            };
            if configured == 0 {
                return Err(io::Error::last_os_error());
            }

            // SAFETY: both handles are open for the call: the job's is owned by `job`, and
            // the process's by `child`, which outlives it.
            let assigned =
                unsafe { AssignProcessToJobObject(job.0.as_raw_handle(), child.as_raw_handle()) };
            if assigned == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(job)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::grade::cargo::Cargo;

    /// A crate named `fixture` whose binary runs `main`, built in `dir`, with a Cargo
    /// that runs it.
    fn binary_crate(dir: &Path, main: &str) -> Cargo {
        let project = dir.join("project");
        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(
            project.join("Cargo.toml"),
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(project.join("src/main.rs"), main).unwrap();
        let cargo = Cargo::new(project, dir);
        let build = cargo.invocation("build", &["build", "--quiet"]);
        let finished = Local.run(&build, Duration::from_secs(300), dir).unwrap();
        assert!(finished.succeeded(), "{}", finished.stderr.tail());
        cargo
    }

    #[test]
    fn output_is_logged_in_full_and_kept_as_a_tail() {
        let dir = TempDir::new().unwrap();
        let cargo = binary_crate(dir.path(), "fn main() { println!(\"hello\"); }");

        let finished = Local
            .run(
                &cargo.invocation("run", &["run", "--quiet"]),
                Duration::from_secs(60),
                dir.path(),
            )
            .unwrap();

        assert!(finished.succeeded());
        assert_eq!(finished.stdout.tail().trim(), "hello");
        assert_eq!(
            fs::read_to_string(finished.stdout.log()).unwrap(),
            finished.stdout.tail()
        );
    }

    #[test]
    fn a_command_past_its_deadline_is_killed_and_reaped() {
        let dir = TempDir::new().unwrap();
        let cargo = binary_crate(
            dir.path(),
            "fn main() { loop { std::thread::sleep(std::time::Duration::from_secs(1)); } }",
        );

        let started = Instant::now();
        let finished = Local
            .run(
                &cargo.invocation("run", &["run", "--quiet"]),
                Duration::from_secs(2),
                dir.path(),
            )
            .unwrap();

        assert!(finished.timed_out());
        assert!(started.elapsed() < Duration::from_secs(30));
    }

    #[test]
    fn a_descendant_cannot_hold_the_output_open() {
        let dir = TempDir::new().unwrap();
        // The binary starts a copy of itself that sleeps with the same stdout, then exits.
        let cargo = binary_crate(
            dir.path(),
            r#"
            fn main() {
                if std::env::args().nth(1).as_deref() == Some("descendant") {
                    std::thread::sleep(std::time::Duration::from_secs(120));
                    return;
                }
                std::process::Command::new(std::env::current_exe().unwrap())
                    .arg("descendant")
                    .spawn()
                    .unwrap();
                println!("parent done");
            }
            "#,
        );

        let started = Instant::now();
        let finished = Local
            .run(
                &cargo.invocation("run", &["run", "--quiet"]),
                Duration::from_secs(60),
                dir.path(),
            )
            .unwrap();

        assert!(finished.succeeded());
        assert_eq!(finished.stdout.tail().trim(), "parent done");
        assert!(started.elapsed() < Duration::from_secs(30));
    }

    #[test]
    fn a_descendant_that_closes_its_output_is_still_stopped() {
        let dir = TempDir::new().unwrap();
        let marker = dir.path().join("descendant-survived");
        // The descendant drops the inherited output, waits, then leaves a marker.
        let cargo = binary_crate(
            dir.path(),
            r#"
            use std::process::{Command, Stdio};
            use std::time::Duration;

            fn main() {
                let mut args = std::env::args().skip(1);
                let role = args.next().unwrap();
                let marker = args.next().unwrap();
                if role == "descendant" {
                    std::thread::sleep(Duration::from_secs(2));
                    std::fs::write(marker, "").unwrap();
                    return;
                }
                Command::new(std::env::current_exe().unwrap())
                    .args(["descendant", &marker])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap();
            }
            "#,
        );
        let marker_arg = marker.to_str().unwrap();

        let finished = Local
            .run(
                &cargo.invocation("run", &["run", "--quiet", "--", "parent", marker_arg]),
                Duration::from_secs(60),
                dir.path(),
            )
            .unwrap();
        thread::sleep(Duration::from_secs(4));

        assert!(finished.succeeded());
        assert!(!marker.exists(), "the descendant outlived the command");
    }

    #[test]
    fn exiting_at_the_deadline_is_never_an_error() {
        let dir = TempDir::new().unwrap();
        let cargo = Cargo::new(dir.path().to_owned(), dir.path());

        for _ in 0..10 {
            let finished = Local.run(
                &cargo.invocation("version", &["--version"]),
                Duration::ZERO,
                dir.path(),
            );

            assert!(finished.is_ok(), "{finished:?}");
        }
    }

    #[test]
    fn a_missing_program_is_a_spawn_error() {
        let dir = TempDir::new().unwrap();
        let invocation = Invocation {
            program: dir.path().join("absent").into(),
            args: Vec::new(),
            dir: dir.path().to_owned(),
            env: Vec::new(),
            log_name: "absent".to_owned(),
        };

        let error = Local
            .run(&invocation, Duration::from_secs(5), dir.path())
            .unwrap_err();

        assert!(matches!(error, ProcessError::Spawn { .. }), "{error:?}");
    }

    #[test]
    fn the_tail_keeps_the_end_and_records_truncation() {
        let mut output = Output::default();

        output.push(&vec![b'a'; TAIL_BYTES]);
        output.push(b"end");

        assert_eq!(output.tail.len(), TAIL_BYTES);
        assert!(output.tail().ends_with("aend"));
        assert!(output.truncated());
    }
}
