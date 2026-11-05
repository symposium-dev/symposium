//! Whether each expected plugin hook left a successful record (RFD reference,
//! "Synchronization and hooks").
//!
//! Each hook in `evals/` appends a JSON line, naming itself and its exit code, to
//! `hook-records.jsonl` in `EVAL_EVIDENCE_DIR`.

use std::path::Path;

use serde::Deserialize;

use super::log::{self, CallEvidence, Counted};
use crate::task::HookName;

/// The fields this reader needs.
#[derive(Debug, Deserialize)]
struct Record {
    hook: String,
    exit_code: u8,
}

/// Whether `records` shows `hook` succeeding.
pub(crate) fn hook_record(records: &Path, hook: &HookName, counted: Counted) -> CallEvidence {
    log::calls(records, counted, |record: &Record| {
        (record.hook == hook.as_ref()).then_some(record.exit_code)
    })
}

/// How many of `records` are `hook`'s, among those that can be read.
pub(crate) fn record_count(records: &Path, hook: &HookName) -> usize {
    log::count(records, |record: &Record| record.hook == hook.as_ref())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    fn record(hook: &str, exit_code: u8) -> String {
        serde_json::json!({ "hook": hook, "already_formatted": true, "exit_code": exit_code })
            .to_string()
    }

    fn evidence(lines: &[String], counted: Counted) -> CallEvidence {
        let dir = TempDir::new().unwrap();
        let records = dir.path().join("hook-records.jsonl");
        let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
        fs::write(&records, text).unwrap();
        let hook = HookName::new("format-on-stop").unwrap();
        hook_record(&records, &hook, counted)
    }

    #[test]
    fn only_the_named_hooks_records_count() {
        let lines = [record("other-hook", 0), record("format-on-stop", 1)];

        assert_eq!(evidence(&lines, Counted::Any), CallEvidence::Failed);
        assert_eq!(
            evidence(&[record("other-hook", 0)], Counted::Any),
            CallEvidence::Missing
        );
    }

    #[test]
    fn a_stop_hook_is_decided_by_its_first_record() {
        let lines = [record("format-on-stop", 1), record("format-on-stop", 0)];

        assert_eq!(evidence(&lines, Counted::First), CallEvidence::Failed);
        assert_eq!(evidence(&lines, Counted::Any), CallEvidence::Succeeded);
    }

    /// Runs the real `Stop` hook, and with it the plugin's, under real `srt`.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "needs node, srt, and bubblewrap installed, and builds cargo-agents"]
    fn the_format_on_stop_hook_leaves_a_record_this_reader_reads() {
        use crate::run::Configuration;
        use crate::run::client::Client;
        use crate::run::stage::{Audience, Tool};
        use crate::run::test_support::{RealSrt, api_key, built_cargo_agents};
        use crate::task::TaskName;

        let mut real = RealSrt::new(api_key());
        let tool = Tool::Executable(built_cargo_agents());
        real.stage("cargo-agents", &tool, Audience::Symposium);
        let task = TaskName::new("formatting").unwrap();
        let trial = real.trial_of(&task, 1, Configuration::Symposium);
        real.set_up_symposium(&trial, Client::ClaudeCode);
        assert!(
            !trial.hook_records().exists(),
            "a record before the hook ran"
        );

        let finished = real.run_stop(&trial);

        assert!(finished.succeeded(), "{}", finished.stderr.tail());
        let hook = HookName::new("format-on-stop").unwrap();
        assert_eq!(
            hook_record(&trial.hook_records(), &hook, Counted::First),
            CallEvidence::Succeeded,
            "{}",
            std::fs::read_to_string(trial.hook_records()).unwrap_or_default()
        );
    }
}
