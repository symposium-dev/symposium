//! A scratch machine and the trials on it, shared by `run`'s tests.

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use super::{ApiKey, Batch, Configuration, Inputs, Machine, Trial};
use crate::evals::Evals;
use crate::task::{BatchName, TaskName};

/// A canonical scratch directory holding `home/`, `checkout/evals/`, and a toolchain
/// under the home, as rustup installs one.
pub(crate) struct Host {
    _temp: TempDir,
    base: PathBuf,
}

impl Host {
    pub(crate) fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let base = fs::canonicalize(temp.path()).unwrap();
        let host = Self { _temp: temp, base };
        host.add_toolchain("home/dev/.rustup/toolchains/stable");
        fs::create_dir_all(host.join("checkout/evals")).unwrap();
        host
    }

    /// `relative`, written with `/`, under the scratch directory.
    pub(crate) fn join(&self, relative: &str) -> PathBuf {
        relative
            .split('/')
            .fold(self.base.clone(), |path, part| path.join(part))
    }

    /// Lays out the parts of a sysroot that `Machine` checks for.
    pub(crate) fn add_toolchain(&self, relative: &str) -> PathBuf {
        let toolchain = self.join(relative);
        fs::create_dir_all(toolchain.join("bin")).unwrap();
        fs::create_dir_all(toolchain.join("lib").join("rustlib")).unwrap();
        let cargo = format!("cargo{}", std::env::consts::EXE_SUFFIX);
        fs::write(toolchain.join("bin").join(cargo), "").unwrap();
        toolchain
    }

    pub(crate) fn machine(&self) -> Machine {
        Machine::for_tests(
            vec![self.join("home")],
            &self.join("checkout"),
            &self.join("home/dev/.rustup/toolchains/stable"),
        )
    }

    pub(crate) fn batch(&self) -> Batch {
        Batch::create(&self.join("eval"), &BatchName::new("batch").unwrap()).unwrap()
    }
}

/// `batch`'s copy of the repository's task `name`, taken on first use and reused after,
/// so a test can create several trials in one batch.
pub(crate) fn inputs(batch: &Batch, name: &str) -> Inputs {
    let name = TaskName::new(name).unwrap();
    if batch.inputs().exists() {
        Inputs::reopen(batch, &name).unwrap()
    } else {
        batch.snapshot(&Evals::in_repository(), &name).unwrap()
    }
}

/// `batch`'s copy of `store-durability`.
pub(crate) fn store_durability(batch: &Batch) -> Inputs {
    inputs(batch, "store-durability")
}

pub(crate) fn trial(batch: &Batch, number: usize, configuration: Configuration) -> Trial {
    batch
        .create_trial(number, configuration, &store_durability(batch))
        .unwrap()
}

/// A key for tests that never leave the sandbox's proxy.
pub(crate) fn api_key() -> ApiKey {
    ApiKey::new("EXAMPLE_API_KEY", "sk-test".into())
}

/// A batch on this machine under the installed `srt`, for the tests that run real
/// `node`, `srt`, and `bwrap`. Every trial's policy and environment use the one API key,
/// so a test cannot mask one key and pass another.
#[cfg(target_os = "linux")]
pub(crate) struct RealSrt {
    srt: super::srt::Srt,
    machine: Machine,
    batch: Batch,
    api_key: ApiKey,
    _eval_root: TempDir,
    /// A home with a shell profile and an installed `cargo-agents`, denied like every
    /// user's, so the boundary is checked against files that are surely there.
    decoy_home: TempDir,
}

#[cfg(target_os = "linux")]
impl RealSrt {
    pub(crate) fn new(api_key: ApiKey) -> Self {
        let srt = installed_srt();
        let decoy_home = decoy_home();
        let machine =
            Machine::new(vec![decoy_home.path().to_owned()], &checkout(), &sysroot()).unwrap();
        let eval_root = TempDir::new().unwrap();
        srt.preflight(eval_root.path()).unwrap();
        let batch = Batch::create(eval_root.path(), &BatchName::new("batch").unwrap()).unwrap();
        Self {
            srt,
            machine,
            batch,
            api_key,
            _eval_root: eval_root,
            decoy_home,
        }
    }

    pub(crate) fn batch(&self) -> &Batch {
        &self.batch
    }

    /// Sets up `trial`'s Symposium configuration for `client`, inside its sandbox.
    pub(crate) fn set_up_symposium(&self, trial: &Trial, client: super::client::Client) {
        self.try_set_up_symposium(trial, client, std::time::Duration::from_secs(60))
            .unwrap();
    }

    /// Setup with a caller-selected deadline, preserving the error for failure tests.
    pub(crate) fn try_set_up_symposium(
        &self,
        trial: &Trial,
        client: super::client::Client,
        deadline: std::time::Duration,
    ) -> Result<(), super::RunError> {
        let backend = self.srt.for_trial(trial).unwrap();
        let env = trial.environment(&self.machine, &self.api_key).unwrap();
        trial.set_up_symposium(client, &backend, env, deadline)
    }

    /// Runs the pre-launch checks on this machine, with the environment `trial` launches
    /// with.
    pub(crate) fn check_before_launch(
        &self,
        trial: &Trial,
        client: super::client::Client,
    ) -> Result<(), super::RunError> {
        let env = trial.environment(&self.machine, &self.api_key).unwrap();
        trial.check_before_launch(client, &env)
    }

    /// Stages `tool` as `name`; trials created afterwards link it.
    pub(crate) fn stage(
        &mut self,
        name: &str,
        tool: &super::stage::Tool,
        audience: super::stage::Audience,
    ) -> PathBuf {
        let name = crate::task::ToolName::new(name).unwrap();
        let staged = self.batch.stage(name, tool, audience).unwrap();
        staged.command().to_owned()
    }

    pub(crate) fn decoy_home(&self) -> &std::path::Path {
        self.decoy_home.path()
    }

    /// Creates a trial of `store-durability` and writes its policy.
    pub(crate) fn trial(&self, number: usize, configuration: Configuration) -> Trial {
        self.trial_of(
            &TaskName::new("store-durability").unwrap(),
            number,
            configuration,
        )
    }

    /// A trial of the repository's task `name`, with its policy written.
    pub(crate) fn trial_of(
        &self,
        name: &TaskName,
        number: usize,
        configuration: Configuration,
    ) -> Trial {
        let trial = self
            .batch
            .create_trial(number, configuration, &inputs(&self.batch, name.as_str()))
            .unwrap();
        let policy =
            super::Policy::for_trial(&trial, &self.machine, "api.example.com", &self.api_key)
                .unwrap();
        trial.write_policy(&policy).unwrap();
        trial
    }

    /// Grades `submission` as `trial` of task `name`, as a batch does: under real `srt`
    /// with the grading policy.
    pub(crate) fn grade(
        &self,
        trial: &Trial,
        name: &str,
        submission: &std::path::Path,
    ) -> crate::grade::Grading {
        let dirs = trial.prepare_grading(&self.machine).unwrap();
        let backend = self.srt.for_grading(trial).unwrap();
        let inputs = inputs(&self.batch, name);
        crate::grade::grade(
            inputs.evals(),
            inputs.task(),
            submission,
            &backend,
            &crate::grade::GradeLimits::default(),
            &dirs,
        )
    }

    /// Runs a command against a local test API, trusting only its additional test CA.
    pub(crate) fn run_with_test_api(
        &self,
        trial: &Trial,
        script: &str,
        port: u16,
        certificate: &std::path::Path,
    ) -> crate::grade::process::Finished {
        use crate::grade::process::{Backend, Invocation};
        let policy =
            super::Policy::for_trial(trial, &self.machine, "localhost", &self.api_key).unwrap();
        trial.write_policy(&policy).unwrap();
        let mut env = trial.environment(&self.machine, &self.api_key).unwrap();
        // Trust the fixture's CA in the outer proxy, without disabling TLS verification.
        env.push(("NODE_EXTRA_CA_CERTS".into(), certificate.into()));
        let invocation = Invocation {
            program: "sh".into(),
            args: vec![
                "-c".into(),
                script.into(),
                "sh".into(),
                port.to_string().into(),
            ],
            dir: trial.project(),
            env,
            log_name: "local-api".to_owned(),
        };
        self.srt
            .for_trial(trial)
            .unwrap()
            .run(
                &invocation,
                std::time::Duration::from_secs(60),
                &trial.evidence(),
            )
            .unwrap()
    }

    /// Runs `script` with `sh -c` in `trial`'s project, under its policy, with `args` as
    /// its positional parameters. They pass through `srt`, which handles them as text, so
    /// a path that is not UTF-8 cannot be one.
    pub(crate) fn run(
        &self,
        trial: &Trial,
        script: &str,
        args: &[std::ffi::OsString],
        deadline: std::time::Duration,
    ) -> crate::grade::process::Finished {
        use crate::grade::process::{Backend, Invocation};

        let invocation = Invocation {
            program: "sh".into(),
            args: ["-c".into(), script.into(), "sh".into()]
                .into_iter()
                .chain(args.iter().cloned())
                .collect(),
            dir: trial.project(),
            env: trial.environment(&self.machine, &self.api_key).unwrap(),
            log_name: "probe".to_owned(),
        };
        self.srt
            .for_trial(trial)
            .unwrap()
            .run(&invocation, deadline, &trial.evidence())
            .unwrap()
    }

    /// Runs Symposium's `SessionStart` hook for Claude Code in `trial`, as the client
    /// does when a session starts in the project.
    pub(crate) fn run_session_start(&self, trial: &Trial) -> crate::grade::process::Finished {
        let fields = serde_json::json!({ "source": "startup" });
        self.run_claude_hook(trial, "SessionStart", "session-start", fields)
    }

    /// Runs Symposium's `Stop` hook for Claude Code in `trial`, as the client does when
    /// the agent's turn ends.
    pub(crate) fn run_stop(&self, trial: &Trial) -> crate::grade::process::Finished {
        let fields = serde_json::json!({ "stop_hook_active": false });
        self.run_claude_hook(trial, "Stop", "stop", fields)
    }

    /// Runs `cargo-agents hook claude <command>` in the project with the payload Claude
    /// Code sends for `event`: what it sends every hook, and `fields`.
    fn run_claude_hook(
        &self,
        trial: &Trial,
        event: &str,
        command: &str,
        fields: serde_json::Value,
    ) -> crate::grade::process::Finished {
        let mut payload = serde_json::json!({
            "session_id": "00000000-0000-4000-8000-000000000000",
            "transcript_path": trial.session().join("transcript.jsonl"),
            "cwd": trial.project(),
            "hook_event_name": event,
        });
        payload
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        self.run(
            trial,
            &format!(r#"printf '%s' "$1" | cargo-agents hook claude {command}"#),
            &[payload.to_string().into()],
            std::time::Duration::from_secs(60),
        )
    }
}

/// The `srt` installed on this machine, with the `node` and `bwrap` it needs.
#[cfg(target_os = "linux")]
fn installed_srt() -> super::srt::Srt {
    // npm links `srt` on `PATH` to the package's `dist/cli.js`.
    let cli = fs::canonicalize(on_path("srt")).unwrap();
    super::srt::Srt::new(&on_path("node"), &cli, &on_path("bwrap")).unwrap()
}

/// The first file called `name` in a directory on `PATH`.
#[cfg(target_os = "linux")]
pub(crate) fn on_path(name: &str) -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("`{name}` is not on PATH"))
}

/// The active toolchain's sysroot, as `rustc` reports it.
#[cfg(target_os = "linux")]
fn sysroot() -> PathBuf {
    let output = std::process::Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .unwrap();
    PathBuf::from(String::from_utf8(output.stdout).unwrap().trim())
}

/// A home holding a shell profile and an installed `cargo-agents`, which a trial must
/// not read or run.
#[cfg(target_os = "linux")]
fn decoy_home() -> TempDir {
    let home = TempDir::new().unwrap();
    fs::write(home.path().join(".bashrc"), "export DECOY=1\n").unwrap();
    // A name `sh` would need quoted, which reaches the script as an argument.
    fs::write(home.path().join("it's private"), "decoy\n").unwrap();
    let installed = home.path().join(".cargo/bin/cargo-agents");
    fs::create_dir_all(installed.parent().unwrap()).unwrap();
    executable(&installed, "#!/bin/sh\necho 'installed cargo-agents'\n");
    home
}

/// Writes an executable shell script at `path`.
#[cfg(unix)]
pub(crate) fn executable(path: &std::path::Path, script: &str) {
    use std::os::unix::fs::PermissionsExt;

    fs::write(path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// The `cargo-agents` under evaluation: built from this checkout, at the path Cargo
/// reports, wherever its target directory is. Built once per test run.
#[cfg(target_os = "linux")]
pub(crate) fn built_cargo_agents() -> PathBuf {
    static BUILT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BUILT.get_or_init(build_cargo_agents).clone()
}

#[cfg(target_os = "linux")]
fn build_cargo_agents() -> PathBuf {
    let output = std::process::Command::new(env!("CARGO"))
        .args([
            "build",
            "--locked",
            "--quiet",
            "--package",
            "symposium",
            "--bin",
            "cargo-agents",
        ])
        .arg("--message-format=json-render-diagnostics")
        .current_dir(checkout())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "cannot build cargo-agents: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|message| message["target"]["name"] == "cargo-agents")
        .and_then(|message| message["executable"].as_str().map(PathBuf::from))
        .expect("Cargo reports the cargo-agents executable")
}

/// The Symposium checkout this crate is built from.
#[cfg(target_os = "linux")]
pub(crate) fn checkout() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned()
}
