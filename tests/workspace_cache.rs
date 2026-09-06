use anyhow::{Context, Result};
use std::{
    fs::{self, File, FileTimes},
    path::Path,
    sync::Arc,
    time::Duration,
};
use symposium::pm::{LoadedWorkspace, WorkspaceDeps};
use symposium_testlib::{TestMode, with_fixture};

const CARGO_CALL_LOG: &str = ".symposium-cargo-calls";
const WORKSPACE_FIXTURE: &[&str] = &["workspace-cache0"];
const RECORDING_CARGO: &str = indoc::indoc! {r#"
    #!/bin/sh
    printf '%s\n' "$1" >> "${0%/*}/.symposium-cargo-calls"
    exec cargo "$@"
"#};
const METADATA_REJECTING_CARGO: &str = indoc::indoc! {r#"
    #!/bin/sh
    printf '%s\n' "$1" >> "${0%/*}/.symposium-cargo-calls"
    if [ "$1" = "metadata" ]; then
        exit 1
    fi
    exec cargo "$@"
"#};

fn read_cargo_calls(call_log: &Path) -> Result<String> {
    fs::read_to_string(call_log)
        .with_context(|| format!("reading Cargo call log `{}`", call_log.display()))
}

fn load_or_panic<'a>(
    resolver: &'a WorkspaceDeps,
    call_log: &Path,
    failure: &str,
) -> &'a Arc<LoadedWorkspace> {
    resolver.load().unwrap_or_else(|| {
        panic!(
            "{failure}; Cargo calls:\n{}",
            read_cargo_calls(call_log).unwrap_or_else(|error| format!("<{error:#}>"))
        )
    })
}

fn advance_modified_time(path: &Path) -> Result<()> {
    let modified = fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .with_context(|| format!("reading modification time for `{}`", path.display()))?;
    let file = File::options().write(true).open(path).with_context(|| {
        format!(
            "opening `{}` to advance its modification time",
            path.display()
        )
    })?;
    let times = FileTimes::new().set_modified(modified + Duration::from_secs(1));
    file.set_times(times)
        .with_context(|| format!("advancing modification time for `{}`", path.display()))
}

#[tokio::test]
async fn repeated_loads_run_cache_miss_commands_once() -> Result<()> {
    with_fixture(
        TestMode::SimulationOnly,
        WORKSPACE_FIXTURE,
        async |mut context| {
            let call_log = context
                .set_mock_cargo(RECORDING_CARGO)
                .with_file_name(CARGO_CALL_LOG);
            let workspace = context
                .workspace_root
                .as_deref()
                .context("workspace-cache0 must provide a workspace root")?;
            let resolver = context.sym.workspace_deps(workspace);

            let first_load = load_or_panic(
                &resolver,
                &call_log,
                "initial workspace dependency load failed",
            );
            let second_load = load_or_panic(
                &resolver,
                &call_log,
                "memoized workspace dependency load failed",
            );
            assert!(
                Arc::ptr_eq(first_load, second_load),
                "repeated loads should return the memoized workspace"
            );

            let calls = read_cargo_calls(&call_log)?;
            assert_eq!(calls, "locate-project\nmetadata\n");

            Ok(())
        },
    )
    .await
}

#[tokio::test]
async fn new_resolver_uses_the_disk_cache_without_metadata() -> Result<()> {
    with_fixture(
        TestMode::SimulationOnly,
        WORKSPACE_FIXTURE,
        async |mut context| {
            let call_log = context
                .set_mock_cargo(RECORDING_CARGO)
                .with_file_name(CARGO_CALL_LOG);
            // Own the path because replacing mock Cargo later mutably borrows `context`.
            let workspace = context
                .workspace_root
                .clone()
                .context("workspace-cache0 must provide a workspace root")?;
            let first_resolver = context.sym.workspace_deps(&workspace);

            load_or_panic(
                &first_resolver,
                &call_log,
                "cache-populating workspace dependency load failed",
            );

            fs::remove_file(&call_log).context("clearing Cargo call log after cache population")?;
            let call_log = context
                .set_mock_cargo(METADATA_REJECTING_CARGO)
                .with_file_name(CARGO_CALL_LOG);

            let second_resolver = context.sym.workspace_deps(&workspace);
            load_or_panic(
                &second_resolver,
                &call_log,
                "new resolver did not use the disk cache",
            );

            let calls = read_cargo_calls(&call_log)?;
            assert_eq!(calls, "locate-project\n");

            Ok(())
        },
    )
    .await
}

#[tokio::test]
async fn changed_cargo_lock_invalidates_disk_cache() -> Result<()> {
    with_fixture(
        TestMode::SimulationOnly,
        WORKSPACE_FIXTURE,
        async |mut context| {
            let call_log = context
                .set_mock_cargo(RECORDING_CARGO)
                .with_file_name(CARGO_CALL_LOG);
            let workspace = context
                .workspace_root
                .as_deref()
                .context("workspace-cache0 must provide a workspace root")?;
            let first_resolver = context.sym.workspace_deps(workspace);

            load_or_panic(
                &first_resolver,
                &call_log,
                "cache-populating workspace dependency load failed",
            );

            fs::remove_file(&call_log).context("clearing Cargo call log after cache population")?;
            advance_modified_time(&workspace.join("Cargo.lock"))?;

            let second_resolver = context.sym.workspace_deps(workspace);
            load_or_panic(
                &second_resolver,
                &call_log,
                "workspace dependency reload failed after Cargo.lock changed",
            );

            let calls = read_cargo_calls(&call_log)?;
            assert_eq!(calls, "locate-project\nmetadata\n");

            Ok(())
        },
    )
    .await
}
