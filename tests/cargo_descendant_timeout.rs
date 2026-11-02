//! Cancelling acquisition must stop the work spawned by either Cargo installer.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

use symposium_install::{CargoSource, InstallContext, Source, UpdateLevel, acquire_source};

fn assert_cancellation_stops_installer_descendants(binstall: bool) {
    let signals = tempfile::tempdir().unwrap();
    let child_script = signals.path().join("installer-child");
    let cargo_script = signals.path().join("mock-cargo");
    let started = signals.path().join("child-started");
    let release = signals.path().join("release");
    let completed = signals.path().join("child-completed");
    // The child can only perform its side effect after the test releases it.
    // Bound its polling as a failsafe so a panic cannot strand it.
    std::fs::write(
        &child_script,
        format!(
            "#!/bin/sh\ncd '{}' || exit 1\necho started > child-started\ni=0\nwhile [ ! -f release ]; do\n  i=$((i+1))\n  [ $i -lt 500 ] || exit 1\n  sleep 0.01\ndone\necho completed > child-completed\n",
            signals.path().display(),
        ),
    )
    .unwrap();
    // Exercise both the binstall fast path and the cargo install fallback.
    let reject_binstall = if binstall {
        ""
    } else {
        "[ \"$1\" = binstall ] && exit 1\n"
    };
    std::fs::write(
        &cargo_script,
        format!(
            "#!/bin/sh\n{reject_binstall}sh '{}' &\nwait\n",
            child_script.display(),
        ),
    )
    .unwrap();
    std::fs::set_permissions(&cargo_script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let context = InstallContext::new(signals.path().join("cache")).with_cargo_bin(cargo_script);
    let mut cargo = CargoSource::new("test-installer");
    cargo.global = true;
    let source = Source::Cargo(cargo);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let result = runtime.block_on(async {
        // This is the same cancellation boundary used by acquire_installation.
        tokio::time::timeout(
            Duration::from_secs(1),
            acquire_source(&context, &source, Some("test-installer"), UpdateLevel::None),
        )
        .await
    });

    // Release before any assertion, including on the failing implementation.
    std::fs::write(&release, "release").unwrap();
    drop(runtime);
    assert!(started.exists(), "the installer child must actually start");
    assert!(result.is_err(), "acquisition must hit its deadline");
    let observation_deadline = Instant::now() + Duration::from_millis(500);
    while !completed.exists() && Instant::now() < observation_deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !completed.exists(),
        "Cargo installer child mutated files after acquisition timed out"
    );
}

#[test]
fn timed_out_binstall_stops_installer_descendants() {
    assert_cancellation_stops_installer_descendants(true);
}

#[test]
fn timed_out_cargo_install_stops_installer_descendants() {
    assert_cancellation_stops_installer_descendants(false);
}
