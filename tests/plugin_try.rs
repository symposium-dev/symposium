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

/// The sync after a removal resolves the remaining `use` entries, and cargo
/// searches crates.io for one that no registry offers.
fn without_cargo(ctx: &mut TestContext) -> anyhow::Result<()> {
    let path = ctx.sym.config_dir().join("config.toml");
    let config = std::fs::read_to_string(&path)?;
    std::fs::write(
        &path,
        format!(
            "{config}\n[[package-manager]]\nname = \"cargo\"\n\
             command = \"/nonexistent/symposium-pm-cargo\"\n"
        ),
    )?;
    ctx.sym = symposium::config::Symposium::from_dir(ctx.sym.config_dir());
    Ok(())
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
                "no skill of `gated-kit` applies in this workspace, so none is installed here",
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
                !messages.iter().any(|m| m.contains("none is installed")),
                "{messages:#?}"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

async fn try_predicate_kit(ctx: &mut TestContext, check: &str) -> anyhow::Result<Vec<String>> {
    init(ctx).await?;
    std::fs::write(
        ctx.tempdir.join("ready-check.sh"),
        format!("#!/bin/sh\n{check}\n"),
    )?;
    fake_agent(
        ctx,
        "codex",
        r#"[{"content":[{"text":"- predicate-guidance: Guidance (file: r0/predicate-guidance/SKILL.md)"}]}]"#,
    );
    let kit = plugin_path(ctx, "predicate-kit");
    Ok(report_messages(ctx, &["plugin", "try", &kit]).await)
}

#[tokio::test]
async fn try_checks_a_plugin_whose_custom_predicate_holds() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-try0"],
        async |mut ctx| {
            let messages = try_predicate_kit(&mut ctx, "exit 0").await?;

            assert!(installed_in_every_agent(
                &ctx,
                &workspace(&ctx),
                "predicate-guidance"
            ));
            assert!(
                messages
                    .iter()
                    .any(|m| m == "Codex CLI lists predicate-guidance"),
                "{messages:#?}"
            );
            assert!(
                !messages.iter().any(|m| m.contains("none is installed")),
                "{messages:#?}"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn try_skips_the_check_when_the_custom_predicate_fails() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-try0"],
        async |mut ctx| {
            let messages = try_predicate_kit(&mut ctx, "exit 1").await?;

            assert!(!installed_anywhere(
                &ctx,
                &workspace(&ctx),
                "predicate-guidance"
            ));
            assert!(
                messages.iter().any(|m| m
                    == "no skill of `predicate-kit` applies in this workspace, so none is installed here"),
                "{messages:#?}"
            );
            assert!(!messages.iter().any(|m| m.contains("list")), "{messages:#?}");
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
            without_cargo(&mut ctx)?;
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
            without_cargo(&mut ctx)?;
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

fn served_kit(workspace: &Path) -> anyhow::Result<String> {
    let kit = workspace.join("authoring/served-kit");
    std::fs::create_dir_all(kit.join("skills/served-guidance"))?;
    std::fs::write(
        kit.join("Symposium.toml"),
        "name = \"served-kit\"\n\n[[skills]]\nsource.path = \"skills\"\n\n\
         [[mcp_servers]]\nname = \"served-kit-server\"\n\
         command = \"/usr/bin/true\"\nargs = []\nenv = []\n",
    )?;
    std::fs::write(
        kit.join("skills/served-guidance/SKILL.md"),
        "---\nname: served-guidance\ndescription: Guidance beside an MCP server\n\
         ---\n\nServed guidance.\n",
    )?;
    Ok(kit.display().to_string())
}

#[tokio::test]
async fn try_and_remove_reach_the_user_level_inside_and_outside_a_workspace() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-try0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--hook-scope", "global", "--add-agent", "claude"])
                .await?;
            let kit = served_kit(&workspace(&ctx))?;
            let home = ctx.sym.home_dir().to_path_buf();
            let registered =
                || std::fs::read_to_string(home.join(".claude.json")).unwrap_or_default();
            let installed = || {
                home.join(".claude/skills/served-kit/skills/served-guidance/SKILL.md")
                    .is_file()
            };
            let outside = tempfile::tempdir()?;

            for cwd in [workspace(&ctx), outside.path().to_path_buf()] {
                ctx.workspace_root = Some(cwd.clone());
                ctx.symposium(&["plugin", "try", &kit]).await?;
                assert!(registered().contains("served-kit-server"), "{cwd:?}");
                assert!(installed(), "{cwd:?}");

                ctx.symposium(&["plugin", "try", "--remove", &kit]).await?;
                assert!(!registered().contains("served-kit-server"), "{cwd:?}");
                assert!(!installed(), "{cwd:?}");
            }
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
            ctx.symposium(&[
                "init",
                "--hook-scope",
                "global",
                "--add-agent",
                "claude",
                "--add-agent",
                "codex",
                "--add-agent",
                "copilot",
            ])
            .await?;
            let ws = workspace(&ctx);
            let config_dir = ctx.sym.config_dir().to_path_buf();
            let claude_config = ctx.sym.home_dir().join(".claude.json");
            let registered = || {
                symposium::state::mcp_registrations(&config_dir)
                    .iter()
                    .any(|registration| registration.name == "served-kit-server")
                    || std::fs::read_to_string(&claude_config)
                        .unwrap_or_default()
                        .contains("served-kit-server")
            };
            let outside = tempfile::tempdir()?;

            for cwd in [ws.clone(), outside.path().to_path_buf()] {
                let kit = served_kit(&ws)?;
                ctx.workspace_root = Some(cwd.clone());
                ctx.symposium(&["plugin", "try", &kit]).await?;
                assert!(registered(), "{cwd:?}");
                std::fs::remove_dir_all(&kit)?;

                ctx.symposium(&["plugin", "try", "--remove", "served-kit"])
                    .await?;

                assert!(!user_plugins(&ctx).join("served-kit").exists(), "{cwd:?}");
                assert!(
                    !installed_anywhere(&ctx, &cwd, "served-guidance"),
                    "{cwd:?}"
                );
                assert!(!registered(), "{cwd:?}");
            }
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
