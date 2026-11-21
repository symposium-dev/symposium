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
            std::fs::write(root.join("descendant-pid"), std::process::id().to_string()).unwrap();
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
        "report" => {
            println!("installer stdout");
            eprintln!("installer stderr");
        }
        "failure" => std::process::exit(23),
        #[cfg(windows)]
        "priority" => {
            use windows::Win32::System::Threading::{GetCurrentProcess, GetPriorityClass};
            // SAFETY: the pseudo-handle refers to this live process.
            println!("priority={}", unsafe {
                GetPriorityClass(GetCurrentProcess())
            });
        }
        "completed-parent" => {
            #[allow(clippy::zombie_processes)]
            let _descendant = fixture_command("descendant", root).spawn().unwrap();
            assert!(wait_for_fixture_file(&root.join("descendant-ready")));
        }
        other => panic!("unknown fixture role: {other}"),
    }
}

#[tokio::test]
async fn installer_output_and_failure_status_are_preserved() {
    let fixture = Fixture(tempfile::tempdir().unwrap());
    for role in ["report", "failure"] {
        let mut command = fixture_command(role, fixture.0.path());
        process_tree::own_group(&mut command);
        let output = tokio::time::timeout(Duration::from_secs(10), process_tree::output(command))
            .await
            .expect("installer did not finish")
            .unwrap();
        if role == "report" {
            assert!(output.status.success());
            assert!(String::from_utf8_lossy(&output.stdout).contains("installer stdout"));
            assert!(String::from_utf8_lossy(&output.stderr).contains("installer stderr"));
        } else {
            assert_eq!(output.status.code(), Some(23));
        }
    }
}

#[tokio::test]
async fn completed_installer_does_not_wait_for_a_background_worker() {
    let fixture = Fixture(tempfile::tempdir().unwrap());
    let root = fixture.0.path();
    let mut command = fixture_command("completed-parent", root);
    process_tree::own_group(&mut command);
    let output = tokio::time::timeout(Duration::from_secs(5), process_tree::output(command)).await;
    // Bound cleanup of the worker the installer deliberately left behind.
    std::fs::write(root.join("release"), "").unwrap();
    let worker_ran = wait_for_file(&root.join("side-effect"), Duration::from_secs(3)).await;

    assert!(
        output
            .expect(
                "completion must report the installer without waiting for its background worker"
            )
            .unwrap()
            .status
            .success()
    );
    assert!(
        worker_ran,
        "normal completion must leave the background worker alive"
    );
}

#[cfg(windows)]
#[tokio::test]
async fn foreground_acquisition_cancellation_also_stops_orphaned_descendants() {
    use symposium_install::{CargoSource, InstallContext, Source, UpdateLevel, acquire_source};

    let fixture = Fixture(tempfile::tempdir().unwrap());
    let root = fixture.0.path();
    let cargo = root.join("cargo.cmd");
    std::fs::copy(std::env::current_exe().unwrap(), root.join("fixture.exe")).unwrap();
    // Keep the batch source ASCII: %~dp0 supplies paths without embedding the
    // user's potentially non-ASCII home directory in a code-page-dependent file.
    // Environment changes belong to the child, never the parallel test process.
    std::fs::write(
        &cargo,
        format!(
            "@echo off\r\nset \"{ROLE}=installer\"\r\nset \"{ROOT}=%~dp0\"\r\n\
             \"%~dp0fixture.exe\" --exact {TEST_NAME} --nocapture\r\nexit /b %errorlevel%\r\n"
        ),
    )
    .unwrap();
    let context = InstallContext::new(root.join("cache"))
        .with_cargo_bin(cargo)
        .in_foreground();
    let source = Source::Cargo(
        CargoSource::new("cancellation-fixture").with_git("https://example.invalid/fixture"),
    );
    let mut tasks = tokio::task::JoinSet::new();
    tasks.spawn(async move {
        acquire_source(&context, &source, Some("fixture"), UpdateLevel::None).await
    });
    let ready = wait_for_file(&root.join("installer-ready"), Duration::from_secs(10)).await;
    let witness = ProcessWitness::open(root);
    tasks.abort_all();
    let cancelled = tasks.join_next().await.unwrap();
    witness.assert_exited();
    std::fs::write(root.join("release"), "").unwrap();
    let survived = wait_for_file(&root.join("side-effect"), Duration::from_secs(3)).await;

    assert!(ready, "foreground acquisition never started the installer");
    assert!(cancelled.is_err_and(|error| error.is_cancelled()));
    assert!(
        !survived,
        "a foreground installer descendant survived acquisition cancellation"
    );
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
    #[cfg(windows)]
    let witness = ProcessWitness::open(root);
    tasks.abort_all();
    let cancelled = tasks.join_next().await.unwrap();
    #[cfg(windows)]
    witness.assert_exited();

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

#[cfg(windows)]
struct ProcessWitness(std::os::windows::io::OwnedHandle);

#[cfg(windows)]
impl ProcessWitness {
    fn open(root: &Path) -> Self {
        use std::os::windows::io::FromRawHandle;
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE};
        let pid = std::fs::read_to_string(root.join("descendant-pid"))
            .unwrap()
            .parse()
            .unwrap();
        // SAFETY: the worker is gated alive; OpenProcess validates its PID.
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }.unwrap();
        // SAFETY: OpenProcess returned an owned handle, transferred exactly once.
        Self(unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(handle.0) })
    }

    fn assert_exited(&self) {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
        use windows::Win32::System::Threading::WaitForSingleObject;
        // SAFETY: the retained process handle grants SYNCHRONIZE access.
        let status = unsafe { WaitForSingleObject(HANDLE(self.0.as_raw_handle()), 0) };
        assert_eq!(
            status, WAIT_OBJECT_0,
            "cancellation returned before the worker terminated"
        );
    }
}

#[cfg(windows)]
#[tokio::test]
async fn explicit_windows_creation_flags_reach_the_installer() {
    use windows::Win32::System::Threading::{CREATE_NO_WINDOW, IDLE_PRIORITY_CLASS};
    let fixture = Fixture(tempfile::tempdir().unwrap());
    let output = process_tree::output_with_creation_flags(
        fixture_command("priority", fixture.0.path()),
        (CREATE_NO_WINDOW | IDLE_PRIORITY_CLASS).0,
    )
    .await
    .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains(&format!("priority={}", IDLE_PRIORITY_CLASS.0))
    );
}

#[cfg(windows)]
#[tokio::test]
async fn explicit_suspension_is_rejected_before_starting_an_installer() {
    use windows::Win32::System::Threading::CREATE_SUSPENDED;
    let fixture = Fixture(tempfile::tempdir().unwrap());
    let error = process_tree::output_with_creation_flags(
        fixture_command("descendant", fixture.0.path()),
        CREATE_SUSPENDED.0,
    )
    .await
    .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(!fixture.0.path().join("descendant-ready").exists());
}
