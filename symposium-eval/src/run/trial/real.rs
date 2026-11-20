//! Whole trials under real `srt`, with the `symposium` binary built from this checkout and a
//! stand-in for Claude Code, run through the real adapter: run on Linux with
//! `cargo test -p symposium-eval run::trial::real -- --ignored`.
//!
//! The stand-in leaves `double` unformatted, so a trial passes only if Symposium's
//! format-on-stop hook ran: the Symposium trial succeeds, and the baseline fails on
//! formatting.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Value, json};
use tempfile::TempDir;

use super::Driver;
use crate::client::claude::Claude;
use crate::client::claude::version;
use crate::result::{Arm, Deadlines, IsolationMode, Provenance, SymposiumBuild};
use crate::run::stage::{Audience, Tool};
use crate::run::test_support::{RealSrt, api_key, built_symposium, executable, inputs, on_path};
use crate::run::{Configuration, SRT_VERSION};

const STAND_IN: &str = include_str!("claude-stand-in.js");

#[test]
#[ignore = "needs node, srt, and bubblewrap installed, and builds symposium"]
fn symposium_stop_hook_formats_the_submission_and_the_trial_passes() {
    let scenario = FormattingScenario::new();

    let trial = scenario.run(Configuration::Symposium);
    let result = &trial.result;

    for artifact in [
        "result.json",
        "transcript.jsonl",
        "submission/src/lib.rs",
        "symposium.log",
    ] {
        assert!(trial.directory.join(artifact).is_file(), "{artifact}");
    }
    assert!(!trial.directory.join(super::retain::SKIPPED_FILE).exists());

    assert_eq!(result["code"]["cargo_fmt_check"], "pass", "{result:#}");
    assert_eq!(result["outcome"]["result"], "succeeded", "{result:#}");
    assert_eq!(
        result["code"]["edits_outside_editable"],
        json!([]),
        "{result:#}"
    );
}

#[test]
#[ignore = "needs node, srt, and bubblewrap installed, and builds symposium"]
fn baseline_trial_completes_but_fails_the_formatting_check() {
    let scenario = FormattingScenario::new();

    let trial = scenario.run(Configuration::Baseline);
    let result = &trial.result;

    assert_eq!(
        result["session"]["ended"]["status"], "completed",
        "{result:#}"
    );
    assert_eq!(result["code"]["cargo_fmt_check"], "fail", "{result:#}");
    assert_eq!(result["outcome"]["result"], "failed", "{result:#}");
    assert_eq!(result["outcome"]["failure"], "code", "{result:#}");
}

/// A fresh batch with a real sandbox and a deterministic Claude stand-in. The stand-in
/// always leaves the submission unformatted, so only Symposium's hook can format it.
struct FormattingScenario {
    real: RealSrt,
    client_version: version::Version,
    symposium_build: SymposiumBuild,
}

/// Keeps the retained artifacts alive while a test inspects its result.
struct RetainedTrial {
    _results: TempDir,
    directory: PathBuf,
    result: Value,
}

impl FormattingScenario {
    fn new() -> Self {
        let sources = TempDir::new().expect("create tool staging directory");
        let mut real = RealSrt::new(api_key());
        real.stage("node", &Tool::Executable(on_path("node")), Audience::Common);
        let package = sources.path().join("claude");
        fs::create_dir(&package).expect("create Claude stand-in package");
        executable(&package.join("cli.js"), STAND_IN);
        let tool = Tool::NodePackage {
            package,
            entry: "cli.js".into(),
        };
        let claude = real.stage_tool("claude", &tool, Audience::Common);
        let symposium = built_symposium();
        real.stage(
            "symposium",
            &Tool::Executable(symposium.clone()),
            Audience::Symposium,
        );
        let client_version = version::preflight(real.batch(), &claude, sources.path())
            .expect("probe the staged Claude stand-in version");
        let symposium_build = SymposiumBuild {
            version: "symposium (this checkout)".to_owned(),
            sha256: crate::digest::file(&symposium).expect("hash symposium"),
            revision: None,
        };
        Self {
            real,
            client_version,
            symposium_build,
        }
    }

    fn run(self, configuration: Configuration) -> RetainedTrial {
        let inputs = inputs(self.real.batch(), "formatting");
        let provenance = provenance(self.client_version);
        let driver = Driver {
            batch: self.real.batch(),
            inputs: &inputs,
            machine: self.real.machine(),
            sandbox: self.real.srt(),
            api_host: "api.example.com",
            api_key: self.real.api_key(),
            provenance: &provenance,
        };
        let arm = match configuration {
            Configuration::Symposium => Arm::Symposium(self.symposium_build),
            Configuration::Baseline => Arm::Baseline,
        };
        let runner = Claude::enforced("a-model".to_owned(), self.client_version);
        let done = driver.run(1, arm, &runner).expect("run formatting trial");
        let results = TempDir::new().expect("create retained-results directory");
        let directory = results.path().join("trial");
        done.retain(&directory).expect("retain formatting trial");
        let bytes = fs::read(directory.join("result.json")).expect("read retained result.json");
        let result = serde_json::from_slice(&bytes).expect("parse retained result.json");
        RetainedTrial {
            _results: results,
            directory,
            result,
        }
    }
}

fn provenance(client_version: version::Version) -> Provenance {
    Provenance {
        client_version: Some(client_version.to_string()),
        os: std::env::consts::OS.to_owned(),
        toolchain: None,
        isolation: IsolationMode::Enforced,
        srt_version: Some(SRT_VERSION.to_owned()),
        deadlines: Deadlines {
            setup: Duration::from_secs(120),
            agent: Duration::from_secs(300),
            grading: Duration::from_secs(600),
        },
    }
}
