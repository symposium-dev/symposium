//! `srt`'s short `TMPDIR` alias under real `srt`: run on Linux with
//! `cargo test -p symposium-eval -- --ignored`.
//!
//! Each command's sandbox has its own `tmp/` at `/tmp/symposium-srt`, where `srt` makes its
//! proxy's sockets, so a deep evaluation root still runs, the command keeps its own
//! `TMPDIR`, and no two sandboxes share the alias.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use super::test_support::{RealSrt, api_key, wait_for};
use super::{Configuration, Trial};
use crate::grade::process::{Backend, Finished, Invocation};
use crate::grade::{Grading, Outcome};

const DEADLINE: Duration = Duration::from_secs(120);

/// An evaluation root 150 bytes deep, whose trial and grading `tmp/` paths are far too
/// long for a socket.
fn deep() -> PathBuf {
    std::iter::repeat_n("a-directory-of-thirty-bytes-xx", 5).collect()
}

/// The lines `script` prints, run with `sh -c` in `trial`'s sandbox.
fn lines(finished: &Finished) -> Vec<String> {
    let stdout = std::fs::read_to_string(finished.stdout.log()).unwrap();
    assert!(finished.succeeded(), "{}", finished.stderr.tail());
    stdout.lines().map(str::to_owned).collect()
}

/// Prints the command's `TMPDIR`, then what is at the alias.
const SHOW_TMP: &str = r#"printf '%s\n' "$TMPDIR"; ls -a /tmp/symposium-srt"#;

#[test]
#[ignore = "needs node, srt, and bubblewrap installed"]
fn a_deep_trial_runs_with_its_own_tmpdir_and_srts_sockets_at_the_alias() {
    let real = RealSrt::nested(api_key(), &deep());
    let trial = real.trial_of(
        &crate::task::TaskName::new("formatting").unwrap(),
        1,
        Configuration::Baseline,
    );

    let lines = lines(&real.run(&trial, SHOW_TMP, &[], DEADLINE));

    assert_eq!(Path::new(&lines[0]), trial.tmp(), "{lines:?}");
    // The trial's network goes through a proxy, whose sockets `srt` made at the alias.
    assert!(
        lines.iter().any(|line| line.starts_with("claude-http-")),
        "{lines:?}"
    );
}

#[test]
#[ignore = "needs node, srt, and bubblewrap installed"]
fn a_deep_trial_grades_with_its_own_tmpdir() {
    let real = RealSrt::nested(api_key(), &deep());
    let trial = real.trial_of(
        &crate::task::TaskName::new("formatting").unwrap(),
        1,
        Configuration::Baseline,
    );
    let solution =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../evals/tasks/formatting/solution/src/lib.rs");
    std::fs::copy(solution, trial.project().join("src/lib.rs")).unwrap();

    let grading = real.grade(&trial, "formatting", &trial.project());
    let shown = lines(&grading_run(&real, &trial, SHOW_TMP));

    let Grading::Graded(report) = &grading else {
        panic!("expected a report, got {grading:?}");
    };
    assert!(
        report
            .steps
            .iter()
            .all(|step| step.outcome == Outcome::Passed),
        "{report:?}"
    );
    assert_eq!(Path::new(&shown[0]), trial.grading_tmp(), "{shown:?}");
}

/// Two sandboxes running at once each see only their own directory at the alias.
#[test]
#[ignore = "needs node, srt, and bubblewrap installed"]
fn concurrent_sandboxes_never_share_the_alias() {
    let real = RealSrt::new(api_key());
    let first = real.trial(1, Configuration::Baseline);
    let second = real.trial(2, Configuration::Baseline);
    // Each leaves a marker in its own `tmp/` and says it is ready, then lists the alias
    // once released, which happens only when both are ready: both are running then.
    let script = r#"touch "$TMPDIR/marker-$1"
touch "$TMPDIR/ready"
while [ ! -f "$TMPDIR/release" ]; do sleep 0.02; done
ls /tmp/symposium-srt"#;

    let (first_lines, second_lines) = thread::scope(|scope| {
        let first_run =
            scope.spawn(|| lines(&real.run(&first, script, &["first".into()], DEADLINE)));
        let second_run =
            scope.spawn(|| lines(&real.run(&second, script, &["second".into()], DEADLINE)));
        for trial in [&first, &second] {
            wait_for(&trial.tmp().join("ready"));
        }
        for trial in [&first, &second] {
            std::fs::write(trial.tmp().join("release"), "").unwrap();
        }
        (first_run.join().unwrap(), second_run.join().unwrap())
    });

    for (own, other, lines) in [
        ("marker-first", "marker-second", &first_lines),
        ("marker-second", "marker-first", &second_lines),
    ] {
        assert!(lines.iter().any(|line| line == own), "{lines:?}");
        assert!(!lines.iter().any(|line| line == other), "{lines:?}");
    }
}

/// Runs `script` with `sh -c` under `trial`'s grading policy.
fn grading_run(real: &RealSrt, trial: &Trial, script: &str) -> Finished {
    let backend = real.srt().for_grading(trial).unwrap();
    let invocation = Invocation {
        program: "sh".into(),
        args: vec!["-c".into(), script.into()],
        dir: trial.grading_work(),
        env: vec![("PATH".into(), OsString::from("/usr/bin:/bin"))],
        log_name: "show-tmp".to_owned(),
    };
    std::fs::create_dir_all(trial.grading_tmp()).unwrap();
    backend
        .run(&invocation, DEADLINE, &trial.grading_logs())
        .unwrap()
}
