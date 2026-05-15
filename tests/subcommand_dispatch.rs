//! End-to-end coverage for plugin-vended `cargo agents <name>` dispatch.
//!
//! See `md/design/running-tests.md` for how to run these.
//!
use symposium_testlib::TestMode;

#[tokio::test]
async fn dispatches_known_subcommand() {
    symposium_testlib::with_fixture(
        TestMode::SimulationOnly,
        &["subcommands0"],
        async |mut ctx| {
            // `--version` is forwarded to the spawned `rustc`.
            ctx.symposium(&["greet", "--version"]).await?;
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn unknown_subcommand_errors() {
    symposium_testlib::with_fixture(
        TestMode::SimulationOnly,
        &["subcommands0"],
        async |mut ctx| {
            let err = ctx
                .symposium(&["definitely-not-a-real-subcommand"])
                .await
                .expect_err("dispatch should fail for an unknown name");
            let msg = err.to_string();
            assert!(
                msg.contains("definitely-not-a-real-subcommand"),
                "error should name the subcommand: {msg}"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}
