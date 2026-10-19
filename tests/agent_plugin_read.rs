use std::path::{Path, PathBuf};

use expect_test::expect;
use serde_json::Value;
use symposium::status_command::StatusState;
use symposium_testlib::{TestContext, TestMode, with_fixture};

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .collect()
}

fn installed_skill_dirs(ctx: &TestContext) -> Vec<PathBuf> {
    let skills = ctx.workspace_root.clone().unwrap().join(".claude/skills");
    subdirs(&skills)
        .into_iter()
        .flat_map(|dir| {
            let mut found = subdirs(&dir.join("skills"));
            found.push(dir);
            found
        })
        .filter(|dir| dir.join("SKILL.md").is_file())
        .collect()
}

fn installed_skills(ctx: &TestContext) -> Vec<String> {
    let mut names: Vec<String> = installed_skill_dirs(ctx)
        .iter()
        .map(|dir| dir.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

async fn with_report<F>(f: F) -> Vec<Value>
where
    F: AsyncFnOnce(),
{
    use symposium::report::{ReportLayer, ReportMode};
    use tracing_subscriber::layer::SubscriberExt;

    symposium_testlib::install_tracing_baseline();
    let (layer, handle) = ReportLayer::new(ReportMode::Json, tracing::Level::INFO);
    let guard = tracing::subscriber::set_default(tracing_subscriber::registry().with(layer));
    f().await;
    drop(guard);
    handle.drain()
}

fn event<'a>(events: &'a [Value], kind: &str, name: &str) -> &'a Value {
    events
        .iter()
        .find(|e| e["kind"] == kind && e["name"] == name)
        .unwrap_or_else(|| panic!("no {kind} event for {name}: {events:#?}"))
}

fn assert_warned(events: &[Value], expected: &[&str]) {
    let warnings: Vec<&str> = events
        .iter()
        .filter(|e| e["kind"] == "warning")
        .filter_map(|e| e["message"].as_str())
        .collect();
    for expected in expected {
        assert!(
            warnings.iter().any(|w| w.contains(expected)),
            "{expected}: {warnings:#?}"
        );
    }
}

#[tokio::test]
async fn registry_packages_install_their_skills_and_report_what_they_skip() {
    with_fixture(
        TestMode::SimulationOnly,
        &["agent-plugin-registry0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            let events = ctx.sync_with_report(tracing::Level::INFO).await?;

            assert_eq!(installed_skills(&ctx), ["serde-tables"]);
            assert_warned(
                &events,
                &[
                    "unknown field `commands` ignored",
                    "`mcp.json` is not supported",
                    "invalid `name` `Rejected_Tools`",
                ],
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn use_wakes_a_dormant_package_and_remove_reaps_it() {
    with_fixture(
        TestMode::SimulationOnly,
        &["agent-plugin-registry0"],
        async |mut ctx| {
            std::fs::remove_dir_all(ctx.sym.config_dir().join("plugins/rejected"))?;
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;
            assert!(!installed_skills(&ctx).contains(&"extract-tables".to_string()));

            ctx.symposium(&["use", "pdf-tools"]).await?;
            assert_eq!(installed_skills(&ctx), ["extract-tables", "serde-tables"]);

            ctx.symposium(&["use", "--remove", "pdf-tools"]).await?;
            assert_eq!(installed_skills(&ctx), ["serde-tables"]);
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn status_and_search_show_a_package_with_its_version_and_format() {
    with_fixture(
        TestMode::SimulationOnly,
        &["agent-plugin-registry0"],
        async |ctx| {
            let cwd = ctx.workspace_root.clone().unwrap();
            let status = with_report(async || {
                symposium::status_command::status(&ctx.sym, &cwd)
                    .await
                    .unwrap();
            })
            .await;
            let pdf_tools = event(&status, "plugin_status", "pdf-tools");
            assert_eq!(pdf_tools["state"], "dormant");
            assert_eq!(pdf_tools["version"], "1.2.0");
            assert_eq!(pdf_tools["format"], "agent plugin");
            let gated = event(&status, "plugin_status", "gated-tools");
            assert_eq!(gated["state"], "active");

            let search = with_report(async || {
                symposium::search_command::search(&ctx.sym, "pdf-tools")
                    .await
                    .unwrap();
            })
            .await;
            let hit = search
                .iter()
                .find(|e| e["kind"] == "search_match" && e["format"] == "agent plugin")
                .unwrap_or_else(|| panic!("no labeled hit: {search:#?}"));
            assert_eq!(hit["name"], "pdf-tools");
            assert_eq!(hit["version"], "1.2.0");
            assert_eq!(
                hit["description"],
                "Table extraction guidance (dormant: enable with `cargo agents use`)"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn a_workspace_member_package_installs_without_use() {
    with_fixture(
        TestMode::SimulationOnly,
        &["agent-plugin-member0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;
            assert_eq!(installed_skills(&ctx), ["maintainer-notes", "member-guide"]);
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn a_dependency_package_waits_for_consent_and_reports_what_it_skips() {
    with_fixture(
        TestMode::SimulationOnly,
        &["agent-plugin-dep0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;
            assert!(installed_skills(&ctx).is_empty());

            let deps = ctx.sym.workspace(ctx.workspace_root.as_ref().unwrap());
            let status = |name: &'static str, entries: &[symposium::status_command::StatusEntry]| {
                entries
                    .iter()
                    .find(|e| e.name == name)
                    .map(|e| e.state)
                    .unwrap_or_else(|| panic!("no status row for {name}"))
            };
            let entries = symposium::status_command::workspace_status(&ctx.sym, &deps).await?;
            assert_eq!(status("portable-dep", &entries), StatusState::Candidate);
            let registry = symposium::plugins::load_registry(&ctx.sym).await;
            let candidates = symposium::discovery::discover(&ctx.sym, &deps, &registry.plugins)
                .await
                .candidates;
            assert_eq!(
                candidates[0].description.as_deref(),
                Some("the Agent Plugins package `portable-guidance`")
            );

            ctx.symposium(&["use", "portable-dep"]).await?;
            ctx.symposium(&["use", "invalid-dep"]).await?;
            let events = ctx.sync_with_report(tracing::Level::INFO).await?;
            assert_eq!(installed_skills(&ctx), ["dep-guide"]);
            assert_warned(
                &events,
                &[
                    "unknown field `commands` ignored",
                    "ignoring crate `invalid-dep`: invalid `plugin.json`: missing required field `$schema`",
                ],
            );

            let entries = symposium::status_command::workspace_status(&ctx.sym, &deps).await?;
            assert_eq!(status("invalid-dep", &entries), StatusState::Dormant);
            Ok(())
        },
    )
    .await
    .unwrap();
}

fn validate(ctx: &TestContext, path: &Path) -> String {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_cargo-agents"))
        .args(["plugin", "validate", "--no-check-crates"])
        .arg(path)
        .env("SYMPOSIUM_HOME", ctx.sym.config_dir())
        .output()
        .expect("spawn cargo-agents");
    ctx.normalize_paths(&format!(
        "exit: {}\n{}{}",
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    ))
}

#[tokio::test]
async fn plugin_validate_reports_packages_and_fails_only_on_errors() {
    with_fixture(
        TestMode::SimulationOnly,
        &["agent-plugin-registry0"],
        async |ctx| {
            let plugins = ctx.sym.config_dir().join("plugins");
            expect![[r#"
                exit: 1
                ⚠️  $CONFIG_DIR/plugins/pdf-tools: unknown field `commands` ignored
                ⚠️  $CONFIG_DIR/plugins/pdf-tools: `mcp.json` is not supported yet; its MCP servers are ignored
                  ✅ gated-tools (agent plugin)
                  ✅ serde-tables (skill)
                  ⚠️  pdf-tools (agent plugin): plugin `pdf-tools` references no dependency; it stays dormant until enabled with `cargo agents use pdf-tools`
                  ✅ extract-tables (skill)
                  ✗ <unknown> (plugin): loading agent plugin from `$CONFIG_DIR/plugins/rejected/plugin.json`: invalid `name` `Rejected_Tools`: use 1 to 64 lowercase letters, digits, `-` and `.`, starting and ending with a letter or digit, with no `--` or `..`
            "#]].assert_eq(&validate(&ctx, &plugins));
            expect![[r#"
                exit: 0
                ⚠️  $CONFIG_DIR/plugins/pdf-tools: unknown field `commands` ignored
                ⚠️  $CONFIG_DIR/plugins/pdf-tools: `mcp.json` is not supported yet; its MCP servers are ignored
                  ⚠️  pdf-tools (agent plugin): plugin `pdf-tools` references no dependency; it stays dormant until enabled with `cargo agents use pdf-tools`
                  ✅ extract-tables (skill)
            "#]].assert_eq(&validate(&ctx, &plugins.join("pdf-tools")));
            Ok(())
        },
    )
    .await
    .unwrap();
}
