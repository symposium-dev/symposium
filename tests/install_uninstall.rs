//! Config-only use/remove command tests.

use indoc::indoc;
use symposium::config::{CargoDependencySpec, Config};
use symposium_testlib::{TestMode, with_fixture};

fn parse_config(ctx: &symposium_testlib::TestContext) -> Config {
    let path = ctx.sym.config_dir().join("config.toml");
    let contents = std::fs::read_to_string(path).unwrap();
    toml::from_str(&contents).unwrap()
}

#[tokio::test]
async fn use_cmd_registry_crates_updates_used_crates() {
    with_fixture(TestMode::SimulationOnly, &[], async |mut ctx| {
        let output = ctx
            .symposium(&["use", "foo", "bar@1", "baz@=1.2.3"])
            .await?;
        assert!(output.contains("crate source added: foo"));
        assert!(output.contains("crate source added: bar"));
        assert!(output.contains("crate source added: baz"));

        let config = parse_config(&ctx);
        assert_eq!(
            config.used.crates["foo"],
            CargoDependencySpec::Version("*".to_string())
        );
        assert_eq!(
            config.used.crates["bar"],
            CargoDependencySpec::Version("1".to_string())
        );
        assert_eq!(
            config.used.crates["baz"],
            CargoDependencySpec::Version("=1.2.3".to_string())
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn use_cmd_path_and_git_sources_update_peer_registries() {
    with_fixture(TestMode::SimulationOnly, &[], async |mut ctx| {
        ctx.symposium(&[
            "use",
            "--path",
            "/tmp/plugin-b",
            "--path",
            "/tmp/plugin-a",
        ])
        .await?;
        ctx.symposium(&[
            "use",
            "--git",
            "https://github.com/me/plugin-b",
            "--git",
            "https://github.com/me/plugin-a",
        ])
        .await?;

        let config = parse_config(&ctx);
        assert_eq!(
            config.used.paths,
            vec!["/tmp/plugin-a", "/tmp/plugin-b"]
        );
        assert_eq!(
            config.used.git,
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
async fn use_cmd_is_idempotent_and_updates_version_constraints() {
    with_fixture(TestMode::SimulationOnly, &[], async |mut ctx| {
        ctx.symposium(&["use", "foo@1"]).await?;
        let output = ctx.symposium(&["use", "foo@1"]).await?;
        assert!(output.contains("crate source already added: foo"));

        let output = ctx.symposium(&["use", "foo@2"]).await?;
        assert!(output.contains("crate source updated: foo"));

        let config = parse_config(&ctx);
        assert_eq!(
            config.used.crates["foo"],
            CargoDependencySpec::Version("2".to_string())
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn remove_cmd_removes_exact_matching_entries() {
    with_fixture(TestMode::SimulationOnly, &[], async |mut ctx| {
        ctx.symposium(&["use", "foo", "bar"]).await?;
        ctx.symposium(&["use", "--path", "/tmp/plugin"]).await?;
        ctx.symposium(&["use", "--git", "https://github.com/me/plugin"])
            .await?;

        let output = ctx.symposium(&["remove", "foo"]).await?;
        assert!(output.contains("crate source removed: foo"));
        let output = ctx
            .symposium(&["remove", "--path", "/tmp/plugin"])
            .await?;
        assert!(output.contains("path source removed: /tmp/plugin"));
        let output = ctx
            .symposium(&["remove", "--git", "https://github.com/me/plugin"])
            .await?;
        assert!(output.contains("git source removed: https://github.com/me/plugin"));

        let config = parse_config(&ctx);
        assert!(!config.used.crates.contains_key("foo"));
        assert!(config.used.crates.contains_key("bar"));
        assert!(config.used.paths.is_empty());
        assert!(config.used.git.is_empty());
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn remove_cmd_missing_entry_is_noop() {
    with_fixture(TestMode::SimulationOnly, &[], async |mut ctx| {
        let output = ctx.symposium(&["remove", "missing"]).await?;
        assert!(output.contains("crate source not present: missing"));

        assert!(
            ctx.sym
                .config
                .used
                .crates
                .contains_key("symposium-recommendations")
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn use_cmd_rejects_mixed_source_forms() {
    with_fixture(TestMode::SimulationOnly, &[], async |mut ctx| {
        let err = ctx
            .symposium(&["use", "foo", "--path", "/tmp/plugin"])
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
async fn use_cmd_and_remove_cmd_are_in_human_help_section() {
    with_fixture(TestMode::SimulationOnly, &[], async |mut ctx| {
        let output = ctx.symposium(&["--help"]).await?;
        assert!(output.contains(indoc! {"
            use          Add plugin sources to user config
        "}));
        assert!(output.contains(indoc! {"
            remove       Remove plugin sources from user config
        "}));
        Ok(())
    })
    .await
    .unwrap();
}
