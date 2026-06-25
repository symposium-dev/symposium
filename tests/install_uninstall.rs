//! Config-only install/uninstall command tests.

use indoc::indoc;
use symposium::config::{CargoDependencySpec, Config};
use symposium_testlib::{TestMode, with_fixture};

fn parse_config(ctx: &symposium_testlib::TestContext) -> Config {
    let path = ctx.sym.config_dir().join("config.toml");
    let contents = std::fs::read_to_string(path).unwrap();
    toml::from_str(&contents).unwrap()
}

#[tokio::test]
async fn install_registry_crates_updates_installed_crates() {
    with_fixture(TestMode::SimulationOnly, &[], async |mut ctx| {
        let output = ctx
            .symposium(&["install", "foo", "bar@1", "baz@=1.2.3"])
            .await?;
        assert!(output.contains("crate source installed: foo"));
        assert!(output.contains("crate source installed: bar"));
        assert!(output.contains("crate source installed: baz"));

        let config = parse_config(&ctx);
        assert_eq!(
            config.installed.crates["foo"],
            CargoDependencySpec::Version("*".to_string())
        );
        assert_eq!(
            config.installed.crates["bar"],
            CargoDependencySpec::Version("1".to_string())
        );
        assert_eq!(
            config.installed.crates["baz"],
            CargoDependencySpec::Version("=1.2.3".to_string())
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn install_path_and_git_sources_update_peer_registries() {
    with_fixture(TestMode::SimulationOnly, &[], async |mut ctx| {
        ctx.symposium(&[
            "install",
            "--path",
            "/tmp/plugin-b",
            "--path",
            "/tmp/plugin-a",
        ])
        .await?;
        ctx.symposium(&[
            "install",
            "--git",
            "https://github.com/me/plugin-b",
            "--git",
            "https://github.com/me/plugin-a",
        ])
        .await?;

        let config = parse_config(&ctx);
        assert_eq!(
            config.installed.paths,
            vec!["/tmp/plugin-a", "/tmp/plugin-b"]
        );
        assert_eq!(
            config.installed.git,
            vec![
                "https://github.com/me/plugin-a",
                "https://github.com/me/plugin-b"
            ]
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn install_is_idempotent_and_updates_version_constraints() {
    with_fixture(TestMode::SimulationOnly, &[], async |mut ctx| {
        ctx.symposium(&["install", "foo@1"]).await?;
        let output = ctx.symposium(&["install", "foo@1"]).await?;
        assert!(output.contains("crate source already installed: foo"));

        let output = ctx.symposium(&["install", "foo@2"]).await?;
        assert!(output.contains("crate source updated: foo"));

        let config = parse_config(&ctx);
        assert_eq!(
            config.installed.crates["foo"],
            CargoDependencySpec::Version("2".to_string())
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn uninstall_removes_exact_matching_entries() {
    with_fixture(TestMode::SimulationOnly, &[], async |mut ctx| {
        ctx.symposium(&["install", "foo", "bar"]).await?;
        ctx.symposium(&["install", "--path", "/tmp/plugin"]).await?;
        ctx.symposium(&["install", "--git", "https://github.com/me/plugin"])
            .await?;

        let output = ctx.symposium(&["uninstall", "foo"]).await?;
        assert!(output.contains("crate source uninstalled: foo"));
        let output = ctx
            .symposium(&["uninstall", "--path", "/tmp/plugin"])
            .await?;
        assert!(output.contains("path source uninstalled: /tmp/plugin"));
        let output = ctx
            .symposium(&["uninstall", "--git", "https://github.com/me/plugin"])
            .await?;
        assert!(output.contains("git source uninstalled: https://github.com/me/plugin"));

        let config = parse_config(&ctx);
        assert!(!config.installed.crates.contains_key("foo"));
        assert!(config.installed.crates.contains_key("bar"));
        assert!(config.installed.paths.is_empty());
        assert!(config.installed.git.is_empty());
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn uninstall_missing_entry_is_noop() {
    with_fixture(TestMode::SimulationOnly, &[], async |mut ctx| {
        let output = ctx.symposium(&["uninstall", "missing"]).await?;
        assert!(output.contains("crate source was not installed: missing"));

        assert!(
            ctx.sym
                .config
                .installed
                .crates
                .contains_key("symposium-recommendations")
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn install_rejects_mixed_source_forms() {
    with_fixture(TestMode::SimulationOnly, &[], async |mut ctx| {
        let err = ctx
            .symposium(&["install", "foo", "--path", "/tmp/plugin"])
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("only one source form"),
            "unexpected error: {err}"
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn install_and_uninstall_are_in_human_help_section() {
    with_fixture(TestMode::SimulationOnly, &[], async |mut ctx| {
        let output = ctx.symposium(&["--help"]).await?;
        assert!(output.contains(indoc! {"
            install      Install plugin sources into user config
        "}));
        assert!(output.contains(indoc! {"
            uninstall    Uninstall plugin sources from user config
        "}));
        Ok(())
    })
    .await
    .unwrap();
}
