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

const NO_BINSTALL: &str = "if [ \"$1\" = binstall ]; then\n\
                           echo 'error: no such command: `binstall`' >&2\n\
                           exit 101\n\
                           fi\n";

const BINSTALL_WITHOUT_PREBUILT: &str = "if [ \"$1\" = binstall ]; then\n\
                                         case \" $* \" in *' --disable-strategies compile '*)\n\
                                         echo 'Fallback to cargo-install is disabled' >&2\n\
                                         exit 94;;\n\
                                         esac\n\
                                         shift; set -- install \"$@\"\n\
                                         fi\n";

const BINSTALL_PREBUILT: &str = "if [ \"$1\" = binstall ]; then shift; set -- install \"$@\"; fi\n";

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

async fn installation_events<F>(f: F) -> Vec<serde_json::Value>
where
    F: AsyncFnOnce(),
{
    use symposium::report::{ReportLayer, ReportMode};
    use tracing_subscriber::layer::SubscriberExt;

    symposium_testlib::install_tracing_baseline();
    let (layer, handle) = ReportLayer::new(ReportMode::Json, tracing::Level::INFO);
    let subscriber = tracing_subscriber::registry().with(layer);
    let guard = tracing::subscriber::set_default(subscriber);
    f().await;
    drop(guard);
    handle
        .drain()
        .into_iter()
        .filter(|event| {
            event["kind"]
                .as_str()
                .is_some_and(|kind| kind.starts_with("installation_"))
        })
        .collect()
}

async fn installation_reports<F>(f: F) -> Vec<String>
where
    F: AsyncFnOnce(),
{
    installation_events(f)
        .await
        .iter()
        .filter_map(|event| event["kind"].as_str())
        .map(str::to_string)
        .collect()
}

async fn installation_texts<F>(f: F) -> Vec<String>
where
    F: AsyncFnOnce(),
{
    installation_events(f)
        .await
        .into_iter()
        .map(|event| {
            serde_json::from_value::<symposium::report::ReportEvent>(event)
                .unwrap()
                .format_human()
        })
        .collect()
}

const STARTED: &str = "installing helper for the hooks of plugin helper-kit; without a prebuilt \
                       binary it is built from source, which can take minutes";

#[tokio::test(flavor = "multi_thread")]
async fn sync_says_why_a_helper_was_built_from_source_without_cargo_binstall() {
    with_fixture(
        TestMode::SimulationOnly,
        &["hook-helper0"],
        async |mut ctx| {
            ctx.set_mock_cargo(&mock_cargo(&ctx, &format!("{NO_BINSTALL}{INSTALL}")));

            let texts = installation_texts(async || {
                ctx.symposium(&["sync"]).await.unwrap();
            })
            .await;
            assert_eq!(
                texts,
                [
                    STARTED,
                    "installed helper for the hooks of plugin helper-kit, built from source \
                     because cargo-binstall is not installed; after `cargo install \
                     cargo-binstall` plugins that publish prebuilt binaries install in seconds",
                ]
            );
            assert_eq!(cargo_calls(&ctx), ["binstall", "install"]);
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_says_why_a_helper_was_built_from_source_with_cargo_binstall() {
    with_fixture(
        TestMode::SimulationOnly,
        &["hook-helper0"],
        async |mut ctx| {
            ctx.set_mock_cargo(&mock_cargo(
                &ctx,
                &format!("{BINSTALL_WITHOUT_PREBUILT}{INSTALL}"),
            ));

            let texts = installation_texts(async || {
                ctx.symposium(&["sync"]).await.unwrap();
            })
            .await;
            assert_eq!(
                texts,
                [
                    STARTED,
                    "installed helper for the hooks of plugin helper-kit, built from source \
                     because the plugin publishes no prebuilt binary for this platform",
                ]
            );
            assert_eq!(cargo_calls(&ctx), ["binstall", "install"]);
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_mentions_no_source_build_when_cargo_binstall_installs_the_helper() {
    with_fixture(
        TestMode::SimulationOnly,
        &["hook-helper0"],
        async |mut ctx| {
            ctx.set_mock_cargo(&mock_cargo(&ctx, &format!("{BINSTALL_PREBUILT}{INSTALL}")));

            let texts = installation_texts(async || {
                ctx.symposium(&["sync"]).await.unwrap();
            })
            .await;
            assert_eq!(
                texts,
                [
                    STARTED,
                    "installed helper for the hooks of plugin helper-kit"
                ]
            );
            assert_eq!(cargo_calls(&ctx), ["binstall"]);
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_installs_the_helper_and_a_second_sync_does_nothing() {
    with_fixture(
        TestMode::SimulationOnly,
        &["hook-helper0"],
        async |mut ctx| {
            ctx.set_mock_cargo(&installing_cargo(&ctx));

            let first = installation_reports(async || {
                ctx.symposium(&["sync"]).await.unwrap();
            })
            .await;
            assert_eq!(first, ["installation_started", "installation_installed"]);
            assert_eq!(cargo_calls(&ctx), ["binstall", "install"]);

            let second = installation_reports(async || {
                ctx.symposium(&["sync"]).await.unwrap();
            })
            .await;
            assert!(second.is_empty(), "{second:?}");
            assert_eq!(cargo_calls(&ctx), ["binstall", "install"]);

            let event = pre_tool_use(&mut ctx).await?;
            assert!(event.has_context_containing(HELPER_CONTEXT), "{event:#?}");
            assert_eq!(cargo_calls(&ctx), ["binstall", "install"]);
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_installs_the_helper_a_hook_could_not() {
    with_fixture(
        TestMode::SimulationOnly,
        &["hook-helper0"],
        async |mut ctx| {
            ctx.set_mock_cargo(&failing_cargo(&ctx));
            pre_tool_use(&mut ctx).await?;
            assert_eq!(failure_records(&ctx).len(), 1);

            ctx.set_mock_cargo(&installing_cargo(&ctx));
            ctx.symposium(&["sync"]).await?;
            assert!(failure_records(&ctx).is_empty());

            let event = pre_tool_use(&mut ctx).await?;
            assert!(event.has_context_containing(HELPER_CONTEXT), "{event:#?}");
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_reports_a_failed_install_and_succeeds() {
    with_fixture(
        TestMode::SimulationOnly,
        &["hook-helper0"],
        async |mut ctx| {
            ctx.set_mock_cargo(&failing_cargo(&ctx));

            let reports = installation_reports(async || {
                ctx.symposium(&["sync"]).await.unwrap();
            })
            .await;
            assert_eq!(reports, ["installation_started", "installation_failed"]);
            assert_eq!(cargo_calls(&ctx), ["binstall", "install"]);

            pre_tool_use(&mut ctx).await?;
            assert_eq!(
                cargo_calls(&ctx),
                ["binstall", "install"],
                "a hook event started the installer again"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn use_installs_the_helper_of_the_plugin_it_enables() {
    with_fixture(
        TestMode::SimulationOnly,
        &["hook-helper0"],
        async |mut ctx| {
            let manifest = ctx
                .sym
                .config_dir()
                .join("plugins/helper-kit/Symposium.toml");
            let dormant = std::fs::read_to_string(&manifest)?.replace("depends-on = [\"*\"]\n", "");
            std::fs::write(&manifest, dormant)?;
            ctx.set_mock_cargo(&installing_cargo(&ctx));

            let reports = installation_reports(async || {
                ctx.symposium(&["use", "helper-kit"]).await.unwrap();
            })
            .await;
            assert_eq!(reports, ["installation_started", "installation_installed"]);

            let event = pre_tool_use(&mut ctx).await?;
            assert!(event.has_context_containing(HELPER_CONTEXT), "{event:#?}");
            assert_eq!(cargo_calls(&ctx), ["binstall", "install"]);
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn use_global_outside_a_workspace_installs_the_helper() {
    with_fixture(
        TestMode::SimulationOnly,
        &["hook-helper0"],
        async |mut ctx| {
            std::fs::remove_file(ctx.tempdir.join("Cargo.toml"))?;
            ctx.workspace_root = None;
            let manifest = ctx
                .sym
                .config_dir()
                .join("plugins/helper-kit/Symposium.toml");
            let gated = std::fs::read_to_string(&manifest)?
                .replace("depends-on = [\"*\"]", "depends-on = [\"tokio\"]");
            std::fs::write(&manifest, gated)?;
            ctx.set_mock_cargo(&installing_cargo(&ctx));

            let reports = installation_reports(async || {
                ctx.symposium(&["use", "--global", "helper-kit"])
                    .await
                    .unwrap();
            })
            .await;
            assert_eq!(reports, ["installation_started", "installation_installed"]);
            assert_eq!(cargo_calls(&ctx), ["binstall", "install"]);
            Ok(())
        },
    )
    .await
    .unwrap();
}
