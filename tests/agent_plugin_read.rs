use std::path::Path;

use expect_test::expect;
use serde_json::Value;
use symposium::status_command::StatusState;
use symposium_testlib::{TestContext, TestMode, with_fixture};

fn installed_skills(ctx: &TestContext) -> Vec<String> {
    let dir = ctx.workspace_root.clone().unwrap().join(".claude/skills");
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.path().join("SKILL.md").is_file())
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
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
                    "MCP server `pdf-socket` skipped: unsupported transport `websocket`",
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

            let mcp_json = ctx.workspace_root.clone().unwrap().join(".mcp.json");
            let pdf_server = || -> Option<Value> {
                let text = std::fs::read_to_string(&mcp_json).ok()?;
                let config: Value = serde_json::from_str(&text).unwrap();
                config["mcpServers"].get("pdf-tables").cloned()
            };
            assert_eq!(pdf_server(), None);

            ctx.symposium(&["use", "pdf-tools"]).await?;
            assert_eq!(installed_skills(&ctx), ["extract-tables", "serde-tables"]);
            let root = dunce::canonicalize(ctx.sym.config_dir().join("plugins/pdf-tools"))?;
            let data = dunce::canonicalize(
                ctx.sym
                    .config_dir()
                    .join("plugin-data/user-plugins/pdf-tools"),
            )?;
            let server = pdf_server().expect("pdf-tables registered");
            let command = root.join("bin").join("pdf-server");
            assert_eq!(server["command"], command.display().to_string());
            let (root, data) = (root.display().to_string(), data.display().to_string());
            assert_eq!(
                server["args"],
                serde_json::json!(["--config", format!("{root}/config.json")])
            );
            assert_eq!(
                server["env"],
                serde_json::json!({
                    "CACHE": format!("{data}/cache"),
                    "PLUGIN_ROOT": root,
                    "PLUGIN_DATA": data,
                })
            );

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
            let mcp_json = ctx.workspace_root.clone().unwrap().join(".mcp.json");
            let registered = || {
                std::fs::read_to_string(&mcp_json).is_ok_and(|text| text.contains("portable-docs"))
            };
            assert!(!registered());

            let deps = ctx.sym.workspace_deps(ctx.workspace_root.as_ref().unwrap());
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
                Some("the Agent Plugins package `portable-guidance` (1 skill group, 1 MCP server)")
            );

            ctx.symposium(&["use", "portable-dep"]).await?;
            ctx.symposium(&["use", "invalid-dep"]).await?;
            let events = ctx.sync_with_report(tracing::Level::INFO).await?;
            assert_eq!(installed_skills(&ctx), ["dep-guide"]);
            assert!(registered());
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

#[cfg(unix)]
#[tokio::test]
async fn a_package_skill_link_outside_the_package_is_skipped_and_reported() {
    with_fixture(
        TestMode::SimulationOnly,
        &["agent-plugin-registry0"],
        async |mut ctx| {
            let root = ctx.workspace_root.clone().unwrap();
            let skill = ctx
                .sym
                .config_dir()
                .join("plugins/gated-tools/skills/serde-tables");
            std::os::unix::fs::symlink(root.join("Cargo.toml"), skill.join("notes.md"))?;

            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.sym.config.sync_debounce_secs = 0;
            let events = ctx.sync_with_report(tracing::Level::INFO).await?;

            let installed = root.join(".claude/skills/serde-tables");
            assert!(installed.join("SKILL.md").is_file());
            assert!(!installed.join("notes.md").exists());
            assert_warned(
                &events,
                &["serde-tables/notes.md: resolves outside the package"],
            );

            let events = ctx.sync_with_report(tracing::Level::INFO).await?;
            assert!(
                !events.iter().any(|e| e["kind"] == "skill_installed"),
                "an unchanged package reinstalls nothing: {events:#?}"
            );
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
                ⚠️  $CONFIG_DIR/plugins/pdf-tools: MCP server `pdf-socket` skipped: unsupported transport `websocket`
                  ⚠️  gated-tools (agent plugin): `plugin.json` sets no `version`; agents that cache plugins by version, such as Codex, can keep an outdated copy
                  ✅ serde-tables (skill)
                  ⚠️  pdf-tools (agent plugin): plugin `pdf-tools` references no dependency; it stays dormant until enabled with `cargo agents use pdf-tools`
                  ✅ extract-tables (skill)
                  ✗ rejected (plugin): loading agent plugin from `$CONFIG_DIR/plugins/rejected/plugin.json`: invalid `name` `Rejected_Tools`: use 1 to 64 lowercase letters, digits, `-` and `.`, starting and ending with a letter or digit, with no `--` or `..`
            "#]].assert_eq(&validate(&ctx, &plugins));
            expect![[r#"
                exit: 0
                ⚠️  $CONFIG_DIR/plugins/pdf-tools: unknown field `commands` ignored
                ⚠️  $CONFIG_DIR/plugins/pdf-tools: MCP server `pdf-socket` skipped: unsupported transport `websocket`
                  ⚠️  pdf-tools (agent plugin): plugin `pdf-tools` references no dependency; it stays dormant until enabled with `cargo agents use pdf-tools`
                  ✅ extract-tables (skill)
            "#]].assert_eq(&validate(&ctx, &plugins.join("pdf-tools")));
            assert_eq!(
                validate(&ctx, &plugins.join("pdf-tools/plugin.json")),
                validate(&ctx, &plugins.join("pdf-tools"))
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}
