//! A result as it is written: errors cut to the limit, the file whole and never replaced.

use std::fs;
use std::io;

use serde_json::Value;
use tempfile::TempDir;

use super::result_of;
use crate::result::{CUT, MAX_ERROR, Reached, SCHEMA_VERSION, SetupStep, capped};
use crate::run::Configuration;

#[test]
fn a_long_error_is_cut_within_the_limit_on_a_character_boundary() {
    let cut = capped("é".repeat(MAX_ERROR));

    assert!(cut.ends_with(CUT));
    assert!(cut.len() <= MAX_ERROR, "{} bytes", cut.len());
}

#[test]
fn a_result_is_written_whole_and_never_replaced() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("result.json");
    let error = io::Error::other("no client");
    let reached = Reached::Setup {
        step: SetupStep::Setup,
        error: &error,
    };
    let result = result_of(Configuration::Baseline, reached);

    result.write(&path).unwrap();
    let again = result.write(&path);

    let written: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(written["schema_version"], SCHEMA_VERSION);
    assert_eq!(again.unwrap_err().kind(), io::ErrorKind::AlreadyExists);
    let left = fs::read_dir(dir.path()).unwrap().count();
    assert_eq!(left, 1, "a temporary file was left behind");
}
