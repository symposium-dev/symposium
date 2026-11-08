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

/// Exercise cancellation while initializing as well as after initialization.
/// The next request must fail without respawning a PM in either case.
async fn abandoned_request_stops_workers(during_initialize: bool, refuse_handshake: bool) {
    let signals = tempfile::tempdir().unwrap();
    let script = signals.path().join("pm.sh");
    let stage = if during_initialize {
        "initialize"
    } else {
        "list_deps"
    };
    std::fs::write(&script, format!(r#"
cd "$1" || exit 1
printf started >> spawns
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed 's/.*"id":\([0-9]*\),"method".*/\1/')
  case "$line" in
    *'"{stage}"'*)
      (
        printf started > worker-started
        i=0
        while [ ! -f release ]; do
          i=$((i+1)); [ $i -lt 500 ] || exit 1
          sleep 0.01
        done
        printf completed > worker-completed
      ) &
      while [ ! -f worker-started ]; do sleep 0.01; done
      if [ "$2" = refuse ]; then
        printf '{{"id":%s,"jsonrpc":"2.0","result":{{"protocol_version":999,"name":"demo"}}}}\n' "$id"
      fi
      wait ;;
    *'"initialize"'*)
      printf '{{"id":%s,"jsonrpc":"2.0","result":{{"protocol_version":1,"name":"demo"}}}}\n' "$id" ;;
  esac
done
"#)).unwrap();
    let pm = RemotePm::new(
        "demo",
        RemotePmCommand::new("/bin/sh")
            .arg(script.to_str().unwrap())
            .arg(signals.path().to_str().unwrap())
            .arg(if refuse_handshake { "refuse" } else { "wait" }),
    );
    if refuse_handshake {
        assert!(pm.list_deps().await.is_err());
    } else {
        let mut request = Box::pin(pm.list_deps());
        let started = async {
            while !signals.path().join("worker-started").exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        };
        tokio::select! {
            result = &mut request => panic!("request unexpectedly finished: {result:?}"),
            result = tokio::time::timeout(Duration::from_secs(5), started) => result.unwrap(),
        }
        drop(request);
    }
    assert!(signals.path().join("worker-started").exists());
    std::fs::write(signals.path().join("release"), "release").unwrap();
    let retry = tokio::time::timeout(Duration::from_secs(1), pm.list_deps()).await;
    assert!(
        retry.unwrap().is_err(),
        "a failed or abandoned handshake stays failed"
    );
    assert_eq!(
        std::fs::read_to_string(signals.path().join("spawns")).unwrap(),
        "started"
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!signals.path().join("worker-completed").exists());
}

#[tokio::test]
async fn abandoning_initialize_stops_descendants_and_does_not_respawn() {
    abandoned_request_stops_workers(true, false).await;
}

#[tokio::test]
async fn abandoning_an_operation_stops_descendants_and_does_not_respawn() {
    abandoned_request_stops_workers(false, false).await;
}

#[tokio::test]
async fn refusing_a_handshake_stops_descendants() {
    abandoned_request_stops_workers(true, true).await;
}
