//! Real sandbox regressions with fake credentials and no model requests.

use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use tempfile::TempDir;

use super::test_support::{RealSrt, api_key, on_path, wait_for};
use super::{ApiKey, Configuration};

const DEADLINE: Duration = Duration::from_secs(60);

#[test]
#[ignore = "needs node, srt, and bubblewrap installed"]
fn concurrent_trials_keep_temporary_files_and_processes_separate() {
    let real = RealSrt::new(api_key());
    let first = real.trial(1, Configuration::Baseline);
    let second = real.trial(2, Configuration::Baseline);
    let script = r#"
set -eu
printf '%s' "$1" > /tmp/claude/shared-name
printf ready > "$TMPDIR/ready"
while [ ! -f "$TMPDIR/release" ]; do sleep 0.02; done
cat /tmp/claude/shared-name
"#;
    thread::scope(|scope| {
        let one = scope.spawn(|| real.run(&first, script, &["first".into()], DEADLINE));
        let two = scope.spawn(|| real.run(&second, script, &["second".into()], DEADLINE));
        wait_for(&first.tmp().join("ready"));
        wait_for(&second.tmp().join("ready"));
        // Both have written the same pathname before either reads it. End one sandbox
        // while the other is still alive, then check the survivor can finish normally.
        fs::write(first.tmp().join("release"), "").unwrap();
        let one = one.join().unwrap();
        fs::write(second.tmp().join("release"), "").unwrap();
        let two = two.join().unwrap();
        assert!(one.succeeded(), "{}", one.stderr.tail());
        assert!(two.succeeded(), "{}", two.stderr.tail());
        assert_eq!(one.stdout.tail(), "first");
        assert_eq!(two.stdout.tail(), "second");
    });
}

/// The fixture owns its process, including when an assertion unwinds.
struct TestApi {
    _directory: TempDir,
    child: Child,
    certificate: PathBuf,
    requests: PathBuf,
    port: u16,
}

impl Drop for TestApi {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl TestApi {
    fn start() -> Self {
        let directory = TempDir::new().unwrap();
        let certificate = directory.path().join("ca.pem");
        let key = directory.path().join("key.pem");
        let requests = directory.path().join("requests.jsonl");
        let ready = directory.path().join("port");
        let generated = Command::new("openssl")
            .args([
                "req",
                "-x509",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-days",
                "1",
                "-subj",
                "/CN=localhost",
                "-addext",
                "subjectAltName=DNS:localhost",
                "-keyout",
            ])
            .arg(&key)
            .arg("-out")
            .arg(&certificate)
            .output()
            .unwrap();
        assert!(
            generated.status.success(),
            "{}",
            String::from_utf8_lossy(&generated.stderr)
        );
        fs::write(&requests, "").unwrap();
        let server = r#"
const fs = require('node:fs');
const https = require('node:https');
const [key, cert, requests, ready] = process.argv.slice(1);
const server = https.createServer({key: fs.readFileSync(key), cert: fs.readFileSync(cert)}, (req, res) => {
    fs.appendFileSync(requests, JSON.stringify({url: req.url, key: req.headers['x-api-key']}) + '\n');
    res.end('ok');
});
server.listen(0, '127.0.0.1', () => fs.writeFileSync(ready, String(server.address().port)));
setTimeout(() => process.exit(0), 120000);
"#;
        let child = Command::new(on_path("node"))
            .env_clear()
            .args(["-e", server])
            .args([&key, &certificate, &requests, &ready])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut api = Self {
            _directory: directory,
            child,
            certificate,
            requests,
            port: 0,
        };
        wait_for(&ready);
        api.port = fs::read_to_string(ready).unwrap().parse().unwrap();
        api
    }
}

#[test]
#[ignore = "needs node, srt, bubblewrap, openssl, and curl installed"]
fn only_the_allowed_https_api_receives_the_real_key() {
    let api = TestApi::start();
    let canary = "sk-local-test-only";
    let real = RealSrt::new(ApiKey::new("EXAMPLE_API_KEY", canary.into()));
    let trial = real.trial(1, Configuration::Baseline);
    let script = r#"
set -eu
printf 'sandbox-key=%s\n' "$EXAMPLE_API_KEY"
curl --silent --show-error --fail --max-time 15 --noproxy '' \
    -H "x-api-key: $EXAMPLE_API_KEY" "https://localhost:$1/allowed"
# Same server, but this hostname is absent from the policy. It must be the
# proxy rejecting CONNECT, not an upstream certificate error or failed DNS.
if curl --silent --show-error --max-time 15 --noproxy '' \
    -H "x-api-key: $EXAMPLE_API_KEY" "https://127.0.0.1:$1/blocked" 2> "$TMPDIR/denied"; then
    echo 'disallowed host was reachable' >&2; exit 1
fi
grep -q 'CONNECT tunnel failed, response 403' "$TMPDIR/denied"
"#;
    let finished = real.run_with_test_api(&trial, script, api.port, &api.certificate);
    assert!(finished.succeeded(), "{}", finished.stderr.tail());
    let stdout = fs::read_to_string(finished.stdout.log()).unwrap();
    let stderr = fs::read_to_string(finished.stderr.log()).unwrap();
    assert!(
        !stdout.contains(canary),
        "the key appeared in sandbox output"
    );
    assert!(
        !stderr.contains(canary),
        "the key appeared in sandbox errors"
    );
    let requests = fs::read_to_string(&api.requests).unwrap();
    let requests: Vec<serde_json::Value> = requests
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        requests,
        [serde_json::json!({"url": "/allowed", "key": canary})]
    );
}

#[test]
#[ignore = "needs node, srt, and bubblewrap installed"]
fn project_symlinks_cannot_reach_denied_files_or_modify_fixtures() {
    use super::test_support::checkout;

    let real = RealSrt::new(api_key());
    let trial = real.trial(1, Configuration::Baseline);
    let other = real.trial(2, Configuration::Baseline);
    let hidden = checkout().join("evals/tasks/store-durability/solution/src/lib.rs");
    let other_file = other.project().join("private-marker");
    fs::write(&other_file, "other trial").unwrap();
    let fixture = trial.fixtures().join("fixture-store/src/lib.rs");
    assert!(hidden.is_file());
    let before = fs::read(&fixture).unwrap();
    let script = r#"
set -eu
ln -s "$1" hidden-link
ln -s "$2" other-link
ln -s "$3" fixture-link
if cat hidden-link >/dev/null 2>&1; then echo 'read hidden solution' >&2; exit 1; fi
if cat other-link >/dev/null 2>&1; then echo 'read other trial' >&2; exit 1; fi
# Check the link actually resolves before testing that writes are denied.
cat fixture-link >/dev/null
if (printf corrupted > fixture-link) 2>/dev/null; then echo 'wrote fixture' >&2; exit 1; fi
"#;
    let finished = real.run(
        &trial,
        script,
        &[hidden.into(), other_file.into(), fixture.clone().into()],
        DEADLINE,
    );
    assert!(finished.succeeded(), "{}", finished.stderr.tail());
    assert_eq!(fs::read(fixture).unwrap(), before);
}

#[test]
#[ignore = "needs node, srt, bubblewrap, setsid, and flock installed"]
fn failed_setup_stops_descendants_and_cannot_reuse_the_trial() {
    check_failed_setup(false);
}

#[test]
#[ignore = "needs node, srt, bubblewrap, setsid, and flock installed"]
fn timed_out_setup_stops_descendants_and_cannot_reuse_the_trial() {
    check_failed_setup(true);
}

fn check_failed_setup(timeout: bool) {
    use super::RunError;
    use super::client::Client;
    use super::stage::{Audience, Tool};
    use super::test_support::{executable, store_durability};

    let mut real = RealSrt::new(api_key());
    let source = TempDir::new().unwrap();
    let fake = source.path().join("cargo-agents");
    let mut script = String::from(
        r#"#!/bin/sh
set -eu
setsid sh -c '
    exec 9> "$TMPDIR/child.lock"
    flock -x 9
    printf ready > "$TMPDIR/child-ready"
    sleep 60
' &
while [ ! -f "$TMPDIR/child-ready" ]; do sleep 0.02; done
"#,
    );
    script.push_str(if timeout { "sleep 60\n" } else { "exit 23\n" });
    executable(&fake, &script);
    real.stage("cargo-agents", &Tool::Executable(fake), Audience::Symposium);
    let trial = real.trial(1, Configuration::Symposium);
    let result = real.try_set_up_symposium(&trial, Client::ClaudeCode, Duration::from_secs(5));
    assert!(
        trial.tmp().join("child-ready").is_file(),
        "setup never reached its child: {result:?}"
    );
    if timeout {
        assert!(
            matches!(result, Err(RunError::ToolTimeout { .. })),
            "{result:?}"
        );
    } else {
        assert!(
            matches!(result, Err(RunError::ToolFailed { status, .. }) if status.code() == Some(23)),
            "{result:?}"
        );
    }
    // The detached shell and its sleep inherit this lock. Acquiring it on the host
    // proves they released it, without waiting for a delayed marker to appear.
    let unlocked = Command::new("flock")
        .args(["--nonblock", "--exclusive"])
        .arg(trial.tmp().join("child.lock"))
        .arg("true")
        .status()
        .unwrap();
    assert!(
        unlocked.success(),
        "a setup descendant still holds the lock"
    );
    assert!(
        trial.symposium_config().is_file(),
        "failed setup evidence was removed"
    );
    let retry =
        real.batch()
            .create_trial(1, Configuration::Symposium, &store_durability(real.batch()));
    assert!(
        matches!(retry, Err(RunError::Create { source, .. }) if source.kind() == std::io::ErrorKind::AlreadyExists)
    );
    assert!(
        trial.tmp().join("child-ready").is_file(),
        "retry altered the failed trial"
    );
}
