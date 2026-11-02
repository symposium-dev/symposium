//! A timed-out remote package manager must stop its subprocesses too.
#![cfg(unix)]

use std::time::{Duration, Instant};

use symposium::pm::{PackageManager, RemotePm, RemotePmCommand};

#[tokio::test]
async fn a_timed_out_remote_pm_stops_its_child_processes() {
    let signals = tempfile::tempdir().unwrap();
    let child_script = signals.path().join("worker.sh");
    let pm_script = signals.path().join("pm.sh");
    let started = signals.path().join("worker-started");
    let release = signals.path().join("release");
    let completed = signals.path().join("worker-completed");

    // The worker cannot write its completion marker until the test releases
    // it after timeout. Bound polling as a cleanup fallback on early panic.
    std::fs::write(
        &child_script,
        format!(
            "cd '{}' || exit 1\necho started > worker-started\ni=0\nwhile [ ! -f release ]; do\n  i=$((i+1))\n  [ $i -lt 500 ] || exit 1\n  sleep 0.01\ndone\necho completed > worker-completed\n",
            signals.path().display(),
        ),
    )
    .unwrap();
    std::fs::write(
        &pm_script,
        format!(
            r#"while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed 's/.*"id":\([0-9]*\),"method".*/\1/')
  case "$line" in
    *'"initialize"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"protocol_version":1,"name":"demo"}}}}\n' "$id"
      ;;
    *'"list_deps"'*)
      sh '{}' &
      wait
      ;;
  esac
done
"#,
            child_script.display(),
        ),
    )
    .unwrap();

    let pm = RemotePm::new(
        "demo",
        RemotePmCommand::new("/bin/sh")
            .arg(pm_script.to_str().unwrap())
            .timeout(Duration::from_secs(1)),
    );
    let result = pm.list_deps().await;
    // Always release before asserting, so a failing test does not strand its
    // own worker behind the gate.
    std::fs::write(&release, "release").unwrap();
    drop(pm);
    assert!(started.exists(), "the remote PM's worker must start");
    let error = result.expect_err("the remote PM must time out");
    assert!(format!("{error:#}").contains("timed out"), "{error:#}");

    let observation_deadline = Instant::now() + Duration::from_millis(500);
    while !completed.exists() && Instant::now() < observation_deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        !completed.exists(),
        "remote PM worker mutated files after its request timed out"
    );
}
