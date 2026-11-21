//! A failed replacement must not remove an installation that already works.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;

use crate::{
    AcquiredSource, CargoSource, InstallContext, Source, UpdateLevel, acquire_source,
    get_binary_cache_dir, git_cache_version, make_executable, platform_binary_exe,
};

const TOOL: &str = "cached-tool";
const GIT_URL: &str = "https://example.invalid/cached-tool";

#[tokio::test]
async fn failed_replacement_preserves_the_working_cached_binary() {
    // Establish a working cached version, without invoking an installer.
    let fixture = CachedInstallation::new();
    let original = fixture.acquire("1.0.0").await.unwrap();
    let before = run_cached_binary(&original);
    assert!(fixture.install_attempts().is_empty());

    // Both installation strategies fail when requesting the next version.
    let error = fixture
        .acquire("2.0.0")
        .await
        .err()
        .expect("replacement must fail");
    assert!(format!("{error:#}").contains("simulated replacement failure"));
    assert_eq!(fixture.install_attempts(), ["binstall", "install"]);

    // The original must still be discoverable and runnable without reinstalling.
    assert!(
        fixture.executable.is_file(),
        "failed replacement removed the working cached executable"
    );
    let original = fixture
        .acquire("1.0.0")
        .await
        .expect("previous version must remain available without reinstalling");
    assert_eq!(original.executable().as_ref(), Some(&fixture.executable));
    assert_eq!(run_cached_binary(&original), before);
    assert_eq!(fixture.install_attempts(), ["binstall", "install"]);
}

/// Owns an isolated cache and a Cargo stand-in that always fails.
struct CachedInstallation {
    root: tempfile::TempDir,
    context: InstallContext,
    executable: PathBuf,
}

impl CachedInstallation {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("cached installation ")
            .tempdir()
            .unwrap();
        let cargo = failing_cargo(root.path());
        let context = InstallContext::new(root.path().join("cache")).with_cargo_bin(cargo);
        let cache =
            get_binary_cache_dir(&context, TOOL, &git_cache_version(GIT_URL, Some("1.0.0")))
                .unwrap();
        let executable = cache.join("bin").join(platform_binary_exe(TOOL));
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        // Reuse a native executable on every platform; --list never runs its tests.
        std::fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
        make_executable(&executable).unwrap();
        Self {
            root,
            context,
            executable,
        }
    }

    async fn acquire(&self, version: &str) -> Result<AcquiredSource> {
        let source = Source::Cargo(
            CargoSource::new(TOOL)
                .with_git(GIT_URL)
                .with_version(version),
        );
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            acquire_source(&self.context, &source, Some(TOOL), UpdateLevel::None),
        )
        .await
        .expect("acquisition did not finish")
    }

    fn install_attempts(&self) -> Vec<String> {
        match std::fs::read_to_string(self.root.path().join("attempts")) {
            Ok(attempts) => attempts.lines().map(str::to_owned).collect(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => panic!("could not read installer attempts: {error}"),
        }
    }
}

fn run_cached_binary(acquired: &AcquiredSource) -> Vec<u8> {
    let output = Command::new(acquired.executable().unwrap())
        .arg("--list")
        .output()
        .expect("cached executable must be runnable");
    assert!(output.status.success(), "cached executable must succeed");
    output.stdout
}

fn failing_cargo(root: &Path) -> PathBuf {
    let cargo = root.join(if cfg!(windows) { "cargo.cmd" } else { "cargo" });
    #[cfg(windows)]
    let script = concat!(
        "@echo off\r\n",
        "echo %1>> \"%~dp0attempts\"\r\n",
        "echo simulated replacement failure 1>&2\r\n",
        "exit /b 23\r\n",
    );
    #[cfg(not(windows))]
    let script = concat!(
        "#!/usr/bin/env sh\n",
        "printf '%s\\n' \"$1\" >> \"$(dirname \"$0\")/attempts\"\n",
        "printf '%s\\n' 'simulated replacement failure' >&2\n",
        "exit 23\n",
    );
    std::fs::write(&cargo, script).unwrap();
    make_executable(&cargo).unwrap();
    cargo
}
