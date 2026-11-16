//! Running a trial's commands under `srt` (RFD reference, "Enforced: `srt`").
//!
//! [`Sandboxed`] is the [`Backend`] for one trial: it wraps each command as
//! `srt --settings <run>/srt.json -- <program> <args>` and hands it to [`Local`], which
//! keeps the deadline, kills the whole tree, and logs the output. `srt` runs the command
//! through `bash -c`, quoting each argument, and passes it `srt`'s own environment, so the
//! trial's environment is set on `srt`. `srt` is a Node program, so [`Srt`] names the
//! Node that runs it rather than relying on `env node` finding one on the trial's `PATH`.
//!
//! An outer `bwrap` gives each trial its own `/tmp/claude`. `srt` lets every sandbox write
//! that directory whatever the policy says, and a `denyRead` of it does not hide what is
//! already there (sandbox-runtime issues 654 and 611), so without this, one trial could
//! read what another left there. The outer namespace creates `/tmp/claude` if needed,
//! binds the trial's `tmp/` over it, and changes nothing else; `srt` then builds its own
//! sandbox inside, with its own PID namespace (`--unshare-pid --die-with-parent`), so
//! killing the tree reaches every process, including one that called `setsid`.
//!
//! Enforced mode is Linux only. On macOS trials would share `/tmp/claude`, and a process
//! that leaves the process group outlives the deadline, since there is no PID namespace.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{Policy, RunError, Trial, absolute, succeeded};
use crate::grade::ProcessError;
use crate::grade::process::{Backend, Finished, Invocation, Local};

/// The `srt` release whose behavior the policy and these wrappers were checked against.
pub(crate) const SRT_VERSION: &str = "0.0.81";

/// The oldest Node that release runs on.
const NODE_MINIMUM: (u32, u32) = (22, 12);

/// How long each preflight probe may take.
const PROBE_DEADLINE: Duration = Duration::from_secs(30);

/// Where `srt` lets every sandbox write.
const SRT_SHARED_TMP: &str = "/tmp/claude";

/// The `srt` installation trials run under, on Linux.
#[derive(Debug, Clone)]
pub(crate) struct Srt {
    /// Node 22.12 or later, which runs `srt`.
    node: PathBuf,
    /// `srt`'s entry point, `dist/cli.js` in its npm package.
    cli: PathBuf,
    /// The `bwrap` that gives each trial its own `/tmp/claude`.
    bwrap: PathBuf,
}

/// Commands run under `srt` with one policy, and a `/tmp/claude` of their own.
#[derive(Debug, Clone)]
pub(crate) struct Sandboxed<'a> {
    srt: &'a Srt,
    policy: PathBuf,
    tmp: PathBuf,
}

impl Srt {
    /// # Errors
    ///
    /// When a path is relative.
    pub(crate) fn new(node: &Path, cli: &Path, bwrap: &Path) -> Result<Self, RunError> {
        Ok(Self {
            node: absolute(node)?.to_owned(),
            cli: absolute(cli)?.to_owned(),
            bwrap: absolute(bwrap)?.to_owned(),
        })
    }

    /// Checks, before a batch starts, that every trial can launch: Node is recent enough,
    /// `srt` is the pinned [`SRT_VERSION`], and a command runs through the whole stack
    /// (outer `bwrap`, Node, `srt`, a policy file), which also proves `srt`'s own
    /// dependencies and the nested namespace without this code knowing what they are.
    /// A failure here is the machine's, never a trial's.
    ///
    /// # Errors
    ///
    /// When a tool cannot be started or supervised, does not answer in time, exits
    /// unsuccessfully, or is the wrong version.
    pub(crate) fn preflight(&self, logs: &Path) -> Result<(), RunError> {
        let found = Self::probe("node", &self.node, &["--version"], logs)?;
        if !node_is_supported(&found) {
            return Err(RunError::NodeTooOld { found });
        }
        let cli = self.cli.as_os_str().to_owned();
        let found = Self::probe(
            "srt",
            &self.node,
            &[cli.as_os_str(), "--version".as_ref()],
            logs,
        )?;
        if found != SRT_VERSION {
            return Err(RunError::SrtVersion { found });
        }
        self.check_sandbox(logs)
    }

    /// Runs `true` the way a trial's commands run, under a policy that only lets it write
    /// its own scratch directory.
    fn check_sandbox(&self, logs: &Path) -> Result<(), RunError> {
        let scratch = absolute(logs)?.join("preflight-sandbox");
        let tmp = scratch.join("tmp");
        fs::create_dir_all(&tmp).map_err(|source| RunError::Create {
            path: tmp.clone(),
            source,
        })?;
        let policy_file = scratch.join("srt.json");
        Policy::for_preflight(&tmp).write(&policy_file)?;
        let invocation = Invocation {
            program: "true".into(),
            args: Vec::new(),
            dir: tmp.clone(),
            env: vec![("PATH".into(), "/usr/bin:/bin".into())],
            log_name: "preflight-sandbox".to_owned(),
        };
        let wrapped = self.wrap(&policy_file, &tmp, &invocation);
        Self::check("srt sandbox", &wrapped, logs).map(drop)
    }

    /// Runs `program args` outside any sandbox and returns its trimmed output.
    fn probe<S: AsRef<std::ffi::OsStr>>(
        tool: &'static str,
        program: &Path,
        args: &[S],
        logs: &Path,
    ) -> Result<String, RunError> {
        let invocation = Invocation {
            program: program.into(),
            args: args.iter().map(|arg| arg.as_ref().to_owned()).collect(),
            dir: logs.to_owned(),
            env: vec![("PATH".into(), "/usr/bin:/bin".into())],
            log_name: format!("preflight-{tool}"),
        };
        Self::check(tool, &invocation, logs)
    }

    /// Runs `invocation` for preflight and returns its trimmed output.
    fn check(tool: &'static str, invocation: &Invocation, logs: &Path) -> Result<String, RunError> {
        let finished = succeeded(tool, Local.run(invocation, PROBE_DEADLINE, logs))?;
        Ok(finished.stdout.tail().trim().to_owned())
    }

    /// Runs `trial`'s commands under its policy.
    ///
    /// # Errors
    ///
    /// When the trial's policy has not been written: a trial never runs without one.
    pub(crate) fn for_trial(&self, trial: &Trial) -> Result<Sandboxed<'_>, RunError> {
        self.sandboxed(trial.policy_file(), trial.tmp())
    }

    /// Runs grading `trial`'s submission under its grading policy.
    ///
    /// # Errors
    ///
    /// When the grading policy has not been written: grading never runs without one.
    pub(crate) fn for_grading(&self, trial: &Trial) -> Result<Sandboxed<'_>, RunError> {
        self.sandboxed(trial.grading_policy_file(), trial.grading_tmp())
    }

    fn sandboxed(&self, policy: PathBuf, tmp: PathBuf) -> Result<Sandboxed<'_>, RunError> {
        if !policy.is_file() {
            return Err(RunError::NoPolicy(policy));
        }
        // Only on Linux does `srt` bridge its proxy through Unix sockets.
        if cfg!(target_os = "linux") && tmp.as_os_str().len() + SRT_SOCKET_NAME > MAX_SOCKET_PATH {
            return Err(RunError::TmpTooLong(tmp));
        }
        Ok(Sandboxed {
            srt: self,
            policy,
            tmp,
        })
    }
}

/// The longest Unix socket path Linux allows, in bytes, its terminating NUL left out.
const MAX_SOCKET_PATH: usize = 107;

/// What `srt` 0.0.81 adds to its `TMPDIR` for its proxy's sockets:
/// `/claude-socks-<16 hex>.sock` (`linux-sandbox-utils.js`, `initializeLinuxNetworkBridge`).
const SRT_SOCKET_NAME: usize = "/claude-socks-0123456789abcdef.sock".len();

/// Whether `node --version` output, such as `v22.12.0`, is at least [`NODE_MINIMUM`].
fn node_is_supported(version: &str) -> bool {
    let mut parts = version.trim_start_matches('v').split('.');
    let mut number = || parts.next().and_then(|part| part.parse::<u32>().ok());
    match (number(), number()) {
        (Some(major), Some(minor)) => (major, minor) >= NODE_MINIMUM,
        _ => false,
    }
}

impl Srt {
    /// `invocation`, run by `srt` under `policy` inside the outer `bwrap`, which binds
    /// `tmp` over `/tmp/claude`.
    fn wrap(&self, policy: &Path, tmp: &Path, invocation: &Invocation) -> Invocation {
        let mut args: Vec<OsString> = vec![
            "--dev-bind".into(),
            "/".into(),
            "/".into(),
            // A bind needs its destination, and `srt` may not have created it yet.
            "--dir".into(),
            SRT_SHARED_TMP.into(),
            "--bind".into(),
            tmp.into(),
            SRT_SHARED_TMP.into(),
            "--die-with-parent".into(),
            "--".into(),
            self.node.clone().into(),
            self.cli.clone().into(),
            "--settings".into(),
            policy.into(),
            "--".into(),
            invocation.program.clone(),
        ];
        args.extend(invocation.args.iter().cloned());
        Invocation {
            program: self.bwrap.clone().into(),
            args,
            dir: invocation.dir.clone(),
            env: invocation.env.clone(),
            log_name: invocation.log_name.clone(),
        }
    }
}

impl Sandboxed<'_> {
    /// `invocation`, run by `srt` under the policy, with its own `/tmp/claude`, and the
    /// sandbox's directory as `TMPDIR`: `srt` makes its proxy's sockets there, so its
    /// path must stay short.
    fn wrap(&self, invocation: &Invocation) -> Invocation {
        let mut invocation = invocation.clone();
        invocation.env.retain(|(name, _)| name != "TMPDIR");
        invocation
            .env
            .push(("TMPDIR".into(), self.tmp.clone().into()));
        self.srt.wrap(&self.policy, &self.tmp, &invocation)
    }
}

impl Backend for Sandboxed<'_> {
    fn run(
        &self,
        invocation: &Invocation,
        deadline: Duration,
        logs: &Path,
    ) -> Result<Finished, ProcessError> {
        Local.run(&self.wrap(invocation), deadline, logs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::Configuration;
    use crate::run::policy::Policy;
    use crate::run::test_support::{Host, api_key, trial};

    fn probe(trial: &Trial) -> Invocation {
        Invocation {
            program: "sh".into(),
            args: vec!["-c".into(), "echo hello".into()],
            dir: trial.project(),
            env: vec![("HOME".into(), trial.home().into())],
            log_name: "probe".to_owned(),
        }
    }

    fn written_trial(host: &Host) -> Trial {
        let trial = trial(&host.batch(), 1, Configuration::Baseline);
        let policy =
            Policy::for_trial(&trial, &host.machine(), "api.example.com", &api_key()).unwrap();
        trial.write_policy(&policy).unwrap();
        trial
    }

    fn srt(host: &Host) -> Srt {
        Srt::new(
            &host.join("bin/node"),
            &host.join("srt/dist/cli.js"),
            &host.join("bin/bwrap"),
        )
        .unwrap()
    }

    #[test]
    fn a_sandboxed_command_has_the_sandboxs_tmpdir() {
        let host = Host::new();
        let trial = written_trial(&host);
        let srt = srt(&host);
        let mut invocation = probe(&trial);
        invocation.env.push(("TMPDIR".into(), "/elsewhere".into()));

        let wrapped = srt.for_trial(&trial).unwrap().wrap(&invocation);

        let tmpdirs: Vec<_> = wrapped
            .env
            .iter()
            .filter(|(name, _)| name == "TMPDIR")
            .map(|(_, value)| PathBuf::from(value))
            .collect();
        assert_eq!(tmpdirs, [trial.tmp()]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_tmp_too_long_for_srts_sockets_is_refused() {
        let host = Host::new();
        let trial = written_trial(&host);
        let srt = srt(&host);
        let fits = PathBuf::from("/").join("t".repeat(MAX_SOCKET_PATH - SRT_SOCKET_NAME - 1));
        let long = fits.join("x");

        assert!(srt.sandboxed(trial.policy_file(), fits).is_ok());
        let result = srt.sandboxed(trial.policy_file(), long);
        assert!(matches!(result, Err(RunError::TmpTooLong(_))), "{result:?}");
    }

    #[test]
    fn a_trial_without_a_policy_never_runs() {
        let host = Host::new();
        let trial = trial(&host.batch(), 1, Configuration::Baseline);
        let srt = srt(&host);

        let result = srt.for_trial(&trial);

        assert!(matches!(result, Err(RunError::NoPolicy(_))), "{result:?}");
    }

    #[test]
    fn srt_runs_inside_a_namespace_with_the_trials_own_tmp() {
        let host = Host::new();
        let trial = written_trial(&host);
        let srt = srt(&host);

        let wrapped = srt.for_trial(&trial).unwrap().wrap(&probe(&trial));

        let expected: Vec<OsString> = vec![
            "--dev-bind".into(),
            "/".into(),
            "/".into(),
            "--dir".into(),
            "/tmp/claude".into(),
            "--bind".into(),
            trial.tmp().into(),
            "/tmp/claude".into(),
            "--die-with-parent".into(),
            "--".into(),
            host.join("bin/node").into(),
            host.join("srt/dist/cli.js").into(),
            "--settings".into(),
            trial.policy_file().into(),
            "--".into(),
            "sh".into(),
            "-c".into(),
            "echo hello".into(),
        ];
        assert_eq!(wrapped.program, host.join("bin/bwrap").as_os_str());
        assert_eq!(wrapped.args, expected);
        assert_eq!(wrapped.dir, trial.project());
        let mut env = probe(&trial).env;
        env.push(("TMPDIR".into(), trial.tmp().into()));
        assert_eq!(wrapped.env, env);
    }

    #[test]
    fn node_srt_and_bwrap_must_be_absolute() {
        let host = Host::new();
        let [node, cli, bwrap] = ["node", "cli.js", "bwrap"].map(|name| host.join(name));
        let relative = Path::new("relative");

        for result in [
            Srt::new(relative, &cli, &bwrap),
            Srt::new(&node, relative, &bwrap),
            Srt::new(&node, &cli, relative),
        ] {
            assert!(
                matches!(result, Err(RunError::NotAbsolute(_))),
                "{result:?}"
            );
        }
    }

    #[test]
    fn node_must_be_at_least_22_12() {
        for supported in ["v22.12.0", "v22.23.4", "v23.0.0", "v24.1.0"] {
            assert!(node_is_supported(supported), "{supported}");
        }
        for unsupported in ["v22.11.9", "v20.19.0", "v18.20.0", "", "node", "v22"] {
            assert!(!node_is_supported(unsupported), "{unsupported:?}");
        }
    }

    /// An absolute path to `name`, which [`Srt::new`] demands, so `PATH` lookup is
    /// not available. macOS keeps these in `/usr/bin` and has no `/bin/false`.
    #[cfg(unix)]
    fn system_tool(name: &str) -> PathBuf {
        ["/usr/bin", "/bin"]
            .into_iter()
            .map(|dir| Path::new(dir).join(name))
            .find(|path| path.is_file())
            .unwrap_or_else(|| panic!("no `{name}` in /usr/bin or /bin"))
    }

    /// A tool that cannot start is reported by name, not as a version mismatch.
    #[cfg(unix)]
    #[test]
    fn a_tool_that_fails_is_named() {
        let host = Host::new();
        let srt = Srt::new(
            &system_tool("false"),
            &host.join("missing-cli.js"),
            &system_tool("true"),
        )
        .unwrap();

        let result = srt.preflight(&host.join("checkout"));

        assert!(
            matches!(result, Err(RunError::ToolFailed { tool: "node", .. })),
            "{result:?}"
        );
    }

    /// Tests that run real `node`, `srt`, and `bwrap`: run them on Linux with
    /// `cargo test -p symposium-eval -- --ignored`.
    #[cfg(target_os = "linux")]
    mod real {
        use std::fs;
        use std::thread;

        use super::*;
        use crate::run::env::ApiKey;
        use crate::run::test_support::RealSrt;

        #[test]
        #[ignore = "needs node, srt, and bubblewrap installed"]
        fn trials_do_not_share_temporary_files() {
            let real = RealSrt::new(api_key());
            let marker = format!("left-by-{}", std::process::id());
            let minute = Duration::from_secs(60);

            let first = real.trial(1, Configuration::Baseline);
            let finished = real.run(
                &first,
                &format!("echo 1 > \"$TMPDIR/{marker}\" && echo 1 > /tmp/claude/{marker}-claude"),
                &[],
                minute,
            );
            assert!(finished.succeeded(), "{}", finished.stderr.tail());
            assert!(first.tmp().join(&marker).is_file());
            assert!(first.tmp().join(format!("{marker}-claude")).is_file());
            let shared = Path::new("/tmp/claude").join(format!("{marker}-claude"));
            assert!(!shared.exists());

            let second = real.trial(2, Configuration::Baseline);
            let finished = real.run(
                &second,
                &format!("test ! -e \"$TMPDIR/{marker}\" && test ! -e /tmp/claude/{marker}-claude"),
                &[],
                minute,
            );
            assert!(finished.succeeded(), "{}", finished.stderr.tail());
        }

        /// The sandbox sees a sentinel: the real key is in no environment it can read.
        #[test]
        #[ignore = "needs node, srt, and bubblewrap installed"]
        fn the_api_key_never_enters_the_sandbox() {
            let canary = format!("sk-canary-{}", std::process::id());
            let real = RealSrt::new(ApiKey::new("EXAMPLE_API_KEY", canary.clone().into()));
            let script = "printf 'key=%s\\n' \"$EXAMPLE_API_KEY\"; env; \
                          for f in /proc/[0-9]*/environ; do tr '\\0' '\\n' < \"$f\"; done; true";

            let trial = real.trial(1, Configuration::Baseline);
            let finished = real.run(&trial, script, &[], Duration::from_secs(60));

            assert!(finished.succeeded(), "{}", finished.stderr.tail());
            let stdout = fs::read_to_string(finished.stdout.log()).unwrap();
            assert!(
                !stdout.contains(&canary),
                "the real key reached the sandbox"
            );
            let masked = stdout.lines().find_map(|line| line.strip_prefix("key="));
            assert!(masked.is_some_and(|value| !value.is_empty()), "{stdout}");
        }

        /// A process that leaves the process group still dies with the trial.
        #[test]
        #[ignore = "needs node, srt, and bubblewrap installed"]
        fn a_detached_process_dies_at_the_deadline() {
            let real = RealSrt::new(api_key());
            let script = "setsid sh -c 'sleep 6; echo late > \"$TMPDIR/late\"' & \
                          echo started > \"$TMPDIR/started\"; sleep 60";

            let trial = real.trial(1, Configuration::Baseline);
            let finished = real.run(&trial, script, &[], Duration::from_secs(5));

            assert!(finished.timed_out());
            // Without this, a slow start would pass the test without testing anything.
            assert!(trial.tmp().join("started").is_file());
            thread::sleep(Duration::from_secs(8));
            assert!(!trial.tmp().join("late").exists());
        }
    }
}
