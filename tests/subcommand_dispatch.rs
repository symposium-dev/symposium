//! End-to-end coverage for plugin-vended `cargo agents <name>` dispatch.
//!
//! See `md/design/running-tests.md` for how to run these.
//!
use symposium_testlib::TestMode;

fn redact(s: String) -> String {
    let no_version = s.replace(env!("CARGO_PKG_VERSION"), "$VERSION");
    // Strip ANSI escape sequences (clap styles leak through render_help).
    let ansi_re = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    ansi_re.replace_all(&no_version, "").into_owned()
}

/// Dispatch a known subcommand (`greet` → `rustc --version`). The child's
/// stdout is captured and must contain "rustc".
#[tokio::test]
async fn dispatches_known_subcommand() {
    symposium_testlib::with_fixture(
        TestMode::SimulationOnly,
        &["subcommands0"],
        async |mut ctx| {
            let out = ctx.symposium(&["greet", "--version"]).await?;
            assert!(
                out.contains("rustc"),
                "expected rustc version output, got: {out}"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// `--help` in a workspace with a plugin subcommand shows it in the agents section.
#[tokio::test]
async fn help_shows_plugin_subcommand() {
    symposium_testlib::with_fixture(
        TestMode::SimulationOnly,
        &["subcommands0"],
        async |mut ctx| {
            let out = ctx.symposium(&["--help"]).await?;
            expect_test::expect![[r#"
                AI the Rust Way

                Usage: cargo agents [OPTIONS] [COMMAND]

                Commands for humans:
                init         Set up user-wide configuration
                plugin       Manage plugins
                self-update  Update symposium to the latest version
                sync         Synchronize skills with workspace dependencies

                Commands for agents:
                crate-info   Find crate sources
                greet        Print rustc version

                Options:
                      --update <UPDATE>  Control plugin source update behavior (none, check, fetch) [default: none] [possible values: none, check, fetch]
                  -q, --quiet            Suppress status output
                  -h, --help             Print help
                  -V, --version          Print version
            "#]]
            .assert_eq(&redact(out));
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
