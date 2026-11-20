//! Exercise Windows image locks using real Cargo and a dependency-free package.
//! Only registry version discovery is stubbed; Cargo performs every install.
#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::time::Duration;

use process_wrap::tokio::{ChildWrapper, CommandWrap, JobObject, KillOnDrop};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::time::timeout;

fn package() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/self-update")
}

fn spawn(mut command: Command) -> Box<dyn ChildWrapper> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut command = CommandWrap::from(command);
    command.wrap(KillOnDrop).wrap(JobObject);
    command.spawn().unwrap()
}

async fn output(command: Command, input: &[u8]) -> Output {
    let mut child = spawn(command);
    timeout(Duration::from_secs(120), async move {
        let mut stdin = child.stdin().take().unwrap();
        stdin.write_all(input).await.unwrap();
        drop(stdin);
        Box::into_pin(child.wait_with_output()).await.unwrap()
    })
    .await
    .expect("test command exceeded 120 seconds; its Windows job is terminated")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "status: {}\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

struct Fixture {
    root: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("self update with spaces ")
            .tempdir()
            .unwrap();
        std::fs::create_dir(root.path().join("bin")).unwrap();
        for (name, built) in [
            ("symposium.exe", env!("CARGO_BIN_EXE_symposium")),
            ("cargo-agents.exe", env!("CARGO_BIN_EXE_cargo-agents")),
        ] {
            std::fs::copy(built, root.path().join("bin").join(name)).unwrap();
        }
        let config = root.path().join("config");
        std::fs::create_dir(&config).unwrap();
        std::fs::write(config.join("config.toml"), "auto-update = \"on\"\n").unwrap();
        Self { root }
    }

    fn binary(&self, name: &str) -> PathBuf {
        self.root.path().join("bin").join(name)
    }

    fn command(&self, name: &str, args: &[&str]) -> Command {
        let mut command = Command::new(self.binary(name));
        command
            .args(args)
            .current_dir(self.root.path())
            .env("SYMPOSIUM_HOME", self.root.path().join("config"))
            .env("SYMPOSIUM_CARGO", package().join("cargo.cmd"))
            .env("SELF_UPDATE_TEST_CARGO", env!("CARGO"))
            .env("SELF_UPDATE_TEST_PACKAGE", package())
            .env("SELF_UPDATE_TEST_ROOT", self.root.path())
            .env("SELF_UPDATE_TEST_TARGET", self.root.path().join("target"));
        self.isolate_cargo(&mut command);
        command
    }

    fn isolate_cargo(&self, command: &mut Command) {
        command
            .env("CARGO_HOME", self.root.path().join("cargo-home"))
            .env("CARGO_PROFILE_DEV_DEBUG", "0")
            .env("CARGO_INCREMENTAL", "0");
    }

    fn cargo_install(&self) -> Command {
        let mut command = Command::new(env!("CARGO"));
        command
            .current_dir(self.root.path())
            .args(["install", "--path"])
            .arg(package())
            .arg("--root")
            .arg(self.root.path())
            .arg("--target-dir")
            .arg(self.root.path().join("target"))
            .args(["--debug", "--offline", "--locked", "--force"]);
        self.isolate_cargo(&mut command);
        command
    }
}

/// This test reproduces the original failure.
#[tokio::test]
async fn real_cargo_cannot_replace_a_running_image() {
    let fixture = Fixture::new();
    assert_success(&output(fixture.cargo_install(), b"").await);
    let mut held = spawn(fixture.command("cargo-agents.exe", &["hold"]));
    let mut stdout = BufReader::new(held.stdout().take().unwrap());
    let mut line = String::new();
    timeout(Duration::from_secs(10), stdout.read_line(&mut line))
        .await
        .expect("replacement did not announce startup")
        .unwrap();
    assert_eq!(line.trim(), "replacement: hold");

    let install = output(fixture.cargo_install(), b"").await;
    drop(held.stdin().take());
    timeout(
        Duration::from_secs(10),
        Box::into_pin(held.wait_with_output()),
    )
    .await
    .expect("held replacement did not exit")
    .unwrap();

    assert!(!install.status.success(), "{install:?}");
    assert!(
        String::from_utf8_lossy(&install.stderr).contains("os error 5"),
        "{install:?}"
    );
    assert_success(&output(fixture.cargo_install(), b"").await);
}

/// Regression test.
#[tokio::test]
async fn real_cargo_updates_both_running_cli_executables() {
    for name in ["symposium.exe", "cargo-agents.exe"] {
        let fixture = Fixture::new();
        assert_success(&output(fixture.command(name, &["self-update"]), b"").await);
        let replacement = output(fixture.command(name, &["--version"]), b"").await;
        assert_eq!(replacement.status.code(), Some(42), "{replacement:?}");
        assert_eq!(replacement.stdout, b"replacement: --version\n");
    }
}

#[tokio::test]
async fn failed_cargo_install_restores_the_previous_executable() {
    let fixture = Fixture::new();
    let binary = fixture.binary("cargo-agents.exe");
    let original = std::fs::read(&binary).unwrap();
    let broken = fixture.root.path().join("broken");
    std::fs::create_dir(&broken).unwrap();
    std::fs::write(broken.join("Cargo.toml"), "not a Cargo manifest").unwrap();
    let mut command = fixture.command("cargo-agents.exe", &["self-update"]);
    command.env("SELF_UPDATE_TEST_PACKAGE", broken);

    let failed = output(command, b"").await;

    assert!(!failed.status.success(), "{failed:?}");
    assert!(String::from_utf8_lossy(&failed.stderr).contains("cargo install failed"));
    assert_eq!(std::fs::read(&binary).unwrap(), original);
    assert!(
        !binary
            .with_file_name("cargo-agents.exe.self-update-old")
            .exists()
    );
    assert_success(&output(fixture.command("cargo-agents.exe", &["self-update"]), b"").await);
}

#[tokio::test]
async fn automatic_update_preserves_arguments_and_exit_status() {
    let fixture = Fixture::new();
    let resumed = output(
        fixture.command("cargo-agents.exe", &["agents", "sync"]),
        b"",
    )
    .await;
    assert_eq!(resumed.status.code(), Some(42), "{resumed:?}");
    assert!(String::from_utf8_lossy(&resumed.stdout).contains("replacement: agents|sync"));
}

#[tokio::test]
async fn automatic_update_preserves_hook_input_and_exit_status() {
    let fixture = Fixture::new();
    let input = br#"{"hook_event_name":"SessionStart","session_id":"test"}"#;
    let resumed = output(
        fixture.command("symposium.exe", &["hook", "claude", "session-start"]),
        input,
    )
    .await;
    assert_eq!(resumed.status.code(), Some(42), "{resumed:?}");
    assert_eq!(
        String::from_utf8_lossy(&resumed.stdout),
        format!(
            "replacement: hook|claude|session-start\nstdin: {}\n",
            String::from_utf8_lossy(input)
        )
    );
}
