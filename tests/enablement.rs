//! The enablement commands: `cargo agents use`, `search`, and `status`,
//! plus the discovery consent prompt they share a config section with.

use std::path::{Path, PathBuf};

use symposium::output::Output;
use symposium::status_command::StatusState;
use symposium_testlib::{HookStep, TestContext, TestMode, with_fixture};

/// Everywhere a skill named `<skill_name>` (or `<skill_name>-<hash>`) was
/// delivered for this workspace.
///
/// These tests are about enablement, not about which mechanism carries a skill,
/// so both are searched: a compiled plugin directory, which is what Claude Code
/// now receives, and a standalone skill directory, which is what an agent with no
/// plugin unit still gets.
fn find_delivered_skills(workspace_root: &Path, skill_name: &str) -> Vec<PathBuf> {
    let mut parents = vec![
        workspace_root.join(".claude").join("skills"),
        workspace_root.join(".agents").join("skills"),
    ];
    if let Ok(compiled) = std::fs::read_dir(workspace_root.join(".symposium").join("plugins")) {
        parents.extend(compiled.flatten().map(|e| e.path().join("skills")));
    }

    let mut out = Vec::new();
    for parent in parents {
        let Ok(entries) = std::fs::read_dir(&parent) else {
            continue;
        };
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
    }
    out.sort();
    out
}

/// The unique delivered skill directory with this name. Panics on 0 or >1.
fn find_delivered_skill(workspace_root: &Path, skill_name: &str) -> PathBuf {
    let mut hits = find_delivered_skills(workspace_root, skill_name);
    assert_eq!(
        hits.len(),
        1,
        "expected exactly one delivered skill named `{skill_name}` under {}, found {hits:?}",
        workspace_root.display(),
    );
    hits.pop().unwrap()
}

fn read_config(ctx: &TestContext) -> String {
    std::fs::read_to_string(ctx.sym.config_dir().join("config.toml")).unwrap()
}

/// Run `f` with a JSON report layer installed and return the drained events.
async fn with_report<F>(f: F) -> Vec<serde_json::Value>
where
    F: AsyncFnOnce(),
{
    use symposium::report::{ReportLayer, ReportMode};
    use tracing_subscriber::layer::SubscriberExt;

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
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            let root = ctx.workspace_root.clone().unwrap();

            // Unconsented, the dependency's skills stay out.
            ctx.symposium(&["sync"]).await?;
            assert!(find_delivered_skills(&root, "a-guidance").is_empty());

            ctx.symposium(&["use", "crate-a"]).await?;
            find_delivered_skill(&root, "a-guidance");

            let config = read_config(&ctx);
            assert!(config.contains("crate-a"), "entry recorded: {config}");
            assert!(
                config.contains("workspace"),
                "entry is workspace-scoped: {config}"
            );

            ctx.symposium(&["use", "crate-a"]).await?;
            assert_eq!(config, read_config(&ctx), "re-using must not duplicate");
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
                err.to_string().contains("no crate or plugin named"),
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

/// `use` wakes a dormant registry plugin (one no dependency gates), and
/// `--remove` puts it back to sleep, reaping its skills.
#[tokio::test]
async fn use_wakes_and_remove_sleeps_a_dormant_plugin() {
    with_fixture(
        TestMode::SimulationOnly,
        &["dormant-plugin0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            let root = ctx.workspace_root.clone().unwrap();

            ctx.symposium(&["sync"]).await?;
            assert!(find_delivered_skills(&root, "gateless-guidance").is_empty());

            ctx.symposium(&["use", "gateless-plugin"]).await?;
            find_delivered_skill(&root, "gateless-guidance");
            assert!(read_config(&ctx).contains("gateless-plugin"));

            ctx.symposium(&["use", "--remove", "gateless-plugin"])
                .await?;
            assert!(find_delivered_skills(&root, "gateless-guidance").is_empty());
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
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            let root = ctx.workspace_root.clone().unwrap();

            ctx.symposium(&["use", "crate-a"]).await?;
            find_delivered_skill(&root, "a-guidance");

            ctx.symposium(&["use", "--remove", "crate-a"]).await?;
            assert!(
                !read_config(&ctx).contains("crate-a"),
                "entry removed: {}",
                read_config(&ctx)
            );
            assert!(
                find_delivered_skills(&root, "a-guidance").is_empty(),
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
            kinds.contains(&"info") && kinds.contains(&"search_match"),
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

            let deps = ctx.sym.workspace_deps(&workspace_root);
            let entries = symposium::status_command::workspace_status(&ctx.sym, &deps).await?;
            let candidate = entries
                .iter()
                .find(|e| e.name == "crate-a")
                .expect("crate-a discovered");
            assert_eq!(candidate.state, StatusState::Candidate);
            assert!(candidate.root.contains("awaiting consent"), "{candidate:?}");

            ctx.symposium(&["use", "crate-a"]).await?;
            symposium::discovery::apply_consent(&mut ctx.sym, &[], &["noisy-crate".to_string()])?;

            let deps = ctx.sym.workspace_deps(&workspace_root);
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

            let deps = ctx.sym.workspace_deps(&workspace_root);
            let entries = symposium::status_command::workspace_status(&ctx.sym, &deps).await?;
            let dormant = entries
                .iter()
                .find(|e| e.name == "gateless-plugin")
                .expect("gateless-plugin present");
            assert_eq!(dormant.state, StatusState::Dormant);
            assert!(dormant.root.contains("awaiting `cargo agents use`"));

            // And `search` finds it, tagged with the registry it came from
            // and flagged as needing `use`.
            let hit = symposium::search_command::find_matches(&ctx.sym, "gateless")
                .await
                .into_iter()
                .find(|m| m.name == "gateless-plugin")
                .expect("search finds the manifest plugin");
            assert_eq!(hit.origin, "user-plugins");
            assert!(
                hit.dormant,
                "dormancy is its own flag, so a description stays the plugin's own: {hit:?}"
            );

            ctx.symposium(&["use", "gateless-plugin"]).await?;
            let deps = ctx.sym.workspace_deps(&workspace_root);
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
            let deps = ctx.sym.workspace_deps(&workspace_root);
            assert_eq!(
                symposium::discovery::pending_candidates(&ctx.sym, &deps).await,
                vec!["crate-a".to_string()]
            );

            // The prompt returns without reading stdin, recording nothing.
            let deps = ctx.sym.workspace_deps(&workspace_root);
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
            let deps = ctx.sym.workspace_deps(&workspace_root);
            assert_eq!(
                symposium::discovery::pending_candidates(&ctx.sym, &deps).await,
                vec!["crate-a".to_string()]
            );
            assert!(find_delivered_skills(&workspace_root, "a-guidance").is_empty());
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
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            let workspace_root = ctx.workspace_root.clone().unwrap();

            symposium::discovery::apply_consent(&mut ctx.sym, &["crate-a".to_string()], &[])?;
            assert!(read_config(&ctx).contains("auto-enable"));

            ctx.symposium(&["sync"]).await?;
            find_delivered_skill(&workspace_root, "a-guidance");

            let deps = ctx.sym.workspace_deps(&workspace_root);
            assert!(
                symposium::discovery::pending_candidates(&ctx.sym, &deps)
                    .await
                    .is_empty(),
                "a decided dependency is not offered again"
            );

            symposium::discovery::apply_consent(&mut ctx.sym, &[], &["other-dep".to_string()])?;
            assert!(read_config(&ctx).contains("disable"));
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

            assert!(context.contains("crate-a"), "{context}");
            assert!(context.contains("cargo agents use"), "{context}");
            assert!(context.contains("Do not enable them yourself"), "{context}");
            Ok(())
        },
    )
    .await
    .unwrap();
}

// ── Command coverage for agent plugin packages ───────────────────────

/// `plugin validate` names the manifest that defined each entry, and contains a
/// failure at the level where it takes effect: a rejected package, or a skipped
/// skill inside a package that otherwise loads.
#[test]
fn validate_reports_agent_plugin_packages_per_level() {
    let dir = Path::new("tests/fixtures/agent-plugin-broken0");
    let results = symposium::plugins::validate_source_dir(dir).expect("validate");

    let rejected = results
        .iter()
        .find(|r| r.result.is_err())
        .expect("the package with an unusable name is rejected");
    assert!(
        format!("{:#}", rejected.result.as_ref().unwrap_err()).contains("not 1 to 64 characters"),
        "{:#}",
        rejected.result.as_ref().unwrap_err()
    );

    let partly = results
        .iter()
        .find(|r| r.id == "partly-broken")
        .expect("the other package still loads");
    assert!(partly.result.is_ok(), "a sibling's failure is contained");
    assert_eq!(
        partly.kind.to_string(),
        "agent plugin",
        "the output says which manifest defined it"
    );
    assert_eq!(
        partly.children.iter().filter(|c| c.result.is_ok()).count(),
        1,
        "the good skill loads"
    );
    assert_eq!(
        partly.children.iter().filter(|c| c.result.is_err()).count(),
        1,
        "and the broken one is reported as a skill, not as the package"
    );
}

/// `search` and `status` describe a package in the vocabulary they already use,
/// annotated with the manifest it came from.
#[tokio::test]
async fn search_and_status_describe_agent_plugin_packages() {
    with_fixture(
        TestMode::SimulationOnly,
        &["agent-plugin-read0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;

            let hit = symposium::search_command::find_matches(&ctx.sym, "portable-tools")
                .await
                .into_iter()
                .find(|m| m.name == "portable-tools")
                .expect("search finds the package");
            assert_eq!(hit.kind.as_deref(), Some("agent plugin"));
            assert_eq!(
                hit.version.as_deref(),
                Some("2.1.0"),
                "the version comes from the package's own manifest"
            );
            assert_eq!(
                hit.description.as_deref(),
                Some("An externally authored package")
            );
            assert!(!hit.dormant, "it declares a `dev.symposium` gate");

            let dormant = symposium::search_command::find_matches(&ctx.sym, "dormant-portable")
                .await
                .into_iter()
                .find(|m| m.name == "dormant-portable")
                .expect("search finds the gateless package");
            assert!(dormant.dormant, "no gate, so it waits to be used");
            assert_eq!(
                dormant.description.as_deref(),
                Some("No gate, so it waits to be used"),
                "dormancy is reported separately, so the description stays the author's"
            );

            let workspace_root = ctx.workspace_root.clone().unwrap();
            let deps = ctx.sym.workspace_deps(&workspace_root);
            let entries = symposium::status_command::workspace_status(&ctx.sym, &deps).await?;
            let entry = entries
                .iter()
                .find(|e| e.name == "portable-tools")
                .expect("status lists the package");
            assert_eq!(entry.kind.as_deref(), Some("agent plugin"));
            assert_eq!(entry.state, StatusState::Active);
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// `use` wakes a dormant package and installs it the same way any other plugin
/// is installed; `use --remove` takes it back out.
#[tokio::test]
async fn use_and_remove_a_dormant_agent_plugin_package() {
    with_fixture(
        TestMode::SimulationOnly,
        &["agent-plugin-read0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let root = ctx.workspace_root.clone().unwrap();
            let compiled = root.join(".symposium/plugins/dormant-portable");
            assert!(!compiled.exists(), "dormant, so nothing is installed");

            ctx.symposium(&["use", "dormant-portable"]).await?;
            assert!(
                compiled.join("plugin.json").is_file(),
                "`use` wakes it and the same install path runs"
            );
            assert!(compiled.join("skills/dormant-guidance/SKILL.md").is_file());

            ctx.symposium(&["use", "--remove", "dormant-portable"])
                .await?;
            assert!(
                !compiled.exists(),
                "`remove` disables it and the next sync reaps the directory"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}
