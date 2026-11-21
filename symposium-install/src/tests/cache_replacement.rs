//! A failed replacement must not remove an installation that already works.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;

use crate::{
    AcquiredSource, CargoSource, InstallContext, Source, UpdateLevel, acquire_source,
    complete_cargo_install, get_binary_cache_dir, git_cache_version, make_executable,
    platform_binary_exe,
};

const TOOL: &str = "cached-tool";

#[derive(Clone, Copy)]
enum Installer {
    Binstall,
    Cargo,
    Both,
}

impl Installer {
    fn command(self) -> &'static str {
        match self {
            Self::Binstall => "binstall",
            Self::Cargo => "install",
            Self::Both => "both",
        }
    }

    fn expected_attempts(self) -> &'static [&'static str] {
        match self {
            Self::Binstall | Self::Both => &["binstall"],
            Self::Cargo => &["binstall", "install"],
        }
    }
}

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

#[tokio::test]
async fn successful_binstall_removes_the_older_cached_version() {
    assert_successful_replacement(Installer::Binstall).await;
}

#[tokio::test]
async fn successful_cargo_fallback_removes_the_older_cached_version() {
    assert_successful_replacement(Installer::Cargo).await;
}

async fn assert_successful_replacement(installer: Installer) {
    let fixture = CachedInstallation::new();
    let before = run_cached_binary(&fixture.acquire("1.0.0").await.unwrap());
    fixture.installer_succeeds_on(installer);

    let replacement = fixture.acquire("2.0.0").await.unwrap();

    assert_eq!(run_cached_binary(&replacement), before);
    assert!(
        !fixture.executable.exists(),
        "successful replacement must clean up the old version"
    );
    let expected = installer.expected_attempts();
    assert_eq!(fixture.install_attempts(), expected);
    let cached = fixture.acquire("2.0.0").await.unwrap();
    assert_eq!(cached.executable(), replacement.executable());
    assert_eq!(
        fixture.install_attempts(),
        expected,
        "replacement must be reusable without reinstalling"
    );
}

#[tokio::test]
async fn successful_exit_without_a_binary_preserves_the_older_cached_version() {
    let fixture = CachedInstallation::new();
    let before = run_cached_binary(&fixture.acquire("1.0.0").await.unwrap());
    fixture.installer_succeeds_on(Installer::Both);
    std::fs::write(fixture.root.path().join("omit-binary"), "").unwrap();

    let error = fixture
        .acquire("2.0.0")
        .await
        .err()
        .expect("missing executable must fail acquisition");

    assert!(format!("{error:#}").contains("did not produce expected executable"));
    assert_eq!(fixture.install_attempts(), ["binstall", "install"]);
    assert_eq!(
        run_cached_binary(&fixture.acquire("1.0.0").await.unwrap()),
        before
    );
    assert_eq!(fixture.install_attempts(), ["binstall", "install"]);
}

#[cfg(windows)]
#[tokio::test]
async fn locked_old_binary_does_not_reject_a_successful_replacement() {
    use std::os::windows::fs::OpenOptionsExt;

    let fixture = CachedInstallation::new();
    let before = run_cached_binary(&fixture.acquire("1.0.0").await.unwrap());
    fixture.installer_succeeds_on(Installer::Binstall);
    let _locked = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&fixture.executable)
        .unwrap();

    let replacement = fixture.acquire("2.0.0").await.unwrap();

    assert_eq!(run_cached_binary(&replacement), before);
    assert!(
        fixture.executable.exists(),
        "locked old binary must be left for later cleanup"
    );
    assert_eq!(fixture.install_attempts(), ["binstall"]);
}

#[cfg(windows)]
#[test]
fn cleanup_continues_after_a_locked_version() {
    use std::os::windows::fs::OpenOptionsExt;

    let fixture = CachedInstallation::new();
    let locked_cache = fixture.cache_dir("1.0.0");
    let removable_cache = fixture.cache_dir("0.5.0");
    let current_cache = fixture.cache_dir("2.0.0");
    std::fs::create_dir_all(&removable_cache).unwrap();
    std::fs::write(removable_cache.join("old-artifact"), "").unwrap();
    std::fs::create_dir_all(&current_cache).unwrap();
    let _locked = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&fixture.executable)
        .unwrap();
    let mut entries = std::fs::read_dir(current_cache.parent().unwrap())
        .unwrap()
        .collect::<std::io::Result<Vec<_>>>()
        .unwrap();
    // Force the failure first: filesystem enumeration order is unspecified.
    entries.sort_by_key(|entry| entry.path() != locked_cache);

    crate::remove_older_cargo_entries(&current_cache, entries.into_iter().map(Ok));

    assert!(fixture.executable.is_file(), "locked binary must remain");
    assert!(
        !removable_cache.exists(),
        "a locked entry must not stop later cleanup"
    );
    assert!(
        current_cache.is_dir(),
        "cleanup must preserve the current version"
    );
}

#[test]
fn unnamed_install_with_an_empty_bin_preserves_the_previous_cache() {
    let fixture = CachedInstallation::new();
    let replacement = fixture.cache_dir("2.0.0");
    std::fs::create_dir_all(replacement.join("bin")).unwrap();

    let error = complete_cargo_install(Some(&replacement), None).unwrap_err();

    assert!(error.to_string().contains("did not produce any binaries"));
    assert!(
        fixture.executable.is_file(),
        "validation failure must preserve the old binary"
    );
}

#[test]
fn unnamed_install_with_a_binary_cleans_up_the_previous_cache() {
    let fixture = CachedInstallation::new();
    let replacement = fixture.cache_dir("2.0.0");
    std::fs::create_dir_all(replacement.join("bin")).unwrap();
    let binary = replacement
        .join("bin")
        .join(platform_binary_exe("another-tool"));
    std::fs::copy(&fixture.executable, &binary).unwrap();

    complete_cargo_install(Some(&replacement), None).unwrap();

    assert!(binary.is_file());
    assert!(
        !fixture.executable.exists(),
        "validated installation must clean up the old version"
    );
}

/// Owns an isolated cache and a Cargo stand-in that fails unless configured otherwise.
struct CachedInstallation {
    root: tempfile::TempDir,
    context: InstallContext,
    executable: PathBuf,
    git_url: String,
}

impl CachedInstallation {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("cached installation ")
            .tempdir()
            .unwrap();
        let cargo = fixture_cargo(root.path());
        let context = InstallContext::new(root.path().join("cache")).with_cargo_bin(cargo);
        // Successful acquisition probes Git HEAD. A nonexistent local repository
        // makes that probe fail immediately without contacting any server.
        let git_url = root
            .path()
            .join("missing repository")
            .to_str()
            .unwrap()
            .to_owned();
        let cache =
            get_binary_cache_dir(&context, TOOL, &git_cache_version(&git_url, Some("1.0.0")))
                .unwrap();
        let executable = cache.join("bin").join(platform_binary_exe(TOOL));
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        // Reuse a native executable on every platform; --list never runs its tests.
        std::fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
        std::fs::copy(
            &executable,
            root.path().join(platform_binary_exe("payload")),
        )
        .unwrap();
        make_executable(&executable).unwrap();
        Self {
            root,
            context,
            executable,
            git_url,
        }
    }

    async fn acquire(&self, version: &str) -> Result<AcquiredSource> {
        let source = Source::Cargo(
            CargoSource::new(TOOL)
                .with_git(&self.git_url)
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

    fn installer_succeeds_on(&self, installer: Installer) {
        std::fs::write(self.root.path().join("succeed-on"), installer.command()).unwrap();
    }

    fn cache_dir(&self, version: &str) -> PathBuf {
        get_binary_cache_dir(
            &self.context,
            TOOL,
            &git_cache_version(&self.git_url, Some(version)),
        )
        .unwrap()
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

fn fixture_cargo(root: &Path) -> PathBuf {
    let cargo = root.join(if cfg!(windows) { "cargo.cmd" } else { "cargo" });
    #[cfg(windows)]
    let script = include_str!("fixtures/cache-install.cmd").replace('\n', "\r\n");
    #[cfg(not(windows))]
    let script = include_str!("fixtures/cache-install.sh");
    std::fs::write(&cargo, script).unwrap();
    make_executable(&cargo).unwrap();
    cargo
}
