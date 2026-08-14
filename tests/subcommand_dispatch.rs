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
                mcp          Manage remote MCP servers
                plugin       Manage plugins
                search       Search configured registries for plugins
                self-update  Update symposium to the latest version
                status       Show which plugins are enabled for this workspace, and why
                sync         Synchronize skills with workspace dependencies
                telemetry    Manage opt-in usage telemetry (status, enable, disable, show)
                use          Enable a plugin by name and sync it into the workspace

                Commands for agents:
                crate-info   Find crate sources
                greet        Print rustc version

                Options:
                      --update <UPDATE>  Control plugin source update behavior (none, check, fetch) [default: none] [possible values: none, check, fetch]
                  -q, --quiet            Suppress status output
                  -v, --verbose          Print detailed information about decisions made
                      --json             Output structured JSON report
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

/// A subcommand declared by a crate reached through a `[[plugins]]` chained
/// reference is dispatchable — crate-sourced subcommands flow through the active
/// plugin set, not just skills. `crate-f` vends `facet-tool` (→ `rustc`); the
/// child's stdout must contain "rustc".
#[tokio::test]
async fn dispatches_subcommand_from_chained_crate() {
    symposium_testlib::with_fixture(
        TestMode::SimulationOnly,
        &["crate-facets0"],
        async |mut ctx| {
            let out = ctx.symposium(&["facet-tool", "--version"]).await?;
            assert!(
                out.contains("rustc"),
                "expected rustc version output from the chained crate's subcommand, got: {out}"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// The same crate-sourced subcommand appears in `--help`, under the agents
/// section, proving help discovery walks the active plugin set.
#[tokio::test]
async fn help_shows_chained_crate_subcommand() {
    symposium_testlib::with_fixture(
        TestMode::SimulationOnly,
        &["crate-facets0"],
        async |mut ctx| {
            let out = redact(ctx.symposium(&["--help"]).await?);
            assert!(
                out.contains("facet-tool"),
                "chained crate's subcommand should be listed in help:\n{out}"
            );
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
