//! End-to-end coverage for the audience-grouped `--help` renderer.

use symposium_testlib::TestMode;

#[tokio::test]
async fn cargo_agents_help_lists_plugin_vended() {
    symposium_testlib::with_fixture(
        TestMode::SimulationOnly,
        &["help_render0"],
        async |mut ctx| {
            let out = ctx.symposium(&["--help"]).await?;
            assert!(
                out.contains("Commands for humans:"),
                "missing humans heading: {out}"
            );
            assert!(
                out.contains("Commands for agents:"),
                "missing agents heading: {out}"
            );
            assert!(
                out.contains("example-tool"),
                "plugin-vended subcommand missing: {out}"
            );
            assert!(
                out.contains("crate-info"),
                "crate-info should be un-hidden: {out}"
            );
            assert!(
                !out.contains("\n  hook  "),
                "hook should remain hidden: {out}"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}
