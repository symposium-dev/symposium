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

#[tokio::test]
async fn registry_packages_install_their_skills_and_report_what_they_skip() {
    with_fixture(
        TestMode::SimulationOnly,
        &["agent-plugin-registry0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            let events = ctx.sync_with_report(tracing::Level::INFO).await?;

            assert_eq!(installed_skills(&ctx), ["good", "serde-tables"]);

            let warnings: Vec<&str> = events
                .iter()
                .filter(|e| e["kind"] == "warning")
                .filter_map(|e| e["message"].as_str())
                .collect();
            for expected in [
                "unknown field `commands` ignored",
                "`mcp.json` is not supported",
                "invalid `name` `Rejected_Tools`",
                "cannot list registry `package-as-registry`: plugin source root contains plugin.json",
            ] {
                assert!(
                    warnings.iter().any(|w| w.contains(expected)),
                    "{expected}: {warnings:#?}"
                );
            }
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
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;
            assert!(!installed_skills(&ctx).contains(&"extract-tables".to_string()));

            ctx.symposium(&["use", "pdf-tools"]).await?;
            assert_eq!(
                installed_skills(&ctx),
                ["extract-tables", "good", "serde-tables"]
            );

            ctx.symposium(&["use", "--remove", "pdf-tools"]).await?;
            assert_eq!(installed_skills(&ctx), ["good", "serde-tables"]);
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
            assert_eq!(gated["format"], "agent plugin");

            let search = with_report(async || {
                symposium::search_command::search(&ctx.sym, "pdf-tools")
                    .await
                    .unwrap();
            })
            .await;
            let hits: Vec<&Value> = search
                .iter()
                .filter(|e| e["kind"] == "search_match" && e["origin"] == "user-plugins")
                .collect();
            assert_eq!(hits.len(), 1, "{hits:#?}");
            assert_eq!(hits[0]["name"], "pdf-tools");
            assert_eq!(hits[0]["version"], "1.2.0");
            assert_eq!(hits[0]["format"], "agent plugin");
            assert_eq!(
                hits[0]["description"],
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
async fn a_dependency_package_waits_for_consent_then_installs_its_top_level_skills() {
    with_fixture(
        TestMode::SimulationOnly,
        &["agent-plugin-dep0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;
            assert!(installed_skills(&ctx).is_empty());

            let deps = ctx.sym.workspace_deps(ctx.workspace_root.as_ref().unwrap());
            let entries = symposium::status_command::workspace_status(&ctx.sym, &deps).await?;
            let row = entries
                .iter()
                .find(|e| e.name == "portable-dep")
                .expect("the dependency is listed");
            assert_eq!(row.state, StatusState::Candidate);
            assert_eq!(row.format, Some("agent plugin"));

            ctx.symposium(&["use", "portable-dep"]).await?;
            assert_eq!(installed_skills(&ctx), ["dep-guide"]);
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn a_package_skill_link_outside_the_package_is_skipped_and_reported() {
    use std::os::unix::fs::symlink;

    with_fixture(
        TestMode::SimulationOnly,
        &["agent-plugin-links0"],
        async |mut ctx| {
            let root = ctx.workspace_root.clone().unwrap();
            let plugins = ctx.sym.config_dir().join("plugins");
            for skill in [
                "linked-package/skills/package-guide",
                "linked-manifest/skills/manifest-guide",
            ] {
                symlink(
                    root.join("shared/notes.md"),
                    plugins.join(skill).join("notes.md"),
                )?;
            }

            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            let events = ctx.sync_with_report(tracing::Level::INFO).await?;

            let skills = root.join(".claude/skills");
            assert!(skills.join("package-guide/SKILL.md").is_file());
            assert!(!skills.join("package-guide/notes.md").exists());
            assert_eq!(
                std::fs::read_to_string(skills.join("manifest-guide/notes.md"))?,
                "Shared notes living outside both plugins.\n",
                "a Symposium.toml plugin still follows links anywhere"
            );
            let outside: Vec<&str> = events
                .iter()
                .filter(|e| e["kind"] == "warning")
                .filter_map(|e| e["message"].as_str())
                .filter(|m| m.contains("resolves outside the package"))
                .collect();
            assert_eq!(outside.len(), 1, "{events:#?}");
            assert!(
                outside[0].ends_with("/package-guide/notes.md: resolves outside the package"),
                "{outside:?}"
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
                  ✅ gated-tools (agent plugin)
                  ✅ serde-tables (skill)
                  ✅ partly-broken (agent plugin)
                  ✗ path:partly-broken/skills/broken (skill): SKILL.md frontmatter missing required `description` field
                  ✅ good (skill)
                  ⚠️  pdf-tools (agent plugin): unknown field `commands` ignored
                  ⚠️  pdf-tools (agent plugin): `mcp.json` is not supported yet; its MCP servers are ignored
                  ⚠️  pdf-tools (agent plugin): plugin `pdf-tools` references no dependency; it stays dormant until enabled with `cargo agents use pdf-tools`
                  ✅ extract-tables (skill)
                  ✗ rejected (agent plugin): loading agent plugin from `$CONFIG_DIR/plugins/rejected/plugin.json`: invalid `name` `Rejected_Tools`: use 1 to 64 lowercase letters, digits, `-` and `.`, starting and ending with a letter or digit, with no `--` or `..`
            "#]].assert_eq(&validate(&ctx, &plugins));

            let package = validate(&ctx, &plugins.join("pdf-tools"));
            expect![[r#"
                exit: 0
                  ⚠️  pdf-tools (agent plugin): unknown field `commands` ignored
                  ⚠️  pdf-tools (agent plugin): `mcp.json` is not supported yet; its MCP servers are ignored
                  ⚠️  pdf-tools (agent plugin): plugin `pdf-tools` references no dependency; it stays dormant until enabled with `cargo agents use pdf-tools`
                  ✅ extract-tables (skill)
            "#]].assert_eq(&package);
            assert_eq!(
                validate(&ctx, &plugins.join("pdf-tools/plugin.json")),
                package
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}
