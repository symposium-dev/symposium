//! The command end to end under real `srt`, with the Node stand-in for Claude Code
//! installed as npm installs it, and the `cargo-agents` built from this checkout given
//! as `--symposium-bin`: run on Linux with `cargo test -p symposium-eval -- --ignored`.

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tempfile::TempDir;

use super::{Checkout, Request, run};
use crate::evals::Evals;
use crate::run::test_support::{built_cargo_agents, checkout, executable};
use crate::task::TaskName;

const STAND_IN: &str = include_str!("../run/trial/claude-stand-in.js");

/// A production-shaped command invocation with a deterministic Claude Code stand-in.
struct Scenario {
    _temp: TempDir,
    path: OsString,
    request: Request,
    checkout: Checkout,
}

impl Scenario {
    fn formatting_regression() -> Self {
        Self::for_task("formatting")
    }

    fn store_durability_capability() -> Self {
        Self::for_task("store-durability")
    }

    fn for_task(task: &str) -> Self {
        let temp = TempDir::new().unwrap();
        let bin = install_claude_stand_in(temp.path());
        let path = std::env::join_paths(
            std::iter::once(bin).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap();
        let request = Request {
            task: TaskName::new(task).unwrap(),
            model: "a-model".to_owned(),
            symposium_bin: Some(built_cargo_agents()),
            eval_root: Some(temp.path().join("eval")),
        };
        let checkout = Checkout {
            root: checkout(),
            evals: Evals::in_repository(),
            results: temp.path().join("results"),
        };
        Self {
            _temp: temp,
            path,
            request,
            checkout,
        }
    }

    fn run(&self) {
        run(&self.request, &self.checkout, |name| match name {
            "PATH" => Some(self.path.clone()),
            "ANTHROPIC_API_KEY" => Some(OsString::from("sk-test")),
            _ => None,
        })
        .unwrap();
    }

    fn kept_trials(&self) -> Vec<KeptTrial> {
        let batches = self
            .checkout
            .results
            .join(self.request.task.as_str())
            .join("claude");
        let batch = only_entry(&batches, "batch");
        let mut trials: Vec<_> = fs::read_dir(batch)
            .unwrap()
            .map(|entry| KeptTrial::read(entry.unwrap().path()))
            .collect();
        trials.sort_by(|left, right| left.name().cmp(right.name()));
        trials
    }

    fn only_kept_trial(&self) -> KeptTrial {
        let mut trials = self.kept_trials();
        if trials.len() != 1 {
            panic!("expected one trial, found {}", trials.len());
        }
        trials.pop().unwrap()
    }
}

struct KeptTrial {
    root: PathBuf,
    result: Value,
}

impl KeptTrial {
    fn read(root: PathBuf) -> Self {
        let result = serde_json::from_slice(&fs::read(root.join("result.json")).unwrap()).unwrap();
        Self { root, result }
    }

    fn name(&self) -> &str {
        self.root.file_name().unwrap().to_str().unwrap()
    }

    fn outcome(&self) -> &str {
        self.result["outcome"]["result"].as_str().unwrap()
    }

    fn symposium_revision(&self) -> &Value {
        &self.result["symposium"]["revision"]
    }

    fn toolchain(&self) -> &str {
        self.result["toolchain"].as_str().unwrap()
    }

    fn has_transcript(&self) -> bool {
        self.root.join("transcript.jsonl").is_file()
    }

    fn round(&self) -> u64 {
        self.result["trial"].as_u64().unwrap()
    }
}

fn install_claude_stand_in(root: &Path) -> PathBuf {
    let package = root.join("lib/node_modules/claude-stand-in");
    fs::create_dir_all(&package).unwrap();
    fs::write(package.join("package.json"), "{}").unwrap();
    executable(&package.join("cli.js"), STAND_IN);
    let bin = root.join("bin");
    fs::create_dir(&bin).unwrap();
    symlink(package.join("cli.js"), bin.join("claude")).unwrap();
    bin
}

fn only_entry(directory: &Path, kind: &str) -> PathBuf {
    let entries: Vec<_> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    let [entry] = &entries[..] else {
        panic!(
            "expected one {kind} in `{}`, found {entries:?}",
            directory.display()
        );
    };
    entry.clone()
}

#[test]
#[ignore = "needs node, srt, and bubblewrap installed, and builds cargo-agents"]
fn a_regression_task_runs_one_symposium_trial_and_keeps_it() {
    let scenario = Scenario::formatting_regression();

    scenario.run();

    let trial = scenario.only_kept_trial();
    assert_eq!(trial.name(), "trial-1-with-symposium");
    assert_eq!(trial.outcome(), "succeeded");
    assert_eq!(trial.symposium_revision(), &Value::Null);
    assert!(trial.toolchain().starts_with("rustc "));
    assert!(trial.has_transcript());
}

#[test]
#[ignore = "needs node, srt, and bubblewrap installed, and builds cargo-agents"]
fn a_capability_task_keeps_both_arms_in_the_same_round() {
    let scenario = Scenario::store_durability_capability();

    scenario.run();

    let trials = scenario.kept_trials();
    assert_eq!(
        trials.iter().map(KeptTrial::name).collect::<Vec<_>>(),
        ["trial-1-with-symposium", "trial-1-without-symposium"]
    );
    assert!(trials.iter().all(|trial| trial.round() == 1));
}
