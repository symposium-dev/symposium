#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use symposium_testlib::{TestContext, TestMode, with_fixture};

const SKILL_DIRS: [&str; 2] = [".claude/skills", ".agents/skills"];

async fn init(ctx: &mut TestContext) -> anyhow::Result<()> {
    ctx.symposium(&[
        "init",
        "--add-agent",
        "claude",
        "--add-agent",
        "codex",
        "--add-agent",
        "copilot",
    ])
    .await?;
    ctx.symposium(&["sync"]).await?;
    Ok(())
}

fn workspace(ctx: &TestContext) -> PathBuf {
    ctx.workspace_root.clone().unwrap()
}

fn plugin_path(ctx: &TestContext, name: &str) -> String {
    workspace(ctx)
        .join("authoring")
        .join(name)
        .display()
        .to_string()
}

fn agent_roots(ctx: &TestContext, workspace: &Path) -> [Vec<PathBuf>; 3] {
    let home = ctx.sym.home_dir();
    [
        vec![
            workspace.join(".claude/skills"),
            home.join(".claude/skills"),
        ],
        vec![
            workspace.join(".agents/skills"),
            home.join(".codex/plugins"),
        ],
        vec![
            workspace.join(".agents/skills"),
            ctx.sym.config_dir().join("installed"),
        ],
    ]
}

fn find_skill(root: &Path, skill: &str) -> Option<PathBuf> {
    let dirs = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir());
    for dir in dirs {
        if dir.ends_with(skill) && dir.join("SKILL.md").is_file() {
            return Some(dir);
        }
        if let Some(found) = find_skill(&dir, skill) {
            return Some(found);
        }
    }
    None
}

fn installed_in_every_agent(ctx: &TestContext, workspace: &Path, skill: &str) -> bool {
    agent_roots(ctx, workspace)
        .iter()
        .all(|roots| roots.iter().any(|root| find_skill(root, skill).is_some()))
}

fn installed_anywhere(ctx: &TestContext, workspace: &Path, skill: &str) -> bool {
    agent_roots(ctx, workspace)
        .iter()
        .flatten()
        .any(|root| find_skill(root, skill).is_some())
}

fn config(ctx: &TestContext) -> String {
    std::fs::read_to_string(ctx.sym.config_dir().join("config.toml")).unwrap()
}

fn user_plugins(ctx: &TestContext) -> PathBuf {
    ctx.sym.config_dir().join("plugins")
}

fn snapshot(ctx: &TestContext) -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    files.insert("config.toml".to_string(), config(ctx));
    files.insert(
        "plugins".to_string(),
        format!(
            "{:?}",
            std::fs::read_dir(user_plugins(ctx)).map(|d| d.count()).ok()
        ),
    );
    for dir in SKILL_DIRS {
        collect(&workspace(ctx).join(dir), dir, &mut files);
    }
    files
}

fn collect(path: &Path, rel: &str, files: &mut BTreeMap<String, String>) {
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let rel = format!("{rel}/{}", entry.file_name().to_string_lossy());
        if entry.path().is_dir() {
            collect(&entry.path(), &rel, files);
        } else {
            files.insert(
                rel,
                std::fs::read_to_string(entry.path()).unwrap_or_default(),
            );
        }
    }
}

#[tokio::test]
async fn try_installs_a_dormant_plugin_in_every_agent() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-try0"],
        async |mut ctx| {
            init(&mut ctx).await?;
            let kit = plugin_path(&ctx, "dormant-kit");

            ctx.symposium(&["plugin", "try", &kit]).await?;

            assert_eq!(
                std::fs::read_link(user_plugins(&ctx).join("dormant-kit"))?,
                PathBuf::from(&kit).canonicalize()?
            );
            assert!(
                config(&ctx).contains(r#"use = ["dormant-kit"]"#),
                "{}",
                config(&ctx)
            );
            assert!(installed_in_every_agent(
                &ctx,
                &workspace(&ctx),
                "kit-guidance"
            ));
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn try_installs_a_bare_skill() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-try0"],
        async |mut ctx| {
            init(&mut ctx).await?;

            ctx.symposium(&["plugin", "try", &plugin_path(&ctx, "solo-skill")])
                .await?;

            assert!(installed_in_every_agent(
                &ctx,
                &workspace(&ctx),
                "solo-skill"
            ));
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn try_installs_an_agent_plugins_package() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-try0"],
        async |mut ctx| {
            init(&mut ctx).await?;

            ctx.symposium(&["plugin", "try", &plugin_path(&ctx, "package-kit")])
                .await?;

            assert!(installed_in_every_agent(
                &ctx,
                &workspace(&ctx),
                "package-guidance"
            ));
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn try_leaves_a_gated_plugin_to_its_gate() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-try0"],
        async |mut ctx| {
            init(&mut ctx).await?;
            fake_agent(&ctx, "codex", "[]");
            fake_agent(&ctx, "copilot", "[]");

            let kit = plugin_path(&ctx, "gated-kit");
            let events = report_events(&mut ctx, &["plugin", "try", &kit]).await;
            let messages = messages(&events);

            assert!(
                events
                    .iter()
                    .any(|e| e["kind"] == "plugin_tried" && e["global"] == false),
                "{events:#?}"
            );
            for expected in [
                "`gated-kit` activates only in workspaces that depend on `serde`, as it does for its users",
                "`gated-kit` does not apply in this workspace, so nothing is installed here",
            ] {
                assert!(messages.iter().any(|m| m == expected), "{messages:#?}");
            }
            assert!(!messages.iter().any(|m| m.contains("list")), "{messages:#?}");
            assert!(user_plugins(&ctx).join("gated-kit").is_dir());
            assert!(!config(&ctx).contains("gated-kit"), "{}", config(&ctx));
            assert!(!installed_anywhere(&ctx, &workspace(&ctx), "gated-guidance"));
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn try_checks_a_gated_plugin_the_user_enabled_everywhere() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-try0"],
        async |mut ctx| {
            init(&mut ctx).await?;
            let own = symposium::config::UseEntry::Global("gated-kit".into());
            ctx.sym.config.plugins.used.push(own);
            ctx.sym.save_config()?;
            fake_agent(&ctx, "codex", "[]");

            let kit = plugin_path(&ctx, "gated-kit");
            let events = report_events(&mut ctx, &["plugin", "try", &kit]).await;
            let messages = messages(&events);

            assert!(
                events
                    .iter()
                    .any(|e| e["kind"] == "plugin_tried" && e["global"] == true),
                "{events:#?}"
            );
            assert!(
                messages.iter().any(|m| m
                    == "Codex CLI does not list gated-guidance (checked with `codex debug prompt-input hi`)"),
                "{messages:#?}"
            );
            assert!(
                !messages.iter().any(|m| m.contains("does not apply")),
                "{messages:#?}"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn try_remove_keeps_a_use_entry_it_did_not_add() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-try0"],
        async |mut ctx| {
            init(&mut ctx).await?;
            let config_path = ctx.sym.config_dir().join("config.toml");
            let own = format!("{}\n[plugins]\nuse = [\"gated-kit\"]\n", config(&ctx));
            std::fs::write(&config_path, &own)?;
            ctx.sym = symposium::config::Symposium::from_dir(ctx.sym.config_dir());
            let kit = plugin_path(&ctx, "gated-kit");

            ctx.symposium(&["plugin", "try", &kit]).await?;
            ctx.symposium(&["plugin", "try", "--remove", &kit]).await?;

            assert!(
                config(&ctx).contains(r#"use = ["gated-kit"]"#),
                "{}",
                config(&ctx)
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn try_edits_reach_the_agents_on_the_next_sync() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-try0"],
        async |mut ctx| {
            init(&mut ctx).await?;
            let kit = plugin_path(&ctx, "dormant-kit");
            ctx.symposium(&["plugin", "try", &kit]).await?;

            let source = PathBuf::from(&kit).join("skills/kit-guidance/SKILL.md");
            let edited = std::fs::read_to_string(&source)?.replace("First draft.", "Second draft.");
            std::fs::write(&source, &edited)?;
            ctx.symposium(&["sync"]).await?;

            for roots in agent_roots(&ctx, &workspace(&ctx)) {
                let installed = roots
                    .iter()
                    .find_map(|root| find_skill(root, "kit-guidance"))
                    .expect("kit-guidance is installed");
                let text = std::fs::read_to_string(installed.join("SKILL.md"))?;
                assert_eq!(text, edited, "{}", installed.display());
            }
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn try_remove_preserves_preexisting_dormant_enablement() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-try0"],
        async |mut ctx| {
            let kit = plugin_path(&ctx, "dormant-kit");
            // Outside a workspace, removal cannot fetch a same-named crate after the
            // local trial is unlinked; this test checks the durable configuration.
            let outside = tempfile::tempdir()?;
            ctx.workspace_root = Some(outside.path().to_path_buf());
            let own = symposium::config::UseEntry::Global("dormant-kit".into());
            ctx.sym.config.plugins.used.push(own.clone());
            ctx.sym.save_config()?;

            ctx.symposium(&["plugin", "try", &kit]).await?;
            ctx.symposium(&["plugin", "try", "--remove", &kit]).await?;

            assert!(!user_plugins(&ctx).join("dormant-kit").exists());
            let saved = symposium::config::Symposium::from_dir(ctx.sym.config_dir());
            assert_eq!(saved.config.plugins.used, [own]);
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn try_remove_preserves_explicitly_recreated_enablement() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-try0"],
        async |mut ctx| {
            let kit = plugin_path(&ctx, "dormant-kit");
            // Keep the commands outside a workspace so removal cannot fetch a
            // same-named crate after the trial's local source is unlinked.
            let outside = tempfile::tempdir()?;
            ctx.workspace_root = Some(outside.path().to_path_buf());
            let config_dir = ctx.sym.config_dir().to_path_buf();
            let saved_uses = || {
                symposium::config::Symposium::from_dir(&config_dir)
                    .config
                    .plugins
                    .used
            };
            let explicit = symposium::config::UseEntry::Global("dormant-kit".into());
            assert!(saved_uses().is_empty());

            ctx.symposium(&["plugin", "try", &kit]).await?;
            assert_eq!(saved_uses().as_slice(), std::slice::from_ref(&explicit));

            ctx.symposium(&["use", "--global", "--remove", "dormant-kit"])
                .await?;
            assert!(saved_uses().is_empty(), "the trial's entry must be removed");

            ctx.symposium(&["use", "--global", "dormant-kit"]).await?;
            assert_eq!(saved_uses().as_slice(), std::slice::from_ref(&explicit));

            ctx.symposium(&["plugin", "try", "--remove", &kit]).await?;
            assert!(!user_plugins(&ctx).join("dormant-kit").exists());
            assert_eq!(
                saved_uses(),
                [explicit],
                "removing the trial must preserve the new entry the user explicitly added"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn try_remove_after_adding_a_gate_drops_its_enablement() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-try0"],
        async |mut ctx| {
            let kit = plugin_path(&ctx, "dormant-kit");
            let outside = tempfile::tempdir()?;
            ctx.workspace_root = Some(outside.path().to_path_buf());
            assert!(ctx.sym.config.plugins.used.is_empty());

            ctx.symposium(&["plugin", "try", &kit]).await?;
            assert_eq!(
                ctx.sym.config.plugins.used,
                [symposium::config::UseEntry::Global("dormant-kit".into())]
            );
            let manifest = PathBuf::from(&kit).join("Symposium.toml");
            let edited = std::fs::read_to_string(&manifest)?.replace(
                "name = \"dormant-kit\"",
                "name = \"dormant-kit\"\ndepends-on = [\"*\"]",
            );
            std::fs::write(&manifest, edited)?;
            ctx.symposium(&["plugin", "try", "--remove", &kit]).await?;

            assert!(!user_plugins(&ctx).join("dormant-kit").exists());
            let saved = symposium::config::Symposium::from_dir(ctx.sym.config_dir());
            assert!(saved.config.plugins.used.is_empty(), "{}", config(&ctx));
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn try_refuses_a_name_already_taken() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-try0"],
        async |mut ctx| {
            init(&mut ctx).await?;
            let taken = user_plugins(&ctx).join("dormant-kit");
            std::fs::create_dir_all(&taken)?;

            let err = ctx
                .symposium(&["plugin", "try", &plugin_path(&ctx, "dormant-kit")])
                .await
                .expect_err("a user plugin directory must not be replaced");
            assert!(err.to_string().contains("is not a trial"), "{err}");

            std::fs::remove_dir(&taken)?;
            std::os::unix::fs::symlink(plugin_path(&ctx, "gated-kit"), &taken)?;
            let err = ctx
                .symposium(&["plugin", "try", &plugin_path(&ctx, "dormant-kit")])
                .await
                .expect_err("a trial of another folder must not be replaced");
            assert!(err.to_string().contains("already being tried"), "{err}");
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn try_remove_leaves_everything_as_before() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-try0"],
        async |mut ctx| {
            init(&mut ctx).await?;
            let before = snapshot(&ctx);
            let kit = plugin_path(&ctx, "dormant-kit");

            ctx.symposium(&["plugin", "try", &kit]).await?;
            assert_ne!(snapshot(&ctx), before);
            ctx.symposium(&["plugin", "try", "--remove", &kit]).await?;

            assert_eq!(snapshot(&ctx), before);
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn try_remove_by_name_works_after_the_folder_is_gone() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-try0"],
        async |mut ctx| {
            init(&mut ctx).await?;
            let kit = plugin_path(&ctx, "dormant-kit");
            ctx.symposium(&["plugin", "try", &kit]).await?;
            std::fs::remove_dir_all(&kit)?;

            ctx.symposium(&["plugin", "try", "--remove", "dormant-kit"])
                .await?;

            assert!(!user_plugins(&ctx).join("dormant-kit").exists());
            assert!(!installed_anywhere(&ctx, &workspace(&ctx), "kit-guidance"));
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn try_remove_leaves_a_user_plugin_alone() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-try0"],
        async |mut ctx| {
            init(&mut ctx).await?;
            let own = user_plugins(&ctx).join("dormant-kit");
            std::fs::create_dir_all(&own)?;

            let err = ctx
                .symposium(&["plugin", "try", "--remove", "dormant-kit"])
                .await
                .expect_err("a user plugin is not a trial");
            assert!(err.to_string().contains("not a trial"), "{err}");
            assert!(own.is_dir());
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn try_remove_cleans_other_workspaces_on_their_next_sync() {
    with_fixture(TestMode::SimulationOnly, &["plugin-try0"], async |mut ctx| {
        init(&mut ctx).await?;
        let other = ctx.tempdir.join("other");
        std::fs::create_dir_all(other.join("src"))?;
        std::fs::write(
            other.join("Cargo.toml"),
            "[package]\nname = \"other\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
        )?;
        std::fs::write(other.join("src/lib.rs"), "")?;
        let sync_other = async |ctx: &TestContext| {
            symposium::sync::sync(
                &ctx.sym,
                &ctx.sym.workspace(&other),
                symposium::UpdateLevel::None,
            )
            .await
        };

        let kit = plugin_path(&ctx, "dormant-kit");
        ctx.symposium(&["plugin", "try", &kit]).await?;
        sync_other(&ctx).await?;
        assert!(installed_in_every_agent(&ctx, &other, "kit-guidance"));

        ctx.symposium(&["plugin", "try", "--remove", &kit]).await?;
        sync_other(&ctx).await?;
        assert!(!installed_anywhere(&ctx, &other, "kit-guidance"));
        Ok(())
    })
    .await
    .unwrap();
}

fn fake_agent(ctx: &TestContext, program: &str, stdout: &str) {
    use std::os::unix::fs::PermissionsExt;
    let bin = ctx.sym.config_dir().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let path = bin.join(program);
    std::fs::write(&path, format!("#!/bin/sh\nprintf '%s' '{stdout}'\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

async fn report_events(ctx: &mut TestContext, args: &[&str]) -> Vec<serde_json::Value> {
    use symposium::report::{ReportLayer, ReportMode};
    use tracing_subscriber::layer::SubscriberExt;

    symposium_testlib::install_tracing_baseline();
    let (layer, handle) = ReportLayer::new(ReportMode::Json, tracing::Level::INFO);
    let guard = tracing::subscriber::set_default(tracing_subscriber::registry().with(layer));
    ctx.symposium(args).await.unwrap();
    drop(guard);
    handle.drain()
}

fn messages(events: &[serde_json::Value]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| event["message"].as_str().map(str::to_string))
        .collect()
}

async fn report_messages(ctx: &mut TestContext, args: &[&str]) -> Vec<String> {
    messages(&report_events(ctx, args).await)
}

#[tokio::test]
async fn try_reports_what_each_agent_lists() {
    with_fixture(TestMode::SimulationOnly, &["plugin-try0"], async |mut ctx| {
        init(&mut ctx).await?;
        fake_agent(
            &ctx,
            "codex",
            r#"[{"content":[{"text":"<skills_instructions>\n### Available skills\n- dormant-kit:kit-guidance: Guidance (file: r1/dormant-kit/0.0.0/skills/kit-guidance/SKILL.md)"}]}]"#,
        );
        fake_agent(&ctx, "copilot", "[]");
        let kit = plugin_path(&ctx, "dormant-kit");

        let messages = report_messages(&mut ctx, &["plugin", "try", &kit]).await;

        for expected in [
            "Codex CLI lists kit-guidance",
            "GitHub Copilot does not list kit-guidance (checked with `copilot skill list --json`)",
            "Claude Code has no listing command; start a new session here and run `/skills`",
        ] {
            assert!(
                messages.iter().any(|m| m == expected),
                "missing {expected:?} in {messages:#?}"
            );
        }
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn try_skips_agents_that_are_not_installed() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-try0"],
        async |mut ctx| {
            init(&mut ctx).await?;
            let kit = plugin_path(&ctx, "dormant-kit");

            let messages = report_messages(&mut ctx, &["plugin", "try", &kit]).await;

            assert!(
                messages
                    .iter()
                    .any(|m| m == "Codex CLI: `codex` is not on PATH, not checked"),
                "{messages:#?}"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}
