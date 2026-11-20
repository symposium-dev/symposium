//! Cancellation must stop descendants even after an intermediary exits.
//! Re-execute this native test binary as each fixture process, so the test
//! exercises real Windows process ancestry without a shell or generated code.
#![cfg(any(unix, windows))]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use symposium_install::process_tree;

const TEST_NAME: &str = "cancellation_stops_descendants_after_their_parent_exits";
const ROLE: &str = "SYMPOSIUM_CANCELLATION_TEST_ROLE";
const ROOT: &str = "SYMPOSIUM_CANCELLATION_TEST_ROOT";
const FIXTURE_LIMIT: Duration = Duration::from_secs(15);

fn fixture_command(role: &str, root: &Path) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", TEST_NAME, "--nocapture"])
        .env(ROLE, role)
        .env(ROOT, root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    command
}

fn wait_for_fixture_file(path: &Path) -> bool {
    let deadline = Instant::now() + FIXTURE_LIMIT;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

fn run_fixture(role: &str, root: &Path) {
    match role {
        "descendant" => {
            std::fs::write(root.join("descendant-ready"), "").unwrap();
            if wait_for_fixture_file(&root.join("release")) {
                std::fs::write(
                    root.join("side-effect"),
                    "descendant ran after cancellation completed",
                )
                .unwrap();
            }
        }
        "intermediary" => {
            // Deliberately leave the child alive when this process exits.
            // The child's release gate and lifetime limit bound its cleanup.
            #[allow(clippy::zombie_processes)]
            let _descendant = fixture_command("descendant", root).spawn().unwrap();
            assert!(wait_for_fixture_file(&root.join("descendant-ready")));
        }
        "installer" => {
            let status = fixture_command("intermediary", root).status().unwrap();
            assert!(status.success());
            // The intermediary has exited and been reaped before cancellation.
            std::fs::write(root.join("installer-ready"), "").unwrap();
            let _ = wait_for_fixture_file(&root.join("cleanup"));
        }
        other => panic!("unknown fixture role: {other}"),
    }
}

async fn wait_for_file(path: &Path, limit: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + limit;
    while tokio::time::Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    false
}

// Release fixture processes if an earlier assertion fails. Each fixture also
// exits independently after FIXTURE_LIMIT if the entire test process is killed.
struct Fixture(tempfile::TempDir);

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::write(self.0.path().join("cleanup"), "");
        let _ = std::fs::write(self.0.path().join("release"), "");
    }
}

#[tokio::test]
async fn cancellation_stops_descendants_after_their_parent_exits() {
    if let Some(role) = std::env::var_os(ROLE) {
        let root = PathBuf::from(std::env::var_os(ROOT).unwrap());
        run_fixture(role.to_str().unwrap(), &root);
        return;
    }

    let fixture = Fixture(tempfile::tempdir().unwrap());
    let root = fixture.0.path();
    let mut command = fixture_command("installer", root);
    process_tree::own_group(&mut command);
    let mut tasks = tokio::task::JoinSet::new();
    tasks.spawn(process_tree::output(command));

    let ready = wait_for_file(&root.join("installer-ready"), Duration::from_secs(10)).await;
    tasks.abort_all();
    let cancelled = tasks.join_next().await.unwrap();

    // The descendant cannot perform the observed action before cancellation:
    // this release gate is written only after output() and its guard are gone.
    assert!(!root.join("side-effect").exists());
    std::fs::write(root.join("release"), "").unwrap();
    let survived = wait_for_file(&root.join("side-effect"), Duration::from_secs(3)).await;

    assert!(ready, "the installer fixture did not become ready");
    assert!(
        cancelled.is_err_and(|error| error.is_cancelled()),
        "the installer must still be running when its future is cancelled"
    );
    println!("intermediary_exited_before_cancellation={ready}");
    println!("cancellation_completed=true");
    println!("descendant_wrote_after_cancellation={survived}");
    assert!(
        !survived,
        "installer descendant survived cancellation and wrote a file after cleanup returned"
    );
}
