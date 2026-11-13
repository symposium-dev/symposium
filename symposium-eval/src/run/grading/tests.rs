use std::fs;

use super::*;
use crate::run::Configuration;
use crate::run::test_support::{Host, trial};

#[test]
fn grading_writes_only_its_work_and_reads_but_never_writes_its_sources() {
    let host = Host::new();
    let batch = host.batch();
    let trial = trial(&batch, 1, Configuration::Symposium);

    let policy = Policy::for_grading(&trial, &host.machine()).unwrap();

    for dir in [trial.grading_work().join("target"), trial.grading_tmp()] {
        assert!(policy.can_write(&dir), "{}", dir.display());
    }
    let hidden = trial.grading_sources().join("project/tests/double.rs");
    assert!(policy.can_read(&hidden));
    assert!(!policy.can_write(&hidden));
    for dir in [
        trial.grading_sources(),
        trial.grading_logs(),
        trial.project(),
        trial.home(),
        trial.evidence(),
        trial.tmp(),
    ] {
        assert!(!policy.can_write(&dir), "{}", dir.display());
    }
}

#[test]
fn grading_reads_the_toolchain_and_nothing_of_the_trial_or_batch() {
    let host = Host::new();
    let batch = host.batch();
    let trial = trial(&batch, 1, Configuration::Symposium);
    let other = crate::run::test_support::trial(&batch, 2, Configuration::Baseline);

    let policy = Policy::for_grading(&trial, &host.machine()).unwrap();

    let cargo = host.join("home/dev/.rustup/toolchains/stable/bin/cargo");
    assert!(policy.can_read(&cargo));
    for path in [
        trial.grading_logs().join("cargo build.stdout.log"),
        trial.project().join("src/lib.rs"),
        trial.evidence().join("hook-records.jsonl"),
        trial.registry().join("fixture-store/Symposium.toml"),
        batch
            .inputs()
            .join("tasks/store-durability/checks/settings_persist.rs"),
        batch.symposium_tools().join("cargo-agents"),
        other.project().join("src/lib.rs"),
        host.join("home/dev/.cargo/credentials.toml"),
        host.join("checkout/evals/tasks/formatting/checks/double.rs"),
    ] {
        assert!(!policy.can_read(&path), "{}", path.display());
    }
}

#[test]
fn grading_refuses_a_cargo_configuration_above_the_copies_it_could_read() {
    let host = Host::new();
    let batch = host.batch();
    let trial = trial(&batch, 1, Configuration::Baseline);
    let config = host.join(".cargo/config.toml");
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(&config, "[build]\nrustc-wrapper = \"wrapper\"\n").unwrap();

    let result = trial.prepare_grading(&host.machine());

    assert!(
        matches!(&result, Err(RunError::AmbientCargoConfig(found)) if *found == config),
        "{result:?}"
    );
    assert!(!trial.grading_policy_file().exists());
}

#[test]
fn a_cargo_configuration_the_sandbox_hides_is_no_concern() {
    let host = Host::new();
    let batch = host.batch();
    let trial = trial(&batch, 1, Configuration::Baseline);
    // In a home, which every sandbox denies, as a developer's usually is.
    let config = host.join("home/dev/.cargo/config.toml");
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(&config, "").unwrap();

    let dirs = trial.prepare_grading(&host.machine()).unwrap();

    assert!(trial.grading_policy_file().is_file());
    assert_eq!(dirs.sources, trial.grading_sources());
    assert!(!dirs.sources.exists(), "the copy is made by grading itself");
}
