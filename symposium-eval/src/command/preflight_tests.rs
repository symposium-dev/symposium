//! Fast command-boundary regressions use a controlled backend: the missing-rustfmt
//! case is simulated by making Cargo reject the formatter subcommand. The ignored
//! Linux command test in `real` remains the production-shaped check that successful
//! preparation and a complete journey run under real `srt`.

use std::cell::{Cell, RefCell};
use std::fs;
use std::path::Path;
use std::time::Duration;

use tempfile::TempDir;

use super::{after_task_preflights, preflight_task, probe_publication};
use crate::grade::ProcessError;
use crate::grade::process::{Backend, Finished, Invocation, Local};
use crate::run::test_support::{Host, inputs};
use crate::run::trial::Sandbox;
use crate::run::{Batch, Inputs, Machine, RunError, Trial};
use crate::task::BatchName;

#[derive(Clone, Copy)]
enum PreflightBackend {
    Local,
    MissingFmt,
}

impl Backend for PreflightBackend {
    fn run(
        &self,
        invocation: &Invocation,
        deadline: Duration,
        logs: &Path,
    ) -> Result<Finished, ProcessError> {
        if matches!(self, Self::MissingFmt)
            && invocation.args.iter().any(|argument| argument == "fmt")
        {
            let mut unavailable = invocation.clone();
            unavailable.args = vec!["symposium-missing-rustfmt".into()];
            Local.run(&unavailable, deadline, logs)
        } else {
            Local.run(invocation, deadline, logs)
        }
    }
}

struct PreflightSandbox(PreflightBackend);

impl Sandbox for PreflightSandbox {
    type Backend<'a> = PreflightBackend;

    fn for_trial(&self, _trial: &Trial) -> Result<Self::Backend<'_>, RunError> {
        Ok(self.0)
    }

    fn for_grading(&self, _trial: &Trial) -> Result<Self::Backend<'_>, RunError> {
        Ok(self.0)
    }
}

fn preflight_with_launch_count(
    batch: &Batch,
    inputs: &Inputs,
    machine: &Machine,
    sandbox: &impl Sandbox,
) -> (anyhow::Result<()>, usize) {
    let launches = Cell::new(0);
    let result = after_task_preflights(
        [inputs],
        |inputs| preflight_task(batch, inputs, machine, sandbox),
        || {
            launches.set(launches.get() + 1);
            Ok(())
        },
    );
    (result, launches.get())
}

#[test]
fn publication_preflight_stages_and_renames_inside_results() {
    let root = TempDir::new().unwrap();
    let results = root.path().join("task/client/batch");

    probe_publication(&results).unwrap();

    assert!(results.is_dir());
    assert_eq!(fs::read_dir(&results).unwrap().count(), 0);
}

#[test]
fn publication_preflight_never_reuses_a_batch_results_directory() {
    let root = TempDir::new().unwrap();
    let results = root.path().join("task/client/batch");
    probe_publication(&results).unwrap();

    let error = probe_publication(&results).unwrap_err();

    assert!(error.to_string().contains("cannot create"), "{error:#}");
}

#[test]
fn every_task_passes_preflight_before_a_client_can_launch() {
    let root = TempDir::new().unwrap();
    let first_batch = Batch::create(root.path(), &BatchName::new("first").unwrap()).unwrap();
    let second_batch = Batch::create(root.path(), &BatchName::new("second").unwrap()).unwrap();
    let first = inputs(&first_batch, "formatting");
    let second = inputs(&second_batch, "store-durability");
    let checked = RefCell::new(Vec::new());
    let launches = Cell::new(0);

    let result = after_task_preflights(
        [&first, &second],
        |inputs| {
            let name = inputs.task().name().as_str();
            checked.borrow_mut().push(name.to_owned());
            if name == "store-durability" {
                anyhow::bail!("injected later-task failure");
            }
            Ok(())
        },
        || {
            launches.set(launches.get() + 1);
            Ok(())
        },
    );

    assert!(result.is_err());
    assert_eq!(&*checked.borrow(), &["formatting", "store-durability"]);
    assert_eq!(launches.get(), 0);
}

#[test]
fn missing_rustfmt_stops_before_a_client_launches() {
    let host = Host::new();
    let batch = host.batch();
    let inputs = inputs(&batch, "formatting");

    let (result, launches) = preflight_with_launch_count(
        &batch,
        &inputs,
        &host.machine(),
        &PreflightSandbox(PreflightBackend::MissingFmt),
    );

    let error = result.unwrap_err();
    assert!(
        format!("{error:#}").contains("cargo fmt --version"),
        "{error:#}"
    );
    assert_eq!(launches, 0);
}

#[test]
fn ambient_cargo_configuration_stops_before_a_client_launches() {
    let host = Host::new();
    let config = host.join(".cargo/config.toml");
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(&config, "[build]\nincremental = false\n").unwrap();
    let batch = host.batch();
    let inputs = inputs(&batch, "store-durability");

    let (result, launches) = preflight_with_launch_count(
        &batch,
        &inputs,
        &host.machine(),
        &PreflightSandbox(PreflightBackend::Local),
    );

    let error = result.unwrap_err();
    assert!(
        format!("{error:#}").contains("Cargo would read"),
        "{error:#}"
    );
    assert_eq!(launches, 0);
}

#[test]
fn invalid_starter_metadata_stops_before_a_client_launches() {
    let host = Host::new();
    let batch = host.batch();
    let inputs = inputs(&batch, "store-durability");
    fs::write(
        inputs
            .evals()
            .starter_dir(inputs.task().name())
            .join("Cargo.toml"),
        "this is not TOML",
    )
    .unwrap();

    let (result, launches) = preflight_with_launch_count(
        &batch,
        &inputs,
        &host.machine(),
        &PreflightSandbox(PreflightBackend::Local),
    );

    let error = result.unwrap_err();
    assert!(format!("{error:#}").contains("cargo metadata"), "{error:#}");
    assert_eq!(launches, 0);
}

#[test]
fn publication_failure_stops_before_a_client_launches() {
    let root = TempDir::new().unwrap();
    let results = root.path().join("results");
    fs::write(&results, "not a directory").unwrap();
    let launches = Cell::new(0);

    let result = after_task_preflights(
        std::iter::empty(),
        |_| unreachable!("there are no tasks"),
        || {
            probe_publication(&results)?;
            launches.set(launches.get() + 1);
            Ok(())
        },
    );

    let error = result.unwrap_err();
    assert!(format!("{error:#}").contains("cannot create"), "{error:#}");
    assert_eq!(launches.get(), 0);
}
