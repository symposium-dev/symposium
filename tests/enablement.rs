//! The enablement commands: `symposium use`, `search`, and `status`,
//! plus the discovery consent prompt they share a config section with.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use symposium::config::UseEntry;
use symposium::output::Output;
use symposium::pm::PackageId;
use symposium::status_command::StatusState;
use symposium_testlib::{HookStep, TestContext, TestMode, with_fixture};

/// Every installed skill directory under `parent` named `<skill_name>` or
/// `<skill_name>-<hash>`.
fn find_installed_skills(parent: &Path, skill_name: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let matches = name == skill_name
            || (name.starts_with(skill_name)
                && name.as_bytes().get(skill_name.len()) == Some(&b'-'));
        if matches && path.join("SKILL.md").is_file() {
            out.push(path);
        }
    }
    out.sort();
    out
}

/// The unique installed skill directory with this name. Panics on 0 or >1.
fn find_installed_skill(parent: &Path, skill_name: &str) -> PathBuf {
    let mut hits = find_installed_skills(parent, skill_name);
    assert_eq!(
        hits.len(),
        1,
        "expected exactly one installed skill named `{skill_name}` under {}, found {hits:?}",
        parent.display(),
    );
    hits.pop().unwrap()
}

fn read_config(ctx: &TestContext) -> String {
    std::fs::read_to_string(ctx.sym.config_dir().join("config.toml")).unwrap()
}

fn snapshot(ctx: &TestContext) -> BTreeMap<PathBuf, Option<String>> {
    let mut entries = BTreeMap::new();
    let mut pending = vec![ctx.tempdir.clone()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path == ctx.sym.cache_dir() {
                continue;
            }
            let relative = path.strip_prefix(&ctx.tempdir).unwrap().to_path_buf();
            if path.is_dir() {
                entries.insert(relative, None);
                pending.push(path);
            } else {
                entries.insert(relative, Some(std::fs::read_to_string(&path).unwrap()));
            }
        }
    }
    entries
}

fn record_git_fetch(ctx: &TestContext, url: &str) -> PathBuf {
    let cached =
        symposium_install::git::GitCacheManager::new(&ctx.sym.install_context(), "plugins")
            .cache_path_for_url(url)
            .unwrap();
    std::fs::create_dir_all(&cached).unwrap();
    std::fs::write(
        cached.join(symposium_install::git::CACHE_META_FILENAME),
        serde_json::json!({
            "commit_sha": "0000000000000000000000000000000000000000",
            "fetched_at": chrono::Utc::now().to_rfc3339(),
            "source_url": url,
        })
        .to_string(),
    )
    .unwrap();
    cached
}

fn fetch_facet_review(ctx: &TestContext) {
    let url = "https://github.com/example/facet-skills/tree/main/facet-review";
    std::fs::write(
        record_git_fetch(ctx, url).join("SKILL.md"),
        "---\nname: facet-review\ndescription: Review guidance fetched by facet-kit\n---\n\nReview guidance.\n",
    )
    .unwrap();
}

/// Run `f` with a JSON report layer installed and return the drained events.
async fn with_report<F>(f: F) -> Vec<serde_json::Value>
where
    F: AsyncFnOnce(),
{
    use symposium::report::{ReportLayer, ReportMode};
    use tracing_subscriber::layer::SubscriberExt;

    symposium_testlib::install_tracing_baseline();
    let (layer, handle) = ReportLayer::new(ReportMode::Json, tracing::Level::INFO);
    let subscriber = tracing_subscriber::registry().with(layer);
    let guard = tracing::subscriber::set_default(subscriber);
    f().await;
    drop(guard);
    handle.drain()
}

// ── use ──────────────────────────────────────────────────────────────

/// `use <dep>` records a workspace-scoped entry and installs the
/// dependency's embedded skills right away; running it again changes
/// nothing.
#[tokio::test]
async fn use_records_workspace_entry_and_installs() {
    with_fixture(
        TestMode::SimulationOnly,
        &["auto-enable0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "codex"]).await?;
            let skills_dir = ctx.workspace_root.clone().unwrap().join(".agents/skills");

            // Unconsented, the dependency's skills stay out.
            ctx.symposium(&["sync"]).await?;
            assert!(find_installed_skills(&skills_dir, "a-guidance").is_empty());

            ctx.symposium(&["use", "crate-a"]).await?;
            find_installed_skill(&skills_dir, "a-guidance");

            let config = read_config(&ctx);
            assert!(config.contains("crate-a"), "entry recorded: {config}");
            assert!(
                config.contains("workspace"),
                "entry is workspace-scoped: {config}"
            );

            ctx.symposium(&["use", "crate-a"]).await?;
            assert_eq!(config, read_config(&ctx), "re-using must not duplicate");
            ctx.symposium(&["use", "crate_a"]).await?;
            assert_eq!(
                config,
                read_config(&ctx),
                "another spelling must not duplicate"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// `--global` records a plain-string entry with no workspace scope.
#[tokio::test]
async fn use_global_records_unscoped_entry() {
    with_fixture(
        TestMode::SimulationOnly,
        &["auto-enable0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["use", "--global", "crate-a"]).await?;

            let config = read_config(&ctx);
            assert!(
                config.contains(r#"use = ["crate-a"]"#),
                "global entry is a plain string: {config}"
            );
            ctx.symposium(&["use", "--global", "crate_a"]).await?;
            assert_eq!(
                config,
                read_config(&ctx),
                "another spelling must not duplicate"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// A name that is neither a workspace dependency nor a registry hit is
/// rejected instead of recorded.
#[tokio::test]
async fn use_unknown_name_errors() {
    with_fixture(
        TestMode::SimulationOnly,
        &["auto-enable0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            let err = ctx
                .symposium(&["use", "no-such-plugin"])
                .await
                .expect_err("unknown name should be rejected");
            assert!(
                err.to_string()
                    .contains("no package named `no-such-plugin`"),
                "{err}"
            );
            assert!(
                !read_config(&ctx).contains("no-such-plugin"),
                "nothing recorded"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// A registry is a trust root, so `use`-ing something it already offers is a
/// no-op rather than a recorded entry.
#[tokio::test]
async fn use_registry_content_is_a_noop() {
    with_fixture(TestMode::SimulationOnly, &["plugins0"], async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "claude"]).await?;
        ctx.symposium(&["use", "serde-guidance"]).await?;
        assert!(
            !read_config(&ctx).contains("serde-guidance"),
            "{}",
            read_config(&ctx)
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn use_matches_registry_plugin_names_exactly() {
    with_fixture(
        TestMode::SimulationOnly,
        &["dormant-plugin0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            let err = ctx
                .symposium(&["use", "gateless_plugin"])
                .await
                .expect_err("a registry plugin name is not a crate name");
            assert!(
                err.to_string()
                    .contains("no package named `gateless_plugin`"),
                "{err}"
            );
            assert!(!read_config(&ctx).contains("gateless_plugin"));
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// A crate name matches the way cargo matches it, so `crate_a` names the
/// dependency `crate-a` in `auto-enable`, `use` and `disable` alike.
#[tokio::test]
async fn plugins_entries_match_crate_names_like_cargo() {
    with_fixture(
        TestMode::SimulationOnly,
        &["auto-enable0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            let project_plugin = ctx
                .workspace_root
                .clone()
                .unwrap()
                .join(".claude/skills/crate-a/skills/a-guidance/SKILL.md");
            let installed = || project_plugin.is_file();

            ctx.sym.config.plugins.auto_enable = vec!["crate_a".into()];
            ctx.symposium(&["sync"]).await?;
            assert!(installed(), "auto-enable");

            ctx.sym.config.plugins.auto_enable.clear();
            ctx.sym.config.plugins.used = vec![UseEntry::Global("crate_a".into())];
            ctx.symposium(&["sync"]).await?;
            let user_plugin = ctx.sym.home_dir().join(".claude/skills/crate-a");
            assert!(
                user_plugin.join("skills/a-guidance/SKILL.md").is_file(),
                "use"
            );

            ctx.sym.config.plugins.used.clear();
            ctx.sym.config.plugins.auto_enable = vec!["*".into()];
            ctx.sym.config.plugins.disable = vec!["crate_a".into()];
            ctx.symposium(&["sync"]).await?;
            assert!(!installed(), "disable");
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// A registry plugin's name matches exactly: `use = ["gateless_plugin"]`
/// neither wakes the dormant `gateless-plugin` nor is taken for a crate, and
/// `disable = ["gateless_plugin"]` does not block it. `auto-enable` is consent
/// for dependencies, so it wakes no registry plugin whatever the spelling.
#[tokio::test]
async fn plugins_entries_match_registry_plugin_names_exactly() {
    with_fixture(
        TestMode::SimulationOnly,
        &["dormant-plugin0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "codex"]).await?;
            let skills_dir = ctx.workspace_root.clone().unwrap().join(".agents/skills");
            let woken = || !find_installed_skills(&skills_dir, "gateless-guidance").is_empty();

            ctx.sym.config.plugins.auto_enable = vec!["gateless-plugin".into()];
            ctx.sym.config.plugins.used = vec![UseEntry::Global("gateless_plugin".into())];
            ctx.symposium(&["sync"]).await?;
            assert!(!woken(), "auto-enable and a folded `use` spelling");

            ctx.sym.config.plugins.auto_enable.clear();
            ctx.sym.config.plugins.used.clear();
            ctx.sym.config.plugins.disable = vec!["gateless_plugin".into()];
            ctx.symposium(&["use", "gateless-plugin"]).await?;
            assert!(woken(), "a folded `disable` spelling");

            ctx.symposium(&["use", "--remove", "gateless-plugin"])
                .await?;
            ctx.sym.config.plugins.disable = vec!["gateless-plugin".into()];
            let err = ctx
                .symposium(&["use", "gateless-plugin"])
                .await
                .expect_err("the exact `disable` spelling blocks it");
            assert!(err.to_string().contains("[plugins] disable"), "{err}");
            assert!(!woken());
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn use_global_installs_registry_content_for_the_user() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugins0", "workspace0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["use", "--global", "serde-guidance"])
                .await?;
            assert!(
                read_config(&ctx).contains("serde-guidance"),
                "{}",
                read_config(&ctx)
            );
            let plugin = ctx.sym.home_dir().join(".claude/skills/serde-guidance");
            assert!(plugin.join("skills/serde-guidance/SKILL.md").is_file());
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn a_global_package_reaches_a_workspace_it_does_not_match() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugins0", "workspace0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["use", "--global", "serde-guidance"])
                .await?;
            let plugin = ctx.sym.home_dir().join(".claude/skills/serde-guidance");
            std::fs::remove_dir_all(&plugin)?;

            let other = ctx.tempdir.join("other-workspace");
            std::fs::create_dir_all(other.join("src"))?;
            std::fs::write(
                other.join("Cargo.toml"),
                "[package]\nname = \"other-workspace\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            )?;
            std::fs::write(other.join("src/lib.rs"), "")?;
            ctx.workspace_root = Some(other);

            ctx.prompt_or_hook(
                "hello",
                &[HookStep::session_start()],
                symposium::hook_schema::HookAgent::Claude,
            )
            .await?;
            assert!(plugin.join("skills/serde-guidance/SKILL.md").is_file());
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn use_global_installs_a_crate_under_its_version() {
    with_fixture(
        TestMode::SimulationOnly,
        &["auto-enable0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "codex"]).await?;
            ctx.symposium(&["use", "--global", "crate-a"]).await?;

            let plugin = ctx
                .sym
                .home_dir()
                .join(".codex/plugins/cache/symposium/crate-a");
            let versions: Vec<_> = std::fs::read_dir(&plugin)?
                .flatten()
                .filter(|entry| entry.path().is_dir())
                .map(|entry| entry.file_name())
                .collect();
            assert_eq!(versions, ["0.1.0"]);
            assert!(plugin.join("0.1.0/skills/a-guidance/SKILL.md").is_file());
            let manifest: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(plugin.join("0.1.0/plugin.json"))?)?;
            assert_eq!(manifest["version"], "0.1.0");
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn use_global_again_changes_nothing() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-facets0", "workspace-empty0"],
        async |mut ctx| {
            let home = ctx.sym.home_dir().to_path_buf();
            let project = ctx.workspace_root.clone().unwrap();
            let agents = [
                (
                    "claude",
                    home.join(".claude/skills/facet-kit"),
                    project.join(".mcp.json"),
                ),
                (
                    "codex",
                    home.join(".codex/plugins/cache/symposium/facet-kit/0.0.0"),
                    home.join(".codex/config.toml"),
                ),
                (
                    "copilot",
                    ctx.sym.config_dir().join("installed/facet-kit"),
                    home.join(".copilot/mcp-config.json"),
                ),
                (
                    "antigravity",
                    home.join(".gemini/config/plugins/facet-kit"),
                    project.join(".agents/mcp_config.json"),
                ),
                (
                    "goose",
                    home.join(".agents/plugins/facet-kit"),
                    home.join(".config/goose/config.yaml"),
                ),
                (
                    "kiro",
                    project.join(".kiro"),
                    project.join(".kiro/settings/mcp.json"),
                ),
                (
                    "opencode",
                    project.join(".opencode"),
                    project.join("opencode.json"),
                ),
            ];
            let mut init = vec!["init"];
            for (agent, ..) in &agents {
                init.extend(["--add-agent", *agent]);
            }
            ctx.sym.config.sync_debounce_secs = 0;
            ctx.symposium(&init).await?;

            fetch_facet_review(&ctx);
            ctx.symposium(&["use", "--global", "facet-kit"]).await?;
            let installed = snapshot(&ctx);
            fetch_facet_review(&ctx);
            ctx.symposium(&["use", "--global", "facet-kit"]).await?;
            let reinstalled = snapshot(&ctx);

            let changed: BTreeMap<_, _> = installed
                .keys()
                .chain(reinstalled.keys())
                .filter(|path| installed.get(*path) != reinstalled.get(*path))
                .map(|path| (path, (installed.get(path), reinstalled.get(path))))
                .collect();
            assert!(
                changed.is_empty(),
                "the second run changed (before, after): {changed:#?}"
            );

            let config = read_config(&ctx);
            assert!(config.contains(r#"use = ["facet-kit"]"#), "{config}");

            let bookkeeping: Vec<_> = installed
                .keys()
                .filter(|path| path.ends_with(symposium_install::git::CACHE_META_FILENAME))
                .collect();
            assert!(bookkeeping.is_empty(), "installed: {bookkeeping:#?}");

            for (agent, skills_root, mcp_config) in &agents {
                for skill in ["facet-guidance", "facet-review"] {
                    assert!(
                        skills_root
                            .join("skills")
                            .join(skill)
                            .join("SKILL.md")
                            .is_file(),
                        "{agent} has no {skill} under {}",
                        skills_root.display()
                    );
                }
                let mcp_config = std::fs::read_to_string(mcp_config)?;
                assert_eq!(
                    mcp_config.matches("--facet-kit-stdio").count(),
                    1,
                    "{agent} MCP servers: {mcp_config}"
                );
            }

            for hooks in [
                ".claude/settings.json",
                ".codex/hooks.json",
                ".github/hooks/symposium.json",
                ".agents/hooks.json",
                ".kiro/agents/symposium.json",
            ] {
                let registered = std::fs::read_to_string(project.join(hooks))?;
                let commands: Vec<&str> = registered
                    .split('"')
                    .filter(|value| value.starts_with("symposium hook"))
                    .collect();
                let distinct: BTreeSet<&str> = commands.iter().copied().collect();
                assert!(
                    !commands.is_empty() && commands.len() == distinct.len(),
                    "{hooks}: {commands:?}"
                );
            }

            let fired = ctx
                .prompt_or_hook(
                    "ignored",
                    &[HookStep::PreToolUse {
                        tool_name: "Bash".to_string(),
                        tool_input: serde_json::json!({ "command": "ls" }),
                    }],
                    symposium::hook_schema::HookAgent::Claude,
                )
                .await?;
            let context = fired.hooks[0].output["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .unwrap_or_default();
            assert_eq!(
                context.matches("facet-kit-hook-fired").count(),
                1,
                "{:#?}",
                fired.hooks
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn use_global_outside_a_workspace_installs_for_the_user() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-facets0"],
        async |mut ctx| {
            assert!(ctx.workspace_root.is_none());
            let home = ctx.sym.home_dir().to_path_buf();
            let installed = [
                (
                    "claude",
                    "Claude Code",
                    home.join(".claude/skills/facet-kit"),
                ),
                (
                    "codex",
                    "Codex CLI",
                    home.join(".codex/plugins/cache/symposium/facet-kit/0.0.0"),
                ),
                (
                    "copilot",
                    "GitHub Copilot",
                    ctx.sym.config_dir().join("installed/facet-kit"),
                ),
                (
                    "antigravity",
                    "Antigravity CLI",
                    home.join(".gemini/config/plugins/facet-kit"),
                ),
                ("goose", "Goose", home.join(".agents/plugins/facet-kit")),
            ];
            let mut init = vec!["init", "--add-agent", "kiro", "--add-agent", "opencode"];
            for (agent, ..) in &installed {
                init.extend(["--add-agent", *agent]);
            }
            ctx.symposium(&init).await?;
            fetch_facet_review(&ctx);

            let reported = with_report(async || {
                ctx.symposium(&["use", "--global", "facet-kit"])
                    .await
                    .unwrap();
            })
            .await;

            for (agent, _, plugin) in &installed {
                for skill in ["facet-guidance", "facet-review"] {
                    assert!(
                        plugin.join("skills").join(skill).join("SKILL.md").is_file(),
                        "{agent} has no {skill} under {}",
                        plugin.display()
                    );
                }
            }
            for registered in [".codex/config.toml", ".copilot/settings.json"] {
                let registered = std::fs::read_to_string(home.join(registered))?;
                assert!(registered.contains("facet-kit@symposium"), "{registered}");
            }
            let not_installed: Vec<&str> = reported
                .iter()
                .filter(|event| event["kind"] == "info")
                .filter_map(|event| event["message"].as_str())
                .filter(|message| message.contains("Kiro") && message.contains("OpenCode"))
                .collect();
            assert_eq!(not_installed.len(), 1, "{reported:#?}");
            for (_, name, _) in &installed {
                assert!(!not_installed[0].contains(name), "{not_installed:?}");
            }
            let not_registered = reported
                .iter()
                .filter_map(|event| event["message"].as_str())
                .filter(|message| message.contains("MCP"))
                .count();
            assert_eq!(not_registered, 1, "{reported:#?}");
            for (agent, file) in user_mcp_files(&ctx) {
                let registered = std::fs::read_to_string(&file).unwrap_or_default();
                assert!(
                    !registered.contains("--facet-kit-stdio"),
                    "{agent}: {registered}"
                );
            }

            ctx.symposium(&["use", "--global", "--remove", "facet-kit"])
                .await?;
            for (agent, _, plugin) in &installed {
                assert!(!plugin.exists(), "{agent} keeps {}", plugin.display());
            }
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn use_global_outside_a_workspace_evaluates_custom_predicates() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-facets0"],
        async |mut ctx| {
            let kit = ctx.sym.config_dir().join("plugins/checked-kit");
            std::fs::create_dir_all(&kit)?;
            std::fs::write(kit.join("check.sh"), "exit 0\n")?;
            let mut manifest = String::from(
                "name = \"checked-kit\"\n\n\
                 [[installations]]\nname = \"check\"\nscript = \"check.sh\"\n\n\
                 [[predicate]]\nname = \"checked\"\ncommand = \"check\"\n",
            );
            for (server, gate) in [("passing", "checked(on)"), ("failing", "not(checked(on))")] {
                manifest.push_str(&format!(
                    "\n[[mcp_servers]]\nname = \"checked-kit-{server}\"\n\
                     command = \"/usr/bin/true\"\nargs = []\nenv = []\npredicates = [\"{gate}\"]\n"
                ));
            }
            std::fs::write(kit.join("Symposium.toml"), manifest)?;
            ctx.symposium(&["init", "--hook-scope", "global", "--add-agent", "claude"])
                .await?;

            ctx.symposium(&["use", "--global", "checked-kit"]).await?;

            let registered = std::fs::read_to_string(ctx.sym.home_dir().join(".claude.json"))?;
            assert!(registered.contains("checked-kit-passing"), "{registered}");
            assert!(!registered.contains("checked-kit-failing"), "{registered}");
            Ok(())
        },
    )
    .await
    .unwrap();
}

fn user_mcp_files(ctx: &TestContext) -> [(&'static str, PathBuf); 7] {
    let home = ctx.sym.home_dir();
    [
        ("claude", home.join(".claude.json")),
        ("codex", home.join(".codex/config.toml")),
        ("copilot", home.join(".copilot/mcp-config.json")),
        ("antigravity", home.join(".gemini/config/mcp_config.json")),
        ("goose", home.join(".config/goose/config.yaml")),
        ("kiro", home.join(".kiro/settings/mcp.json")),
        ("opencode", home.join(".config/opencode/opencode.json")),
    ]
}

fn registrations_of_facet_kit(ctx: &TestContext) -> Vec<(&'static str, usize)> {
    user_mcp_files(ctx)
        .into_iter()
        .map(|(agent, file)| {
            let registered = std::fs::read_to_string(file).unwrap_or_default();
            (agent, registered.matches("--facet-kit-stdio").count())
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn use_global_outside_a_workspace_registers_mcp_servers_for_the_user() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-facets0"],
        async |mut ctx| {
            let mut init = vec!["init", "--hook-scope", "global"];
            for (agent, _) in user_mcp_files(&ctx) {
                init.extend(["--add-agent", agent]);
            }
            ctx.sym.config.sync_debounce_secs = 0;
            ctx.symposium(&init).await?;
            fetch_facet_review(&ctx);

            ctx.symposium(&["use", "--global", "facet-kit"]).await?;
            for (agent, count) in registrations_of_facet_kit(&ctx) {
                assert_eq!(count, 1, "{agent}");
            }

            let installed = snapshot(&ctx);
            fetch_facet_review(&ctx);
            ctx.symposium(&["use", "--global", "facet-kit"]).await?;
            let reinstalled = snapshot(&ctx);
            let changed: Vec<_> = installed
                .keys()
                .chain(reinstalled.keys())
                .filter(|path| installed.get(*path) != reinstalled.get(*path))
                .collect();
            assert!(changed.is_empty(), "the second run changed {changed:#?}");

            ctx.symposium(&["use", "--global", "--remove", "facet-kit"])
                .await?;
            for (agent, count) in registrations_of_facet_kit(&ctx) {
                assert_eq!(count, 0, "{agent}");
            }
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn use_remove_unregisters_the_mcp_servers_of_the_plugin() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-facets0", "workspace-empty0"],
        async |mut ctx| {
            let own_entry = project_mcp_entry(&ctx);
            let mut init = vec!["init", "--hook-scope", "global"];
            for (agent, _) in user_mcp_files(&ctx) {
                init.extend(["--add-agent", agent]);
            }
            ctx.symposium(&init).await?;
            fetch_facet_review(&ctx);

            for scope in [&[][..], &["--global"]] {
                ctx.symposium(&[&["use", "facet-kit"][..], scope].concat())
                    .await?;
                for (agent, count) in registrations_of_facet_kit(&ctx) {
                    assert_eq!(count, 1, "{agent} {scope:?}");
                }
                ctx.symposium(&[&["use", "--remove", "facet-kit"][..], scope].concat())
                    .await?;
                for (agent, count) in registrations_of_facet_kit(&ctx) {
                    assert_eq!(count, 0, "{agent} {scope:?}");
                }
            }
            assert_eq!(project_mcp_entry(&ctx), own_entry);
            Ok(())
        },
    )
    .await
    .unwrap();
}

fn project_mcp_entry(ctx: &TestContext) -> String {
    let file = ctx.workspace_root.as_ref().unwrap().join(".mcp.json");
    if !file.exists() {
        std::fs::write(
            &file,
            r#"{"mcpServers":{"facet-kit-server":{"command":"mine"}}}"#,
        )
        .unwrap();
    }
    std::fs::read_to_string(file).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn use_remove_unregisters_the_mcp_servers_of_a_nested_registry_entry() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-facets0"],
        async |mut ctx| {
            let entry = ctx.sym.config_dir().join("plugins/tools/widget");
            std::fs::create_dir_all(&entry)?;
            std::fs::write(
                entry.join("Symposium.toml"),
                "name = \"widget\"\n\n[[mcp_servers]]\nname = \"widget-server\"\n\
                 command = \"/usr/bin/true\"\nargs = []\nenv = []\n",
            )?;
            ctx.symposium(&["init", "--hook-scope", "global", "--add-agent", "claude"])
                .await?;
            let claude = ctx.sym.home_dir().join(".claude.json");
            let registered = || std::fs::read_to_string(&claude).unwrap_or_default();

            ctx.symposium(&["use", "--global", "widget"]).await?;
            assert!(registered().contains("widget-server"), "{}", registered());
            ctx.symposium(&["use", "--global", "--remove", "widget"])
                .await?;

            assert!(!registered().contains("widget-server"), "{}", registered());
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// `use` wakes a dormant registry plugin (one no dependency gates), and
/// `--remove` puts it back to sleep, reaping its skills.
#[tokio::test]
async fn use_wakes_and_remove_sleeps_a_dormant_plugin() {
    with_fixture(
        TestMode::SimulationOnly,
        &["dormant-plugin0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "codex"]).await?;
            let skills_dir = ctx.workspace_root.clone().unwrap().join(".agents/skills");

            ctx.symposium(&["sync"]).await?;
            assert!(find_installed_skills(&skills_dir, "gateless-guidance").is_empty());

            ctx.symposium(&["use", "gateless-plugin"]).await?;
            find_installed_skill(&skills_dir, "gateless-guidance");
            assert!(read_config(&ctx).contains("gateless-plugin"));

            ctx.symposium(&["use", "--remove", "gateless-plugin"])
                .await?;
            assert!(find_installed_skills(&skills_dir, "gateless-guidance").is_empty());
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// `--remove` drops the entry, reaps the installed skills, and errors when
/// there is nothing left to remove.
#[tokio::test]
async fn use_remove_reaps_and_then_errors() {
    with_fixture(
        TestMode::SimulationOnly,
        &["auto-enable0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "codex"]).await?;
            let skills_dir = ctx.workspace_root.clone().unwrap().join(".agents/skills");

            ctx.symposium(&["use", "crate-a"]).await?;
            find_installed_skill(&skills_dir, "a-guidance");

            ctx.symposium(&["use", "--remove", "crate-a"]).await?;
            assert!(
                !read_config(&ctx).contains("crate-a"),
                "entry removed: {}",
                read_config(&ctx)
            );
            assert!(
                find_installed_skills(&skills_dir, "a-guidance").is_empty(),
                "skills reaped after removal"
            );

            let err = ctx
                .symposium(&["use", "--remove", "crate-a"])
                .await
                .expect_err("nothing left to remove");
            assert!(err.to_string().contains("no `use` entry"), "{err}");
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// `--remove --global` targets only the global entry, leaving a
/// workspace-scoped one alone.
#[tokio::test]
async fn use_remove_respects_scope() {
    with_fixture(
        TestMode::SimulationOnly,
        &["auto-enable0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["use", "crate-a"]).await?;

            let err = ctx
                .symposium(&["use", "--remove", "--global", "crate-a"])
                .await
                .expect_err("no global entry to remove");
            assert!(err.to_string().contains("no `use` entry"), "{err}");
            assert!(
                read_config(&ctx).contains("crate-a"),
                "workspace entry untouched"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

// ── search ───────────────────────────────────────────────────────────

/// Search finds both a registry's standalone skills and its plugin entries,
/// tagged with the instance they came from.
#[tokio::test]
async fn search_finds_registry_content_tagged_by_origin() {
    with_fixture(TestMode::SimulationOnly, &["plugins0"], async |mut ctx| {
        let matches = symposium::search_command::find_matches(&ctx.sym, "SERDE-gui").await;
        assert!(
            matches.iter().any(|m| m.name == "serde-guidance"),
            "case-insensitive substring match: {matches:?}"
        );

        let matches = symposium::search_command::find_matches(&ctx.sym, "my-skill").await;
        assert!(
            matches
                .iter()
                .any(|m| m.origin == "user-plugins" && m.name == "my-skill"),
            "registry entry tagged with its instance name: {matches:?}"
        );

        let matches = symposium::search_command::find_matches(&ctx.sym, "MY-SKILL").await;
        assert!(
            matches
                .iter()
                .any(|m| m.origin == "user-plugins" && m.name == "my-skill"),
            "registry entries match case-insensitively: {matches:?}"
        );

        // A query nothing matches finds nothing (and does not fail).
        assert!(
            symposium::search_command::find_matches(&ctx.sym, "zzz-nothing")
                .await
                .is_empty()
        );

        ctx.symposium(&["search", "serde"]).await?;
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn search_lists_a_registry_entry_once() {
    with_fixture(TestMode::SimulationOnly, &["plugins0"], async |ctx| {
        let entry = ctx.sym.config_dir().join("plugins/tokio-guidance");
        std::fs::create_dir_all(&entry)?;
        std::fs::write(
            entry.join("SKILL.md"),
            "---\nname: tokio-guidance\ndescription: Guidance for tokio\ndepends-on: tokio\n---\n",
        )?;
        let matches = symposium::search_command::find_matches(&ctx.sym, "tokio-guidance").await;
        let hits: Vec<_> = matches
            .iter()
            .filter(|m| m.origin == "user-plugins" && m.name == "tokio-guidance")
            .collect();
        assert_eq!(hits.len(), 1, "{matches:#?}");
        Ok(())
    })
    .await
    .unwrap();
}

/// The rendered report groups hits under their origin and carries the
/// per-hit detail.
#[tokio::test]
async fn search_renders_grouped_by_origin() {
    with_fixture(TestMode::SimulationOnly, &["plugins0"], async |ctx| {
        let events = with_report(async || {
            symposium::search_command::search(&ctx.sym, "serde")
                .await
                .unwrap();
        })
        .await;

        let kinds: Vec<&str> = events
            .iter()
            .filter_map(|e| e["kind"].as_str())
            .collect::<Vec<_>>();
        assert!(
            kinds.contains(&"search_origin") && kinds.contains(&"search_match"),
            "a group heading plus at least one hit: {events:?}"
        );

        // The bare skill is searched as the plugin it now is: matched by its
        // name, tagged with its registry origin. Plugins carry no per-skill
        // description in search (only a dormancy hint, and this one is active).
        let hit = events
            .iter()
            .find(|e| e["kind"] == "search_match")
            .expect("a search_match event");
        assert_eq!(hit["name"], "serde-guidance");
        assert_eq!(hit["origin"], "user-plugins");

        let events = with_report(async || {
            symposium::search_command::search(&ctx.sym, "zzz-nothing")
                .await
                .unwrap();
        })
        .await;
        assert_eq!(events.len(), 1, "just the nothing-found notice: {events:?}");
        assert_eq!(events[0]["kind"], "info");
        Ok(())
    })
    .await
    .unwrap();
}

/// `disable` is the last word, so `use` on a declined plugin is refused
/// rather than recorded. Recording it would report success and then install
/// nothing, since activation refuses the plugin too.
#[tokio::test]
async fn use_on_a_disabled_plugin_errors() {
    with_fixture(
        TestMode::SimulationOnly,
        &["auto-enable0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            let pms = ctx.sym.detached_managers();
            symposium::discovery::apply_consent(
                &mut ctx.sym,
                &pms,
                &[],
                &[PackageId::any_version("cargo", "crate-a")],
            )
            .await?;

            let err = ctx
                .symposium(&["use", "crate-a"])
                .await
                .expect_err("a disabled plugin cannot be enabled by name");
            assert!(
                err.to_string().contains("disabled in `[plugins] disable`"),
                "{err}"
            );
            let config = read_config(&ctx);
            assert!(!config.contains("use = "), "nothing recorded: {config}");
            Ok(())
        },
    )
    .await
    .unwrap();
}

// ── status ───────────────────────────────────────────────────────────

/// `status` reports an undecided dependency plugin as a candidate, then as
/// active with its `use` root once enabled — and names declined entries.
#[tokio::test]
async fn status_reports_candidate_then_used() {
    with_fixture(
        TestMode::SimulationOnly,
        &["auto-enable0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            let workspace_root = ctx.workspace_root.clone().unwrap();

            let deps = ctx.sym.workspace(&workspace_root);
            let entries = symposium::status_command::workspace_status(&ctx.sym, &deps).await?;
            let candidate = entries
                .iter()
                .find(|e| e.name == "crate-a")
                .expect("crate-a discovered");
            assert_eq!(candidate.state, StatusState::Candidate);
            assert!(candidate.root.contains("awaiting consent"), "{candidate:?}");

            ctx.symposium(&["use", "crate-a"]).await?;
            let pms = ctx.sym.detached_managers();
            symposium::discovery::apply_consent(
                &mut ctx.sym,
                &pms,
                &[],
                &[PackageId::any_version("cargo", "noisy-crate")],
            )
            .await?;

            let deps = ctx.sym.workspace(&workspace_root);
            let entries = symposium::status_command::workspace_status(&ctx.sym, &deps).await?;
            let used = entries
                .iter()
                .find(|e| e.name == "crate-a")
                .expect("crate-a present");
            assert_eq!(used.state, StatusState::Active);
            assert_eq!(used.root, "`[plugins] use`");
            assert_eq!(used.version.as_deref(), Some("0.1.0"));

            let declined = entries
                .iter()
                .find(|e| e.name == "noisy-crate")
                .expect("declined entry present");
            assert_eq!(declined.state, StatusState::Declined);

            // CLI wiring, and the rendered report.
            let events = with_report(async || {
                symposium::status_command::status(&ctx.sym, &workspace_root)
                    .await
                    .unwrap();
            })
            .await;
            assert!(
                events
                    .iter()
                    .any(|e| e["kind"] == "plugin_status" && e["state"] == "active"),
                "{events:?}"
            );
            ctx.symposium(&["status"]).await?;
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// A registry plugin nothing gates is reported dormant until `use` names it.
#[tokio::test]
async fn status_reports_dormant_registry_plugin() {
    with_fixture(
        TestMode::SimulationOnly,
        &["dormant-plugin0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            let workspace_root = ctx.workspace_root.clone().unwrap();

            let deps = ctx.sym.workspace(&workspace_root);
            let entries = symposium::status_command::workspace_status(&ctx.sym, &deps).await?;
            let dormant = entries
                .iter()
                .find(|e| e.name == "gateless-plugin")
                .expect("gateless-plugin present");
            assert_eq!(dormant.state, StatusState::Dormant);
            assert!(dormant.root.contains("awaiting `symposium use`"));

            // And `search` finds it, tagged with the registry it came from
            // and flagged as needing `use`.
            let hit = symposium::search_command::find_matches(&ctx.sym, "gateless")
                .await
                .into_iter()
                .find(|m| m.name == "gateless-plugin")
                .expect("search finds the manifest plugin");
            assert_eq!(hit.origin, "user-plugins");
            assert!(
                hit.description
                    .as_deref()
                    .is_some_and(|d| d.contains("dormant")),
                "{hit:?}"
            );

            ctx.symposium(&["use", "gateless-plugin"]).await?;
            let deps = ctx.sym.workspace(&workspace_root);
            let entries = symposium::status_command::workspace_status(&ctx.sym, &deps).await?;
            let awake = entries
                .iter()
                .find(|e| e.name == "gateless-plugin")
                .expect("gateless-plugin present");
            assert_eq!(awake.state, StatusState::Active);
            assert_eq!(awake.root, "`[plugins] use`");
            Ok(())
        },
    )
    .await
    .unwrap();
}

// ── consent ──────────────────────────────────────────────────────────

/// The names of the dependency plugins awaiting consent.
async fn pending_names(
    sym: &symposium::config::Symposium,
    deps: &std::sync::Arc<symposium::pm::Workspace>,
) -> Vec<String> {
    symposium::discovery::pending_candidates(sym, deps)
        .await
        .into_iter()
        .map(|c| c.id.name)
        .collect()
}

/// The consent prompt must never block on stdin outside a terminal session.
///
/// The gate is [`Output::is_interactive`], not a bare TTY check: `cargo test`
/// inherits the developer's terminal, so a stdin-only check would make this
/// test hang on an interactive machine. Quiet and capturing outputs — the
/// ones hook dispatch and the library harness use — are never interactive.
#[tokio::test]
async fn consent_prompt_never_fires_non_interactively() {
    with_fixture(
        TestMode::SimulationOnly,
        &["auto-enable0"],
        async |mut ctx| {
            assert!(!Output::quiet().is_interactive());
            assert!(!Output::capturing().is_interactive());

            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            let workspace_root = ctx.workspace_root.clone().unwrap();

            // There *is* something to ask about — so nothing below is
            // vacuous.
            let deps = ctx.sym.workspace(&workspace_root);
            assert_eq!(pending_names(&ctx.sym, &deps).await, ["crate-a"]);

            // The prompt returns without reading stdin, recording nothing.
            let deps = ctx.sym.workspace(&workspace_root);
            let out = Output::quiet();
            symposium::discovery::prompt_for_consent(&mut ctx.sym, &deps, &out).await?;
            assert!(ctx.sym.config.plugins.auto_enable.is_empty());
            assert!(ctx.sym.config.plugins.disable.is_empty());

            // Nor does the `sync` command, whose harness output captures.
            ctx.symposium(&["sync"]).await?;
            let config = read_config(&ctx);
            assert!(!config.contains("auto-enable"), "{config}");
            assert!(!config.contains("disable"), "{config}");

            // Still undecided, and still not installed.
            let deps = ctx.sym.workspace(&workspace_root);
            assert_eq!(pending_names(&ctx.sym, &deps).await, ["crate-a"]);
            assert!(
                find_installed_skills(&workspace_root.join(".claude/skills"), "a-guidance")
                    .is_empty()
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// Recorded consent is durable in both directions: approval enables and the
/// candidate stops being offered; a decline is remembered too.
#[tokio::test]
async fn apply_consent_records_both_answers() {
    with_fixture(
        TestMode::SimulationOnly,
        &["auto-enable0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "codex"]).await?;
            let workspace_root = ctx.workspace_root.clone().unwrap();

            let pms = ctx.sym.detached_managers();
            symposium::discovery::apply_consent(
                &mut ctx.sym,
                &pms,
                &[PackageId::any_version("cargo", "crate-a")],
                &[],
            )
            .await?;
            assert!(read_config(&ctx).contains("auto-enable"));
            // Cargo's two spellings still record one decision.
            symposium::discovery::apply_consent(
                &mut ctx.sym,
                &pms,
                &[PackageId::any_version("cargo", "crate_a")],
                &[],
            )
            .await?;
            assert_eq!(ctx.sym.config.plugins.auto_enable, ["crate-a"]);

            ctx.symposium(&["sync"]).await?;
            find_installed_skill(&workspace_root.join(".agents/skills"), "a-guidance");

            let deps = ctx.sym.workspace(&workspace_root);
            assert!(
                symposium::discovery::pending_candidates(&ctx.sym, &deps)
                    .await
                    .is_empty(),
                "a decided dependency is not offered again"
            );

            let pms = ctx.sym.detached_managers();
            symposium::discovery::apply_consent(
                &mut ctx.sym,
                &pms,
                &[],
                &[PackageId::any_version("cargo", "other-dep")],
            )
            .await?;
            assert!(read_config(&ctx).contains("disable"));
            Ok(())
        },
    )
    .await
    .unwrap();
}

async fn decide_crate_d(ctx: &mut TestContext, approve: bool) -> anyhow::Result<()> {
    let workspace_root = ctx.workspace_root.clone().unwrap();
    let deps = ctx.sym.workspace(&workspace_root);
    let candidate = symposium::discovery::discover_for(&ctx.sym, &deps)
        .await
        .candidates
        .into_iter()
        .find(|c| c.id.name == "crate-d")
        .expect("crate-d is pending approval");
    let (approved, declined) = if approve {
        (vec![candidate.id], vec![])
    } else {
        (vec![], vec![candidate.id])
    };
    symposium::discovery::apply_consent(&mut ctx.sym, deps.pms(), &approved, &declined).await
}

async fn crate_d_is_offered(ctx: &mut TestContext) -> anyhow::Result<(bool, bool)> {
    let workspace_root = ctx.workspace_root.clone().unwrap();
    let deps = ctx.sym.workspace(&workspace_root);
    let pending = symposium::discovery::pending_candidates(&ctx.sym, &deps).await;
    let session = ctx
        .prompt_or_hook(
            "hello",
            &[HookStep::session_start()],
            symposium::hook_schema::HookAgent::Claude,
        )
        .await?;
    Ok((
        pending.iter().any(|c| c.name() == "crate-d"),
        session.has_context_containing("crate-d"),
    ))
}

async fn active_crate_d_facets(ctx: &mut TestContext) -> anyhow::Result<Vec<&'static str>> {
    let workspace_root = ctx.workspace_root.clone().unwrap();
    let mut active = Vec::new();
    if workspace_root
        .join(".claude/skills/crate-d/skills/d-guidance/SKILL.md")
        .is_file()
    {
        active.push("skill");
    }
    let mcp = std::fs::read_to_string(workspace_root.join(".mcp.json")).unwrap_or_default();
    if mcp.contains("d-server") {
        active.push("mcp server");
    }
    let hook = ctx
        .prompt_or_hook(
            "ignored",
            &[HookStep::PreToolUse {
                tool_name: "Task".to_string(),
                tool_input: serde_json::json!({}),
            }],
            symposium::hook_schema::HookAgent::Claude,
        )
        .await?;
    if hook.has_context_containing("d-hook-output") {
        active.push("hook");
    }
    if ctx
        .symposium(&["d-tool", "--version"])
        .await
        .is_ok_and(|out| out.contains("rustc"))
    {
        active.push("subcommand");
    }
    Ok(active)
}

async fn crate_d_status(ctx: &TestContext) -> anyhow::Result<StatusState> {
    let workspace_root = ctx.workspace_root.clone().unwrap();
    let deps = ctx.sym.workspace(&workspace_root);
    let entries = symposium::status_command::workspace_status(&ctx.sym, &deps).await?;
    Ok(entries
        .into_iter()
        .find(|e| e.name == "crate-d")
        .expect("crate-d listed")
        .state)
}

#[tokio::test(flavor = "multi_thread")]
async fn declined_dependency_plugin_stays_inactive() {
    with_fixture(
        TestMode::SimulationOnly,
        &["consent-facets0"],
        async |mut ctx| {
            ctx.sym.config.auto_update = symposium::config::AutoUpdate::Off;
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            assert_eq!(crate_d_is_offered(&mut ctx).await?, (true, true));

            decide_crate_d(&mut ctx, false).await?;
            assert_eq!(ctx.sym.config.plugins.disable, ["crate-d"]);
            ctx.sym.config.plugins.auto_enable = vec!["*".into()];
            ctx.symposium(&["sync"]).await?;

            assert_eq!(active_crate_d_facets(&mut ctx).await?, Vec::<&str>::new());
            assert_eq!(crate_d_status(&ctx).await?, StatusState::Declined);
            assert_eq!(crate_d_is_offered(&mut ctx).await?, (false, false));
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn approved_dependency_plugin_activates() {
    with_fixture(
        TestMode::SimulationOnly,
        &["consent-facets0"],
        async |mut ctx| {
            ctx.sym.config.auto_update = symposium::config::AutoUpdate::Off;
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            assert_eq!(crate_d_is_offered(&mut ctx).await?, (true, true));

            decide_crate_d(&mut ctx, true).await?;
            ctx.symposium(&["sync"]).await?;

            assert_eq!(
                active_crate_d_facets(&mut ctx).await?,
                ["skill", "mcp server", "hook", "subcommand"]
            );
            assert_eq!(crate_d_status(&ctx).await?, StatusState::Active);
            assert_eq!(crate_d_is_offered(&mut ctx).await?, (false, false));
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// Non-interactively, pending candidates surface as `SessionStart` context
/// rather than a prompt — and the agent is told not to act on them itself.
#[tokio::test]
async fn session_start_hints_pending_candidates() {
    with_fixture(
        TestMode::SimulationOnly,
        &["auto-enable0"],
        async |mut ctx| {
            ctx.sym.config.auto_update = symposium::config::AutoUpdate::Off;
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;

            let result = ctx
                .prompt_or_hook(
                    "hello",
                    &[HookStep::session_start()],
                    symposium::hook_schema::HookAgent::Claude,
                )
                .await?;

            let context = result
                .hooks
                .iter()
                .filter_map(|h| {
                    h.output
                        .get("additionalContext")
                        .and_then(|v| v.as_str())
                        .or_else(|| {
                            h.output
                                .get("hookSpecificOutput")
                                .and_then(|o| o.get("additionalContext"))
                                .and_then(|v| v.as_str())
                        })
                })
                .next()
                .expect("session-start should produce additionalContext");

            // No audits list is read here, so the candidate is named with no
            // audit answer at all.
            assert!(
                context.contains(
                    "not enabled yet: crate-a. They stay off until the user consents. Tell"
                ),
                "{context}"
            );
            assert!(context.contains("symposium use"), "{context}");
            assert!(context.contains("Do not enable them yourself"), "{context}");
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// A plugin named by both `use` and `disable` is reported declined, matching
/// what activation does with it. The two must agree, or `status` says a
/// plugin is active while it never runs.
#[tokio::test]
async fn status_reports_a_used_but_disabled_plugin_as_declined() {
    with_fixture(
        TestMode::SimulationOnly,
        &["auto-enable0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            let workspace_root = ctx.workspace_root.clone().unwrap();

            ctx.symposium(&["use", "crate-a"]).await?;
            // Straight into the config: `use` itself now refuses this pairing,
            // but a hand-edited config can still hold both.
            let pms = ctx.sym.detached_managers();
            symposium::discovery::apply_consent(
                &mut ctx.sym,
                &pms,
                &[],
                &[PackageId::any_version("cargo", "crate-a")],
            )
            .await?;

            let deps = ctx.sym.workspace(&workspace_root);
            let entries = symposium::status_command::workspace_status(&ctx.sym, &deps).await?;
            let entry = entries
                .iter()
                .find(|e| e.name == "crate-a")
                .expect("crate-a present");
            assert_eq!(entry.state, StatusState::Declined);
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn status_lists_each_declined_name_once() {
    with_fixture(
        TestMode::SimulationOnly,
        &["auto-enable0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            let workspace_root = ctx.workspace_root.clone().unwrap();
            ctx.sym.config.plugins.disable = ["crate_a", "*", "never-seen", "never_seen"]
                .map(String::from)
                .to_vec();

            let deps = ctx.sym.workspace(&workspace_root);
            let entries = symposium::status_command::workspace_status(&ctx.sym, &deps).await?;
            let named = |name: &str| {
                entries
                    .iter()
                    .filter(|e| e.name.replace('_', "-") == name)
                    .collect::<Vec<_>>()
            };

            let crate_a = named("crate-a");
            assert_eq!(crate_a.len(), 1, "{entries:?}");
            assert_eq!(crate_a[0].state, StatusState::Declined);
            assert_eq!(crate_a[0].version.as_deref(), Some("0.1.0"));
            assert!(named("*").is_empty(), "{entries:?}");
            // No package manager has `never-seen`, so nothing says the two
            // spellings are one package.
            let never_seen = named("never-seen");
            assert_eq!(never_seen.len(), 2, "{entries:?}");
            assert!(never_seen.iter().all(|e| e.state == StatusState::Declined));
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn status_reports_a_pluginless_dependency_only_when_used() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plain-dep0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            let workspace_root = ctx.workspace_root.clone().unwrap();
            ctx.sym.config.plugins.auto_enable = vec!["*".into()];
            ctx.sym
                .config
                .plugins
                .used
                .push(symposium::config::UseEntry::Global("not-a-dep".into()));

            let deps = ctx.sym.workspace(&workspace_root);
            let entries = symposium::status_command::workspace_status(&ctx.sym, &deps).await?;
            assert!(
                !entries.iter().any(|e| e.name == "plain-dep"),
                "{entries:?}"
            );
            let crate_a = entries
                .iter()
                .find(|e| e.name == "crate-a")
                .expect("crate-a present");
            assert_eq!(crate_a.state, StatusState::Active);
            assert_eq!(crate_a.root, "`[plugins] auto-enable`");
            let not_a_dep = entries
                .iter()
                .find(|e| e.name == "not-a-dep")
                .expect("not-a-dep present");
            assert_eq!(not_a_dep.state, StatusState::Dormant);
            assert!(not_a_dep.root.contains("cached crate"), "{not_a_dep:?}");

            ctx.sym.config.plugins.auto_enable.clear();
            ctx.symposium(&["use", "plain-dep"]).await?;
            let deps = ctx.sym.workspace(&workspace_root);
            let entries = symposium::status_command::workspace_status(&ctx.sym, &deps).await?;
            let plain = entries
                .iter()
                .find(|e| e.name == "plain-dep")
                .expect("plain-dep present once used");
            assert_eq!(plain.state, StatusState::Dormant);
            assert!(!plain.root.contains("not a dependency"), "{plain:?}");
            Ok(())
        },
    )
    .await
    .unwrap();
}

// A registry plugin wins name resolution over a same-named dependency.
// Choosing it must not also grant consent to the dependency's embedded plugin.

#[tokio::test]
async fn choosing_registry_plugin_does_not_enable_same_named_dependency() {
    with_fixture(TestMode::SimulationOnly, &["auto-enable0"], async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "claude"]).await?;
        ctx.sym.config.defaults.user_plugins = true;
        let plugin_dir = ctx.sym.config_dir().join("plugins/registry-choice");
        std::fs::create_dir_all(&plugin_dir)?;
        std::fs::write(plugin_dir.join("Symposium.toml"), "name = \"crate-a\"\n")?;
        let registry = symposium::plugins::load_registry(&ctx.sym).await;
        assert!(registry.plugins.iter().any(|plugin|
            plugin.manifest.name == "crate-a" && plugin.manifest.requires_use));
        ctx.symposium(&["use", "crate-a"]).await?;
        let skills = ctx.workspace_root.as_ref().unwrap().join(".claude/skills");
        let installed = find_installed_skills(&skills, "a-guidance");
        assert!(installed.is_empty(),
            "enabling the registry plugin also installed the unconsented dependency: {installed:?}");
        Ok(())
    }).await.unwrap();
}
