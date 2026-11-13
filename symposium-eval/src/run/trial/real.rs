//! Whole trials under real `srt`, with the `cargo-agents` built from this checkout and a
//! stand-in for Claude Code, run through the real adapter: run on Linux with
//! `cargo test -p symposium-eval -- --ignored`.
//!
//! The stand-in leaves `double` unformatted, so a trial passes only if Symposium's
//! format-on-stop hook ran: the Symposium trial succeeds, and the baseline fails on
//! formatting.

use std::fs;
use std::time::Duration;

use serde_json::Value;
use tempfile::TempDir;

use super::{Driver, TrialError};
use crate::client::claude::Claude;
use crate::client::claude::version;
use crate::result::{Arm, Deadlines, IsolationMode, Provenance, SymposiumBuild};
use crate::run::SRT_VERSION;
use crate::run::stage::{Audience, Tool};
use crate::run::test_support::{RealSrt, api_key, built_cargo_agents, executable, inputs, on_path};

const STAND_IN: &str = include_str!("claude-stand-in.js");

#[test]
#[ignore = "needs node, srt, and bubblewrap installed, and builds cargo-agents"]
fn a_trial_runs_end_to_end_under_srt() {
    let sources = TempDir::new().unwrap();
    let mut real = RealSrt::new(api_key());
    real.stage("node", &Tool::Executable(on_path("node")), Audience::Common);
    let package = sources.path().join("claude");
    fs::create_dir(&package).unwrap();
    executable(&package.join("cli.js"), STAND_IN);
    let tool = Tool::NodePackage {
        package,
        entry: "cli.js".into(),
    };
    let claude = real.stage_tool("claude", &tool, Audience::Common);
    let cargo_agents = built_cargo_agents();
    real.stage(
        "cargo-agents",
        &Tool::Executable(cargo_agents.clone()),
        Audience::Symposium,
    );
    let version = version::preflight(real.batch(), &claude, sources.path()).unwrap();
    let build = SymposiumBuild {
        version: "cargo-agents (this checkout)".to_owned(),
        sha256: crate::digest::file(&cargo_agents).unwrap(),
        revision: None,
    };
    let inputs = inputs(real.batch(), "formatting");
    let provenance = provenance(&version.to_string());
    let driver = Driver {
        batch: real.batch(),
        inputs: &inputs,
        machine: real.machine(),
        sandbox: real.srt(),
        api_host: "api.example.com",
        api_key: real.api_key(),
        provenance: &provenance,
    };
    let run = |number, arm| -> Result<Value, TrialError> {
        let runner = Claude::enforced("a-model".to_owned(), version);
        let path = driver.run(number, arm, &runner)?;
        Ok(serde_json::from_slice(&fs::read(path).unwrap()).unwrap())
    };

    let symposium = run(1, Arm::Symposium(build)).unwrap();
    let baseline = run(2, Arm::Baseline).unwrap();

    assert_eq!(symposium["outcome"]["result"], "succeeded", "{symposium:#}");
    assert_eq!(
        symposium["code"]["edits_outside_editable"],
        Value::Array(Vec::new())
    );
    assert_eq!(
        baseline["session"]["ended"]["status"], "completed",
        "{baseline:#}"
    );
    assert_eq!(baseline["code"]["cargo_fmt_check"], "fail", "{baseline:#}");
    assert_eq!(baseline["outcome"]["result"], "failed", "{baseline:#}");
    assert_eq!(baseline["outcome"]["failure"], "code", "{baseline:#}");
}

fn provenance(client_version: &str) -> Provenance {
    Provenance {
        client_version: Some(client_version.to_owned()),
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
