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

fn installed_in_every_agent(workspace: &Path, skill: &str) -> bool {
    SKILL_DIRS
        .iter()
        .all(|dir| workspace.join(dir).join(skill).join("SKILL.md").is_file())
}

fn installed_anywhere(workspace: &Path, skill: &str) -> bool {
    SKILL_DIRS
        .iter()
        .any(|dir| workspace.join(dir).join(skill).exists())
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
            assert!(installed_in_every_agent(&workspace(&ctx), "kit-guidance"));
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

            assert!(installed_in_every_agent(&workspace(&ctx), "solo-skill"));
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

            let kit = plugin_path(&ctx, "gated-kit");
            let messages = report_messages(&mut ctx, &["plugin", "try", &kit]).await;

            for expected in [
                "`gated-kit` activates only in workspaces that depend on `serde`, as it does for its users",
                "`gated-kit` does not apply in this workspace, so nothing is installed here",
            ] {
                assert!(messages.iter().any(|m| m == expected), "{messages:#?}");
            }
            assert!(user_plugins(&ctx).join("gated-kit").is_dir());
            assert!(!config(&ctx).contains("gated-kit"), "{}", config(&ctx));
            assert!(!installed_anywhere(&workspace(&ctx), "gated-guidance"));
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

            for dir in SKILL_DIRS {
                let installed = workspace(&ctx).join(dir).join("kit-guidance/SKILL.md");
                assert_eq!(std::fs::read_to_string(installed)?, edited, "{dir}");
            }
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
            assert!(!installed_anywhere(&workspace(&ctx), "kit-guidance"));
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
                &ctx.sym.workspace_deps(&other),
                symposium::UpdateLevel::None,
            )
            .await
        };

        let kit = plugin_path(&ctx, "dormant-kit");
        ctx.symposium(&["plugin", "try", &kit]).await?;
        sync_other(&ctx).await?;
        assert!(installed_in_every_agent(&other, "kit-guidance"));

        ctx.symposium(&["plugin", "try", "--remove", &kit]).await?;
        assert!(installed_in_every_agent(&other, "kit-guidance"));
        sync_other(&ctx).await?;
        assert!(!installed_anywhere(&other, "kit-guidance"));
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

async fn report_messages(ctx: &mut TestContext, args: &[&str]) -> Vec<String> {
    use symposium::report::{ReportLayer, ReportMode};
    use tracing_subscriber::layer::SubscriberExt;

    symposium_testlib::install_tracing_baseline();
    let (layer, handle) = ReportLayer::new(ReportMode::Json, tracing::Level::INFO);
    let guard = tracing::subscriber::set_default(tracing_subscriber::registry().with(layer));
    ctx.symposium(args).await.unwrap();
    drop(guard);
    handle
        .drain()
        .iter()
        .filter_map(|event| event["message"].as_str().map(str::to_string))
        .collect()
}

#[tokio::test]
async fn try_reports_what_each_agent_lists() {
    with_fixture(TestMode::SimulationOnly, &["plugin-try0"], async |mut ctx| {
        init(&mut ctx).await?;
        fake_agent(
            &ctx,
            "codex",
            r#"[{"content":[{"text":"<skills_instructions>\n### Available skills\n- kit-guidance: Guidance (file: r0/kit-guidance/SKILL.md)"}]}]"#,
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
