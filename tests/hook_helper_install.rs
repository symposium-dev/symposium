#![cfg(unix)]

use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use serde_json::json;
use symposium::hook_schema::HookAgent;
use symposium_testlib::{HookStep, SubmitResult, TestContext, TestMode, with_fixture};

const HELPER_CONTEXT: &str = "helper-ran";

fn mock_cargo(ctx: &TestContext, installer: &str) -> String {
    format!(
        "#!/bin/sh\n\
         case \"$1\" in binstall|install) ;; *) exec cargo \"$@\";; esac\n\
         echo \"$1\" >> '{dir}/cargo-calls'\n\
         {installer}",
        dir = ctx.tempdir.display(),
    )
}

const HANG: &str = "i=0\n\
                    while [ $i -lt 2000 ]; do i=$((i+1)); sleep 0.01; done\n\
                    exit 1\n";

const INSTALL: &str = "[ \"$1\" = install ] || exit 1\n\
                       install_root=\n\
                       while [ \"$#\" -gt 0 ]; do\n\
                         if [ \"$1\" = --root ]; then shift; install_root=$1; fi\n\
                         shift\n\
                       done\n\
                       mkdir -p \"$install_root/bin\" || exit 1\n\
                       cat > \"$install_root/bin/helper-kit-hooks\" <<'HELPER'\n\
                       #!/bin/sh\n\
                       echo '{\"PreToolUse\":{\"additionalContext\":\"helper-ran\"}}'\n\
                       HELPER\n\
                       chmod +x \"$install_root/bin/helper-kit-hooks\"\n";

fn failing_cargo(ctx: &TestContext) -> String {
    mock_cargo(ctx, "exit 1\n")
}

fn hanging_cargo(ctx: &TestContext) -> String {
    mock_cargo(ctx, HANG)
}

fn installing_cargo(ctx: &TestContext) -> String {
    mock_cargo(ctx, INSTALL)
}

fn installing_then_hanging_cargo(ctx: &TestContext) -> String {
    mock_cargo(ctx, &format!("{INSTALL}{HANG}"))
}

fn cargo_calls(ctx: &TestContext) -> Vec<String> {
    std::fs::read_to_string(ctx.tempdir.join("cargo-calls"))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

fn failure_records(ctx: &TestContext) -> Vec<PathBuf> {
    let mut records: Vec<PathBuf> = std::fs::read_dir(ctx.sym.cache_dir().join("binaries"))
        .into_iter()
        .flatten()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "failed")
        })
        .collect();
    records.sort();
    records
}

async fn pre_tool_use(ctx: &mut TestContext) -> anyhow::Result<SubmitResult> {
    ctx.prompt_or_hook(
        "ignored",
        &[HookStep::PreToolUse {
            tool_name: "Bash".to_string(),
            tool_input: json!({"command": "ls"}),
        }],
        HookAgent::Claude,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn helper_that_installs_inside_the_budget_runs_on_the_first_event() {
    with_fixture(
        TestMode::SimulationOnly,
        &["hook-helper0"],
        async |mut ctx| {
            ctx.set_mock_cargo(&installing_cargo(&ctx));

            let first = pre_tool_use(&mut ctx).await?;
            assert!(first.has_context_containing(HELPER_CONTEXT), "{first:#?}");
            assert_eq!(cargo_calls(&ctx), ["binstall", "install"]);

            let second = pre_tool_use(&mut ctx).await?;
            assert!(second.has_context_containing(HELPER_CONTEXT), "{second:#?}");
            assert_eq!(cargo_calls(&ctx), ["binstall", "install"]);
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn install_past_the_hook_budget_is_not_attempted_on_the_next_event() {
    with_fixture(
        TestMode::SimulationOnly,
        &["hook-helper0"],
        async |mut ctx| {
            ctx.set_mock_cargo(&hanging_cargo(&ctx));

            let overrun = pre_tool_use(&mut ctx).await?;
            assert!(!overrun.has_context_containing(HELPER_CONTEXT));
            assert_eq!(cargo_calls(&ctx), ["binstall"]);
            assert_eq!(failure_records(&ctx).len(), 1);

            let started = Instant::now();
            pre_tool_use(&mut ctx).await?;
            assert_eq!(
                cargo_calls(&ctx),
                ["binstall"],
                "the next event started the installer again"
            );
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "the next event waited on the install: {:?}",
                started.elapsed()
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn helper_an_overrun_attempt_left_installed_runs_on_the_next_event() {
    with_fixture(
        TestMode::SimulationOnly,
        &["hook-helper0"],
        async |mut ctx| {
            ctx.set_mock_cargo(&installing_then_hanging_cargo(&ctx));

            let overrun = pre_tool_use(&mut ctx).await?;
            assert!(!overrun.has_context_containing(HELPER_CONTEXT));
            assert_eq!(failure_records(&ctx).len(), 1);

            let next = pre_tool_use(&mut ctx).await?;
            assert!(next.has_context_containing(HELPER_CONTEXT), "{next:#?}");
            assert_eq!(cargo_calls(&ctx), ["binstall", "install"]);
            assert!(failure_records(&ctx).is_empty());
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn failed_install_is_attempted_again_only_outside_a_hook() {
    with_fixture(
        TestMode::SimulationOnly,
        &["hook-helper0"],
        async |mut ctx| {
            ctx.set_mock_cargo(&failing_cargo(&ctx));
            pre_tool_use(&mut ctx).await?;
            assert_eq!(cargo_calls(&ctx), ["binstall", "install"]);
            assert_eq!(failure_records(&ctx).len(), 1);

            ctx.set_mock_cargo(&installing_cargo(&ctx));
            let skipped = pre_tool_use(&mut ctx).await?;
            assert!(!skipped.has_context_containing(HELPER_CONTEXT));
            assert_eq!(
                cargo_calls(&ctx),
                ["binstall", "install"],
                "a hook event started the installer again"
            );

            ctx.symposium(&["helper-kit-probe"]).await?;
            assert_eq!(
                cargo_calls(&ctx),
                ["binstall", "install", "binstall", "install"]
            );
            assert!(failure_records(&ctx).is_empty());

            let installed = pre_tool_use(&mut ctx).await?;
            assert!(
                installed.has_context_containing(HELPER_CONTEXT),
                "{installed:#?}"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

fn age_failure_records(ctx: &TestContext, hours: u64) {
    for record in failure_records(ctx) {
        std::fs::File::options()
            .write(true)
            .open(record)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(hours * 60 * 60))
            .unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn hook_attempts_a_failed_install_again_once_a_day() {
    with_fixture(
        TestMode::SimulationOnly,
        &["hook-helper0"],
        async |mut ctx| {
            ctx.set_mock_cargo(&failing_cargo(&ctx));
            pre_tool_use(&mut ctx).await?;
            assert_eq!(cargo_calls(&ctx), ["binstall", "install"]);

            age_failure_records(&ctx, 23);
            pre_tool_use(&mut ctx).await?;
            assert_eq!(
                cargo_calls(&ctx),
                ["binstall", "install"],
                "a hook attempted the install again within a day"
            );

            age_failure_records(&ctx, 25);
            pre_tool_use(&mut ctx).await?;
            assert_eq!(
                cargo_calls(&ctx),
                ["binstall", "install", "binstall", "install"],
                "a hook did not attempt the install again after a day"
            );

            pre_tool_use(&mut ctx).await?;
            assert_eq!(
                cargo_calls(&ctx),
                ["binstall", "install", "binstall", "install"],
                "the failed retry did not start a new day"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn overrun_installs_of_two_sources_of_one_crate_are_each_attempted_once() {
    with_fixture(
        TestMode::SimulationOnly,
        &["hook-helper0", "hook-helper-pinned0"],
        async |mut ctx| {
            ctx.set_mock_cargo(&hanging_cargo(&ctx));

            pre_tool_use(&mut ctx).await?;
            assert_eq!(cargo_calls(&ctx), ["binstall"]);

            pre_tool_use(&mut ctx).await?;
            assert_eq!(cargo_calls(&ctx), ["binstall", "binstall"]);

            let started = Instant::now();
            pre_tool_use(&mut ctx).await?;
            assert_eq!(
                cargo_calls(&ctx),
                ["binstall", "binstall"],
                "an event attempted an install that already overran"
            );
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "the event waited on an install: {:?}",
                started.elapsed()
            );
            assert_eq!(failure_records(&ctx).len(), 2);
            Ok(())
        },
    )
    .await
    .unwrap();
}
