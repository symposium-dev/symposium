//! Cancelling acquisition must also stop the Git revision probe that follows
//! a successful Cargo install, otherwise runtime shutdown waits for that probe.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

use symposium_install::{CargoSource, InstallContext, Source, UpdateLevel, acquire_source};

const CHILD_ROOT: &str = "SYMPOSIUM_GIT_TIMEOUT_TEST_ROOT";

fn wait_until(mut ready: impl FnMut() -> bool, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    loop {
        if ready() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct ChildGuard {
    process: Child,
    status: Option<ExitStatus>,
}

impl ChildGuard {
    fn exited(&mut self) -> bool {
        if self.status.is_none() {
            self.status = self.process.try_wait().unwrap();
        }
        self.status.is_some()
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        // Kill only while the child has not been reaped, so its process-group
        // ID cannot have been reused by an unrelated process. This also covers
        // failures before the parent opens the Git subprocess's release gate.
        if self.status.is_none() {
            unsafe {
                libc::kill(-(self.process.id() as libc::pid_t), libc::SIGKILL);
            }
            let _ = self.process.wait();
        }
    }
}

fn run_acquisition_child(root: &Path) {
    let context = InstallContext::new(root.join("cache")).with_cargo_bin(root.join("cargo"));
    let mut cargo = CargoSource::new("git-timeout-probe");
    cargo.git = Some("https://example.invalid/git-timeout-probe".into());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let source = Source::Cargo(cargo);
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            acquire_source(
                &context,
                &source,
                Some("git-timeout-probe"),
                UpdateLevel::None,
            ),
        )
        .await;
        assert!(
            result.is_err(),
            "acquisition must time out in the Git probe"
        );
        std::fs::write(root.join("timed-out"), "").unwrap();
    });
    drop(runtime);
    std::fs::write(root.join("runtime-stopped"), "").unwrap();
}

#[test]
fn timed_out_git_probe_does_not_hold_runtime_shutdown() {
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        run_acquisition_child(Path::new(&root));
        return;
    }

    let signals = tempfile::tempdir().unwrap();
    let root = signals.path();
    // Cargo succeeds without invoking the network. The subsequent Git probe
    // waits for the parent to release it after observing runtime shutdown.
    std::fs::write(
        root.join("cargo"),
        "#!/bin/sh\ncase \"$1\" in binstall) exit 1;; install) exit 0;; *) exit 2;; esac\n",
    )
    .unwrap();
    std::fs::write(
        root.join("git"),
        "#!/bin/sh\n\
         : > \"$SYMPOSIUM_GIT_TIMEOUT_TEST_ROOT/git-started\"\n\
         i=0\n\
         while [ ! -f \"$SYMPOSIUM_GIT_TIMEOUT_TEST_ROOT/release\" ]; do\n\
           i=$((i+1))\n\
           [ $i -lt 1000 ] || exit 1\n\
           sleep 0.01\n\
         done\n\
         echo '0123456789abcdef HEAD'\n",
    )
    .unwrap();
    for name in ["cargo", "git"] {
        std::fs::set_permissions(root.join(name), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut child = ChildGuard {
        process: Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "timed_out_git_probe_does_not_hold_runtime_shutdown",
                "--nocapture",
            ])
            .env(CHILD_ROOT, root)
            .env(
                "PATH",
                std::env::join_paths([root, Path::new("/usr/bin"), Path::new("/bin")]).unwrap(),
            )
            .process_group(0)
            .spawn()
            .unwrap(),
        status: None,
    };

    wait_until(
        || root.join("timed-out").exists() || child.exited(),
        Duration::from_secs(5),
    );
    let timed_out = root.join("timed-out").exists();
    if timed_out {
        wait_until(
            || root.join("runtime-stopped").exists() || child.exited(),
            Duration::from_millis(500),
        );
    }
    let stopped_before_release = root.join("runtime-stopped").exists();
    // Release before asserting, so the failing implementation can finish its
    // blocking worker and we never strand it behind the test's own gate.
    std::fs::write(root.join("release"), "").unwrap();
    let finished = wait_until(|| child.exited(), Duration::from_secs(5));

    assert!(
        root.join("git-started").exists(),
        "the Git probe must actually start"
    );
    assert!(timed_out, "source acquisition must hit its deadline");
    assert!(finished, "child must finish after release");
    assert!(child.status.unwrap().success(), "acquisition child failed");
    assert!(
        stopped_before_release,
        "timed-out Git probe kept runtime shutdown waiting for the release gate"
    );
}
