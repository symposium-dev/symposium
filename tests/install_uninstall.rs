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

#[tokio::test]
async fn use_cmd_creates_directory_scoped_entry() {
    with_fixture(TestMode::SimulationOnly, &[], async |mut ctx| {
        ctx.symposium(&["use", "foo"]).await?;
        let config = parse_config(&ctx);
        // Without --global, should create a directory-scoped entry
        let scoped = config.plugins.iter().find(|e| !e.predicates.is_empty());
        assert!(
            scoped.is_some(),
            "use without --global should create a directory-scoped entry"
        );
        let scoped = scoped.unwrap();
        assert_eq!(scoped.predicates.predicates.len(), 1);
        let pred_str = scoped.predicates.predicates[0].to_string();
        assert!(
            pred_str.starts_with("directory(") && pred_str.ends_with("/**)"),
            "expected directory(**) predicate, got: {pred_str}"
        );
        assert_eq!(
            scoped.source.crates["foo"],
            CargoDependencySpec::Version("*".to_string())
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn use_cmd_global_creates_unscoped_entry() {
    with_fixture(TestMode::SimulationOnly, &[], async |mut ctx| {
        ctx.symposium(&["use", "--global", "foo"]).await?;
        let config = parse_config(&ctx);
        // With --global, should go into an entry with empty predicates
        let global = config.plugins.iter().find(|e| {
            e.predicates.is_empty() && e.source.crates.contains_key("foo")
        });
        assert!(
            global.is_some(),
            "use --global should create/use a global entry"
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn use_cmd_appends_to_existing_directory_entry() {
    with_fixture(TestMode::SimulationOnly, &[], async |mut ctx| {
        ctx.symposium(&["use", "foo"]).await?;
        ctx.symposium(&["use", "bar"]).await?;
        let config = parse_config(&ctx);
        // Both should end up in the same directory-scoped entry
        let scoped = config.plugins.iter().find(|e| !e.predicates.is_empty()).unwrap();
        assert!(scoped.source.crates.contains_key("foo"));
        assert!(scoped.source.crates.contains_key("bar"));
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn remove_cmd_finds_source_across_entries() {
    with_fixture(TestMode::SimulationOnly, &[], async |mut ctx| {
        ctx.symposium(&["use", "foo"]).await?;
        ctx.symposium(&["use", "--global", "bar"]).await?;
        // Remove from scoped entry
        ctx.symposium(&["remove", "foo"]).await?;
        let config = parse_config(&ctx);
        assert!(!config.used.crates.contains_key("foo"));
        assert!(config.used.crates.contains_key("bar"));
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn remove_cmd_cleans_up_empty_entry() {
    with_fixture(TestMode::SimulationOnly, &[], async |mut ctx| {
        ctx.symposium(&["use", "foo"]).await?;
        ctx.symposium(&["remove", "foo"]).await?;
        let config = parse_config(&ctx);
        // The scoped entry that had only "foo" should be cleaned up
        let scoped_entries: Vec<_> = config.plugins.iter()
            .filter(|e| !e.predicates.is_empty())
            .collect();
        assert!(
            scoped_entries.is_empty() || scoped_entries.iter().all(|e| !e.source.is_empty()),
            "empty entries should be cleaned up after removal"
        );
        Ok(())
    })
    .await
    .unwrap();
}
