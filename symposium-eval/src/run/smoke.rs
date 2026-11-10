//! The no-model smoke test of RFD step 2, under real `srt` and the generated policy.
//!
//! Each trial builds and tests its project, then fails to read what it must not: a home
//! (including a `symposium` installed there, run by absolute path), the checkout's
//! `evals/`, and another trial. It cannot change its fixtures, its policy, the staged
//! `symposium`, a registry, or another trial, and only a Symposium trial can run the
//! staged `symposium`. Every check is one `sh` script per trial, which stops at the
//! first surprise and names it.
//!
//! The home is a decoy the test makes and adds to the denied user data roots, so the test
//! does not depend on what the machine's own home holds; files there are checked as well
//! when they exist. Paths reach the script as arguments, never spliced into its text.
//!
//! A write is checked on the host, not inside the sandbox: `srt` covers a read-denied
//! directory with an empty writable tmpfs, so a write there succeeds and is thrown away.
//!
//! Tools are staged as the runner stages them, and each trial runs them by name from its
//! `bin/`: the machine's `node`, a small npm-style package that finds the staged `node`
//! through its `env node` line, and a stand-in for `symposium`, which a baseline trial
//! must not find at all.
//!
//! FIXME: stage and launch the real clients and `symposium` with `--version`.
//!
//! Run on Linux with `cargo test -p symposium-eval -- --ignored`.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::Configuration;
use super::Trial;
use tempfile::TempDir;

use super::stage::{Audience, Tool};
use super::test_support::{RealSrt, api_key, checkout, executable, on_path};

/// Long enough for a first build of the project and its fixture.
const DEADLINE: Duration = Duration::from_secs(300);

/// The trial's checks. Each argument after a `--` marker is a path or command name the
/// marker applies to; none starts with `--`, so none is taken for a marker. A path before any marker, or after
/// one the script does not know, fails the script rather than going unchecked.
const SCRIPT: &str = r#"
fail() { echo "smoke: $1" >&2; exit 1; }
cargo build --offline --quiet || fail 'cannot build the project'
cargo test --offline --quiet || fail 'cannot test the project'
mode=
for arg; do
    case $arg in
        --*) mode=$arg; continue ;;
    esac
    case $mode in
        --unreadable) cat -- "$arg" >/dev/null 2>&1 && fail "read $arg" ;;
        --unwritable) (echo changed >> "$arg") 2>/dev/null ;;
        --runnable) "$arg" --version >/dev/null || fail "cannot run $arg" ;;
        --not-runnable) "$arg" --version >/dev/null 2>&1 && fail "ran $arg" ;;
        --command) "$arg" --version >/dev/null || fail "cannot run $arg from PATH" ;;
        --absent-command) command -v "$arg" >/dev/null && fail "found $arg on PATH" ;;
        *) fail "unknown boundary mode '$mode' for $arg" ;;
    esac
done
echo 'smoke: ok'
"#;

/// What one trial's script checks, beyond building and testing its project. Every path
/// exists on the host.
struct Boundary {
    unreadable: Vec<PathBuf>,
    /// Files the trial tries to change, which must be unchanged on the host afterwards.
    unwritable: Vec<PathBuf>,
    runnable: Vec<PathBuf>,
    not_runnable: Vec<PathBuf>,
    /// Staged tools the trial runs by name, from its `bin/`.
    commands: Vec<&'static str>,
    /// Names nothing on the trial's `PATH` may answer to.
    absent_commands: Vec<&'static str>,
}

/// Two trials of one batch, one per configuration, created after the tools are staged.
struct Smoke {
    real: RealSrt,
    staged: PathBuf,
    symposium: Trial,
    baseline: Trial,
}

#[test]
#[ignore = "needs node, srt, and bubblewrap installed"]
fn each_trial_reaches_only_its_own_files() {
    let smoke = Smoke::new();

    smoke.check(&smoke.symposium, &smoke.baseline);
    smoke.check(&smoke.baseline, &smoke.symposium);
}

impl Smoke {
    fn new() -> Self {
        let mut real = RealSrt::new(api_key());
        let sources = TempDir::new().unwrap();
        real.stage("node", &Tool::Executable(on_path("node")), Audience::Common);
        let package = sources.path().join("package");
        fs::create_dir(&package).unwrap();
        let cli = "#!/usr/bin/env node\nconsole.log('a staged package');\n";
        executable(&package.join("cli.js"), cli);
        let entry = PathBuf::from("cli.js");
        let tool = Tool::NodePackage { package, entry };
        real.stage("package", &tool, Audience::Common);
        let stand_in = sources.path().join("symposium");
        executable(&stand_in, "#!/bin/sh\necho 'symposium stand-in'\n");
        let tool = Tool::Executable(stand_in);
        let staged = real.stage("symposium", &tool, Audience::Symposium);
        let symposium = real.trial(1, Configuration::Symposium);
        let baseline = real.trial(2, Configuration::Baseline);
        Self {
            real,
            staged,
            symposium,
            baseline,
        }
    }

    /// Runs `trial`'s script, with `other` as the trial beside it, and fails the test at
    /// the first check that does not hold.
    fn check(&self, trial: &Trial, other: &Trial) {
        let boundary = self.boundary(trial, other);
        let before: Vec<Vec<u8>> = boundary.unwritable.iter().map(|path| read(path)).collect();

        let arguments = arguments(&boundary);
        let finished = self.real.run(trial, SCRIPT, &arguments, DEADLINE);

        let configuration = trial.configuration();
        let stderr = finished.stderr.tail();
        assert!(finished.succeeded(), "{configuration}: {stderr}");
        assert!(finished.stdout.tail().contains("smoke: ok"));
        for (path, before) in boundary.unwritable.iter().zip(before) {
            assert_eq!(read(path), before, "{} changed", path.display());
        }
    }

    /// What `trial` may and may not reach.
    fn boundary(&self, trial: &Trial, other: &Trial) -> Boundary {
        let is_symposium = trial.configuration() == Configuration::Symposium;
        let mut boundary = Boundary {
            unreadable: self.unreadable(other),
            unwritable: self.unwritable(trial, other),
            runnable: Vec::new(),
            not_runnable: vec![self.installed_symposium()],
            commands: vec!["node", "package"],
            absent_commands: Vec::new(),
        };
        if is_symposium {
            boundary.runnable.push(self.staged.clone());
            boundary.commands.push("symposium");
        } else {
            boundary.not_runnable.push(self.staged.clone());
            boundary.absent_commands.push("symposium");
            boundary.unreadable.push(self.registry_file());
        }
        boundary
    }

    /// Homes, the checkout's hidden files, and `other`.
    fn unreadable(&self, other: &Trial) -> Vec<PathBuf> {
        let decoy = self.real.decoy_home();
        let mut files = home_files(decoy);
        files.push(decoy.join("it's private"));
        // The machine's own home, as far as it holds these.
        if let Some(home) = std::env::home_dir() {
            files.extend(home_files(&home));
        }
        files.extend(hidden_files());
        files.extend(trial_files(other));
        files
    }

    /// `trial`'s read-only inputs, the batch's shared inputs, and `other`'s work.
    fn unwritable(&self, trial: &Trial, other: &Trial) -> Vec<PathBuf> {
        vec![
            trial.fixtures().join("fixture-store/src/lib.rs"),
            trial.policy_file(),
            self.staged.clone(),
            self.registry_file(),
            other.project().join("src/lib.rs"),
        ]
    }

    /// A plugin in the Symposium trial's registry.
    fn registry_file(&self) -> PathBuf {
        self.symposium
            .registry()
            .join("fixture-store/Symposium.toml")
    }

    /// The developer's own `symposium`, in the decoy home.
    fn installed_symposium(&self) -> PathBuf {
        self.real.decoy_home().join(".cargo/bin/symposium")
    }
}

/// A shell profile and the tools in `~/.cargo/bin`, where a developer's own
/// `symposium` is installed: those of them `home` holds.
fn home_files(home: &Path) -> Vec<PathBuf> {
    [
        ".bashrc",
        ".profile",
        ".cargo/bin/symposium",
        ".cargo/bin/cargo",
    ]
    .iter()
    .map(|relative| home.join(relative))
    .filter(|path| path.is_file())
    .collect()
}

/// A hidden check, a solution, and the checkout's own manifest.
fn hidden_files() -> Vec<PathBuf> {
    let task = checkout().join("evals/tasks/store-durability");
    vec![
        task.join("checks/settings_survive_reopen.rs"),
        task.join("solution/src/lib.rs"),
        checkout().join("Cargo.toml"),
    ]
}

/// What another trial holds.
fn trial_files(trial: &Trial) -> Vec<PathBuf> {
    vec![
        trial.project().join("src/lib.rs"),
        trial.fixtures().join("fixture-store/src/lib.rs"),
        trial.policy_file(),
    ]
}

/// `boundary` as the script's arguments.
fn arguments(boundary: &Boundary) -> Vec<OsString> {
    let sections = [
        ("--unreadable", &boundary.unreadable),
        ("--unwritable", &boundary.unwritable),
        ("--runnable", &boundary.runnable),
        ("--not-runnable", &boundary.not_runnable),
    ];
    let mut arguments = Vec::new();
    let commands = [
        ("--command", &boundary.commands),
        ("--absent-command", &boundary.absent_commands),
    ];
    for (marker, names) in commands {
        arguments.push(OsString::from(marker));
        arguments.extend(names.iter().map(OsString::from));
    }
    for (marker, paths) in sections {
        arguments.push(OsString::from(marker));
        for path in paths {
            // The script tells markers from paths by the leading `--`.
            assert!(path.is_absolute(), "`{}` is relative", path.display());
            assert!(
                path.is_file(),
                "the check needs `{}` to exist",
                path.display()
            );
            arguments.push(path.into());
        }
    }
    arguments
}

fn read(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap()
}
