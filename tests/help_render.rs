//! End-to-end coverage for the audience-grouped `--help` renderer.

use symposium_testlib::TestMode;

fn redact(s: String) -> String {
    let no_version = s.replace(env!("CARGO_PKG_VERSION"), "$VERSION");
    // Strip ANSI escape sequences (clap styles leak through render_help).
    let ansi_re = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    ansi_re.replace_all(&no_version, "").into_owned()
}

#[tokio::test]
async fn cargo_agents_help_lists_plugin_vended() {
    symposium_testlib::with_fixture(
        TestMode::SimulationOnly,
        &["help_render0"],
        async |mut ctx| {
            let out = ctx.symposium(&["--help"]).await?;
            expect_test::expect![[r#"
                AI the Rust Way

                Usage: cargo agents [OPTIONS] [COMMAND]

                Commands for humans:
                init          Set up user-wide configuration
                install       Install plugin sources into user config
                plugin        Manage plugins
                self-update   Update symposium to the latest version
                status        Show resolved plugin/skill state for the current workspace
                sync          Synchronize skills with workspace dependencies
                uninstall     Uninstall plugin sources from user config

                Commands for agents:
                crate-info    Find crate sources
                example-tool  Analyze the example crate

                Options:
                  -q, --quiet    Suppress status output
                  -v, --verbose  Print detailed information about decisions made
                      --json     Output structured JSON report
                  -h, --help     Print help
                  -V, --version  Print version
            "#]]
            .assert_eq(&redact(out));
            Ok(())
        },
    )
    .await
    .unwrap();
}
