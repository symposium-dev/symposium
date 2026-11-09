//! One line per trial, read back from the result it kept, so what is printed is what
//! was recorded.

use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::run::Configuration;
use crate::run::trial::RESULT_FILE;

/// Prints the outcome the trial kept at `kept` recorded, such as
/// `trial-1-with-symposium: failed (expectation)`.
pub(crate) fn trial(kept: &Path) {
    let name = kept.file_name().unwrap_or_default().to_string_lossy();
    println!("{name}: {}", outcome(&kept.join(RESULT_FILE)));
}

/// Prints why one configuration in `round` has no result.
pub(crate) fn no_result(round: usize, configuration: Configuration, error: &anyhow::Error) {
    println!("trial-{round}-{configuration}: no result: {error:#}");
}

fn outcome(result: &Path) -> String {
    let read = fs::read(result)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    let Some(result) = read else {
        return "result unreadable".to_owned();
    };
    describe(&result)
}

/// `result`'s outcome, its failure or the phase it stopped at when there is one.
fn describe(result: &Value) -> String {
    let outcome = &result["outcome"];
    let name = outcome["result"].as_str().unwrap_or("unknown");
    let detail = outcome["failure"].as_str().or_else(|| {
        (name == "setup_failure")
            .then(|| result["reached"]["phase"].as_str())
            .flatten()
    });
    match detail {
        Some(detail) => format!("{name} ({detail})"),
        None => name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_failure_names_what_failed() {
        let result = json!({ "outcome": { "result": "failed", "failure": "expectation" } });

        assert_eq!(describe(&result), "failed (expectation)");
    }

    #[test]
    fn a_setup_failure_names_where_it_stopped() {
        let result = json!({
            "reached": { "phase": "prelaunch" },
            "outcome": { "result": "setup_failure" }
        });

        assert_eq!(describe(&result), "setup_failure (prelaunch)");
    }

    #[test]
    fn a_success_is_one_word() {
        let result = json!({ "outcome": { "result": "succeeded" } });

        assert_eq!(describe(&result), "succeeded");
    }
}
