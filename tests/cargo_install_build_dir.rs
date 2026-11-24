#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use symposium_install::{CargoSource, InstallContext, Source, UpdateLevel, acquire_source};

const TOOL: &str = "build-dir-probe";

fn write_mock_cargo(root: &Path) -> PathBuf {
    let cargo = root.join("mock-cargo");
    std::fs::write(
        &cargo,
        format!(
            "#!/bin/sh\n\
             [ \"$1\" = install ] || exit 1\n\
             cd '{root}' || exit 1\n\
             echo \"$CARGO_TARGET_DIR\" >> build-dirs\n\
             mkdir -p \"$CARGO_TARGET_DIR\" || exit 1\n\
             echo compiled >> \"$CARGO_TARGET_DIR/artifact\"\n\
             echo scratch > \"$TMPDIR/compiler-temp\" || exit 1\n\
             [ ! -f fail ] || exit 101\n\
             if [ ! -f finish ]; then\n\
               i=0\n\
               while [ $i -lt 1000 ]; do i=$((i+1)); sleep 0.01; done\n\
               exit 1\n\
             fi\n\
             install_root=\n\
             while [ \"$#\" -gt 0 ]; do\n\
               if [ \"$1\" = --root ]; then shift; install_root=$1; fi\n\
               shift\n\
             done\n\
             mkdir -p \"$install_root/bin\" || exit 1\n\
             : > \"$install_root/bin/{TOOL}\"\n",
            root = root.display(),
        ),
    )
    .unwrap();
    std::fs::set_permissions(&cargo, std::fs::Permissions::from_mode(0o755)).unwrap();
    cargo
}

fn write_slow_mock_cargo(root: &Path) -> PathBuf {
    let cargo = root.join("mock-cargo");
    std::fs::write(
        &cargo,
        format!(
            "#!/bin/sh\n\
             [ \"$1\" = install ] || exit 1\n\
             cd '{root}' || exit 1\n\
             echo build >> builds\n\
             mkdir -p \"$CARGO_TARGET_DIR\" || exit 1\n\
             artifact=\"$CARGO_TARGET_DIR/artifact-$$\"\n\
             : > \"$artifact\" || exit 1\n\
             if mkdir first-build 2>/dev/null; then sleep 0.5; else sleep 1.5; fi\n\
             [ -f \"$artifact\" ] || exit 1\n\
             install_root=\n\
             while [ \"$#\" -gt 0 ]; do\n\
               if [ \"$1\" = --root ]; then shift; install_root=$1; fi\n\
               shift\n\
             done\n\
             mkdir -p \"$install_root/bin\" || exit 1\n\
             : > \"$install_root/bin/{TOOL}\"\n",
            root = root.display(),
        ),
    )
    .unwrap();
    std::fs::set_permissions(&cargo, std::fs::Permissions::from_mode(0o755)).unwrap();
    cargo
}

fn build_dirs(root: &Path) -> Vec<PathBuf> {
    std::fs::read_to_string(root.join("build-dirs"))
        .unwrap_or_default()
        .lines()
        .map(PathBuf::from)
        .collect()
}

#[tokio::test]
async fn stopped_cargo_build_stays_in_the_cache_until_an_install_succeeds() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let cache = root.join("cache");
    let context = InstallContext::new(cache.clone()).with_cargo_bin(write_mock_cargo(root));
    let source =
        Source::Cargo(CargoSource::new(TOOL).with_git("file:///build-dir-probe-is-never-cloned"));

    let stopped = tokio::time::timeout(
        Duration::from_secs(1),
        acquire_source(&context, &source, Some(TOOL), UpdateLevel::None),
    )
    .await;
    assert!(stopped.is_err(), "the build must still be running");
    let stopped_in = build_dirs(root);
    let [build_dir] = stopped_in.as_slice() else {
        panic!("expected one build, got {stopped_in:?}");
    };
    assert!(
        build_dir.starts_with(&cache),
        "built outside the cache: {}",
        build_dir.display()
    );
    assert!(build_dir.join("artifact").is_file());
    assert!(
        build_dir.join("tmp/compiler-temp").is_file(),
        "the build wrote its temporary files outside its directory"
    );

    std::fs::write(root.join("finish"), "").unwrap();
    acquire_source(&context, &source, Some(TOOL), UpdateLevel::None)
        .await
        .unwrap();
    assert_eq!(build_dirs(root), [build_dir.clone(), build_dir.clone()]);
    assert!(
        !build_dir.exists(),
        "the build directory outlived the install"
    );
}

#[tokio::test]
async fn cargo_build_that_fails_is_removed_from_the_cache() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let context = InstallContext::new(root.join("cache")).with_cargo_bin(write_mock_cargo(root));
    let source =
        Source::Cargo(CargoSource::new(TOOL).with_git("file:///build-dir-probe-is-never-cloned"));
    std::fs::write(root.join("fail"), "").unwrap();

    let failed = acquire_source(&context, &source, Some(TOOL), UpdateLevel::None).await;
    assert!(failed.is_err(), "the build must fail");
    let failed_in = build_dirs(root);
    let [build_dir] = failed_in.as_slice() else {
        panic!("expected one build, got {failed_in:?}");
    };
    assert!(
        !build_dir.exists(),
        "the failed build stayed in the cache: {}",
        build_dir.display()
    );
}

#[tokio::test]
async fn global_cargo_build_stays_in_the_cache() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let cache = root.join("cache");
    let context = InstallContext::new(cache.clone()).with_cargo_bin(write_mock_cargo(root));
    std::fs::write(root.join("fail"), "").unwrap();
    let elsewhere = root.join("elsewhere");
    std::fs::create_dir_all(elsewhere.join("build")).unwrap();

    for crate_name in [TOOL, elsewhere.to_str().unwrap()] {
        let mut cargo = CargoSource::new(crate_name);
        cargo.global = true;
        let failed = acquire_source(
            &context,
            &Source::Cargo(cargo),
            Some(TOOL),
            UpdateLevel::None,
        )
        .await;
        assert!(failed.is_err(), "the build must fail");
    }
    assert!(
        elsewhere.join("build").is_dir(),
        "a global install removed a directory outside the cache"
    );
    let built_in = build_dirs(root);
    let [build_dir] = built_in.as_slice() else {
        panic!("expected one build, got {built_in:?}");
    };
    assert!(
        build_dir.starts_with(&cache),
        "built outside the cache: {}",
        build_dir.display()
    );
}

#[tokio::test]
async fn concurrent_acquisitions_of_one_crate_build_it_once() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let context =
        InstallContext::new(root.join("cache")).with_cargo_bin(write_slow_mock_cargo(root));
    let source =
        Source::Cargo(CargoSource::new(TOOL).with_git("file:///build-dir-probe-is-never-cloned"));

    let (first, second) = tokio::join!(
        acquire_source(&context, &source, Some(TOOL), UpdateLevel::None),
        acquire_source(&context, &source, Some(TOOL), UpdateLevel::None),
    );
    first.unwrap();
    second.unwrap();
    let builds = std::fs::read_to_string(root.join("builds")).unwrap();
    assert_eq!(builds.lines().count(), 1, "the crate was built again");
}
