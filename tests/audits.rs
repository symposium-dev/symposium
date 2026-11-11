//! Audits: `plugin digest`, the builtin list, and how revocations are
//! enforced at activation.

use std::path::{Path, PathBuf};

use symposium::audits::digest::payload_digest;
use symposium::audits::{Answer, AuditIndex, Verdict};
use symposium::config::{RegistryConfig, Symposium, UseEntry};
use symposium::discovery::{DiscoveredPlugin, Discovery};
use symposium::status_command::{StatusEntry, StatusState};
use symposium_testlib::{HookStep, TestContext, TestMode, with_fixture};

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

/// `crate-d`'s source directory in the `audits0` fixture.
fn crate_d(ctx: &TestContext) -> PathBuf {
    ctx.workspace_root.clone().unwrap().join("deps/crate-d")
}

/// Turn the builtin audits list on and write `audits/cargo/<name>.toml` into
/// its cache directory, as if the registry had been fetched with it. Returns
/// the file.
fn write_audits(sym: &mut Symposium, name: &str, text: &str) -> PathBuf {
    sym.config.defaults.symposium_audits = true;
    let dir = sym.audits_dir().unwrap().join("audits/cargo");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(format!("{name}.toml"));
    std::fs::write(&file, text).unwrap();
    file
}

/// Where Claude Code's skills land: the workspace's directory, and the
/// user-level one a plugin enabled for every workspace is delivered to.
fn skill_dirs(ctx: &TestContext) -> Vec<PathBuf> {
    vec![
        ctx.workspace_root.clone().unwrap().join(".claude/skills"),
        ctx.sym.home_dir().join(".claude/skills"),
    ]
}

/// Every installed skill directory named `<name>` or `<name>-<hash>` under
/// `parents`, on its own or inside a plugin directory delivered there.
fn installed(parents: &[PathBuf], name: &str) -> Vec<PathBuf> {
    let children = |dir: &Path| -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .map(|entries| entries.flatten().map(|e| e.path()).collect())
            .unwrap_or_default()
    };
    parents
        .iter()
        .flat_map(|parent| children(parent))
        .flat_map(|dir| {
            let in_plugin = children(&dir.join("skills"));
            std::iter::once(dir).chain(in_plugin)
        })
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n == name || n.starts_with(&format!("{name}-")))
                && p.join("SKILL.md").is_file()
        })
        .collect()
}

async fn discover(ctx: &TestContext) -> Discovery {
    let ws = ctx.sym.workspace(ctx.workspace_root.as_ref().unwrap());
    symposium::discovery::discover_for(&ctx.sym, &ws).await
}

fn names(found: &[DiscoveredPlugin]) -> Vec<&str> {
    found.iter().map(|d| d.name()).collect()
}

/// crate-d's line of `cargo agents status`.
async fn status(ctx: &TestContext) -> StatusEntry {
    let ws = ctx.sym.workspace(ctx.workspace_root.as_ref().unwrap());
    let entries = symposium::status_command::workspace_status(&ctx.sym, &ws)
        .await
        .unwrap();
    entries
        .into_iter()
        .find(|e| e.name == "crate-d")
        .expect("a status entry for crate-d")
}

/// The `plugin_status` line for `crate-d` from `cargo agents status --json`.
async fn status_line(ctx: &mut TestContext) -> serde_json::Value {
    let events = with_report(async || {
        ctx.symposium(&["status"]).await.unwrap();
    })
    .await;
    events
        .iter()
        .find(|e| e["kind"] == "plugin_status" && e["name"] == "crate-d")
        .unwrap_or_else(|| panic!("{events:?}"))
        .clone()
}

/// The standing grants, each one enough on its own to load an unrevoked
/// crate-d, and no grant at all, which leaves it a candidate.
type Grant = (&'static str, fn(&mut Symposium));

fn grants() -> Vec<Grant> {
    vec![
        ("auto-enable", |sym| {
            sym.config.plugins.auto_enable = vec!["crate-d".into()];
        }),
        ("use", |sym| {
            sym.config.plugins.used = vec![UseEntry::Global("crate-d".into())];
        }),
        ("none", |_| {}),
    ]
}

/// Run `audits` (the text of crate-d's audits file, given its payload digest)
/// under each grant: the crate must not load, must never be offered for
/// consent, and sync must say why; without the file the same grant loads it
/// (or, with no grant, offers it).
async fn assert_revoked_under_every_grant(audits: impl Fn(&str) -> String) {
    for (grant_name, grant) in grants() {
        let granted = grant_name != "none";
        with_fixture(TestMode::SimulationOnly, &["audits0"], async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            let skills = skill_dirs(&ctx);
            grant(&mut ctx.sym);
            let digest = payload_digest(&crate_d(&ctx)).unwrap().to_string();

            let file = write_audits(&mut ctx.sym, "crate-d", &audits(&digest));
            let events = ctx.sync_with_report(tracing::Level::DEBUG).await?;
            assert!(
                installed(&skills, "d-guidance").is_empty(),
                "{grant_name}: a revoked crate must not load"
            );
            let found = discover(&ctx).await;
            assert_eq!(names(&found.revoked), ["crate-d"], "{grant_name}");
            assert!(found.candidates.is_empty(), "{grant_name}");
            let entry = status(&ctx).await;
            assert_eq!(entry.state, StatusState::Revoked, "{grant_name}: {entry:?}");
            assert_eq!(
                entry.root,
                "revoked by the `symposium-audits` list: it tells the agent to fetch and run remote code",
            );
            if granted {
                let reason = events
                    .iter()
                    .find(|e| e["kind"] == "plugin_considered" && e["plugin"] == "crate-d")
                    .and_then(|e| e["reason"].as_str())
                    .unwrap_or_else(|| panic!("{grant_name}: no reason in {events:?}"));
                assert_eq!(
                    reason,
                    "revoked by the `symposium-audits` list: it tells the agent to fetch and run remote code",
                );
            }

            // The grant itself is fine: lifting the revocation loads the
            // crate, or offers it for consent.
            std::fs::remove_file(file)?;
            ctx.symposium(&["sync"]).await?;
            assert_eq!(
                installed(&skills, "d-guidance").len(),
                usize::from(granted),
                "{grant_name}"
            );
            let found = discover(&ctx).await;
            assert!(found.revoked.is_empty(), "{grant_name}");
            assert_eq!(found.candidates.len(), usize::from(!granted), "{grant_name}");
            let expected = if granted {
                StatusState::Active
            } else {
                StatusState::Candidate
            };
            assert_eq!(status(&ctx).await.state, expected, "{grant_name}");
            Ok(())
        })
        .await
        .unwrap();
    }
}

/// A revoked version range beats `use` and `auto-enable`.
#[tokio::test]
async fn a_crate_revoked_by_version_does_not_load() {
    assert_revoked_under_every_grant(|_| {
        "format = 1\n[[revoked]]\nversions = \">=1.0.0, <1.3.0\"\nreason = \"remote-exec\"\n"
            .to_string()
    })
    .await;
}

/// So does a revoked payload digest, which wins over an audit of the same
/// digest.
#[tokio::test]
async fn a_crate_revoked_by_digest_does_not_load() {
    assert_revoked_under_every_grant(|digest| {
        format!(
            "format = 1\n[[audit]]\ndigest = \"{digest}\"\n\
             [[revoked]]\ndigest = \"{digest}\"\nreason = \"remote-exec\"\n"
        )
    })
    .await;
}

/// `disable` is the last word: a revoked and disabled crate is declined, and
/// the revocation does not leak into its audit answer.
#[tokio::test]
async fn disable_outranks_a_revocation() {
    with_fixture(TestMode::SimulationOnly, &["audits0"], async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "claude"]).await?;
        let skills = skill_dirs(&ctx);
        ctx.sym.config.plugins.auto_enable = vec!["crate-d".into()];
        ctx.sym.config.plugins.disable = vec!["crate_d".into()];
        write_audits(
            &mut ctx.sym,
            "crate-d",
            "format = 1\n[[revoked]]\nversions = \"*\"\nreason = \"off-task\"\n",
        );
        ctx.symposium(&["sync"]).await?;
        assert!(installed(&skills, "d-guidance").is_empty());
        let found = discover(&ctx).await;
        assert_eq!(names(&found.declined), ["crate-d"]);
        assert!(found.revoked.is_empty());
        let entry = status(&ctx).await;
        assert_eq!(entry.state, StatusState::Declined, "{entry:?}");
        assert_eq!(entry.audit, None, "{entry:?}");
        let line = status_line(&mut ctx).await;
        assert_eq!(line["state"], "declined");
        assert!(line.get("audit").is_none(), "{line}");
        Ok(())
    })
    .await
    .unwrap();
}

/// `status` says an active crate's payload is audited; an audit changes
/// nothing else.
#[tokio::test]
async fn status_says_an_active_payload_is_audited() {
    with_fixture(TestMode::SimulationOnly, &["audits0"], async |mut ctx| {
        ctx.sym.config.plugins.auto_enable = vec!["crate-d".into()];
        let entry = status(&ctx).await;
        assert_eq!(entry.audit, Some(Verdict::NotAudited), "{entry:?}");

        let digest = payload_digest(&crate_d(&ctx)).unwrap();
        write_audits(
            &mut ctx.sym,
            "crate-d",
            &format!("format = 1\n[[audit]]\ndigest = \"{digest}\"\nversions = [\"1.2.0\"]\n"),
        );
        let entry = status(&ctx).await;
        assert_eq!(entry.state, StatusState::Active, "{entry:?}");
        assert_eq!(entry.audit, Some(Verdict::Audited));
        let line = status_line(&mut ctx).await;
        assert_eq!(line["audit"], "audited by the `symposium-audits` list");
        Ok(())
    })
    .await
    .unwrap();
}

/// A candidate awaiting consent shows its audit answer in `status`, in the
/// consent prompt's words (the human form renders the same `audit` string);
/// an audit leaves it a candidate.
#[tokio::test]
async fn a_candidate_awaiting_consent_shows_its_audit_answer() {
    with_fixture(TestMode::SimulationOnly, &["audits0"], async |mut ctx| {
        let line = status_line(&mut ctx).await;
        assert_eq!(line["state"], "candidate");
        assert_eq!(line["audit"], "not audited");

        let digest = payload_digest(&crate_d(&ctx)).unwrap();
        write_audits(
            &mut ctx.sym,
            "crate-d",
            &format!("format = 1\n[[audit]]\ndigest = \"{digest}\"\n"),
        );
        let line = status_line(&mut ctx).await;
        assert_eq!(line["state"], "candidate");
        assert_eq!(line["audit"], "audited by the `symposium-audits` list");

        // A payload that cannot be digested is not audited, and says why.
        std::fs::write(
            crate_d(&ctx).join("Symposium.toml"),
            "[[skills]]\nsource.git = \"https://github.com/example/skills\"\n",
        )?;
        let line = status_line(&mut ctx).await;
        assert_eq!(line["state"], "candidate");
        assert_eq!(
            line["audit"],
            "not audited: its payload cannot be checked against an audit because it fetches skills with `source.git`"
        );
        Ok(())
    })
    .await
    .unwrap();
}

/// The `SessionStart` hint names each pending candidate with its audit
/// answer, and an audited one is still pending: an audit is not consent.
#[tokio::test]
async fn the_session_start_hint_says_whether_a_candidate_is_audited() {
    with_fixture(TestMode::SimulationOnly, &["audits0"], async |mut ctx| {
        ctx.sym.config.auto_update = symposium::config::AutoUpdate::Off;
        ctx.symposium(&["init", "--add-agent", "claude"]).await?;
        let digest = payload_digest(&crate_d(&ctx)).unwrap();
        write_audits(
            &mut ctx.sym,
            "crate-d",
            &format!("format = 1\n[[audit]]\ndigest = \"{digest}\"\n"),
        );

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
            .find_map(|h| {
                h.output
                    .get("hookSpecificOutput")
                    .and_then(|o| o.get("additionalContext"))
                    .and_then(|v| v.as_str())
            })
            .expect("session-start should produce additionalContext");
        assert!(
            context.contains(
                "not enabled yet: crate-d (audited by the `symposium-audits` list). \
                 They stay off until the user consents, audited or not"
            ),
            "{context}"
        );
        assert!(context.contains("Do not enable them yourself"), "{context}");
        assert!(ctx.sym.config.plugins.auto_enable.is_empty());
        Ok(())
    })
    .await
    .unwrap();
}

/// `use` refuses a revoked crate, saying why, and writes nothing.
#[tokio::test]
async fn use_refuses_a_revoked_crate() {
    with_fixture(TestMode::SimulationOnly, &["audits0"], async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "claude"]).await?;
        write_audits(
            &mut ctx.sym,
            "crate-d",
            "format = 1\n[[revoked]]\nversions = \"*\"\nreason = \"remote-exec\"\n",
        );
        let config = ctx.sym.config_dir().join("config.toml");
        let before = std::fs::read_to_string(&config)?;
        for args in [&["use", "crate-d"][..], &["use", "crate-d", "--global"]] {
            let err = ctx.symposium(args).await.unwrap_err();
            assert!(
                format!("{err:#}").contains("tells the agent to fetch and run remote code"),
                "{err:#}"
            );
        }
        assert!(ctx.sym.config.plugins.used.is_empty());
        assert_eq!(std::fs::read_to_string(&config)?, before);
        Ok(())
    })
    .await
    .unwrap();
}

/// `chained-crate0`: a registry plugin vouches for `crate-w` through a
/// `[[plugins]]` chained reference, so `crate-w` loads with no `[plugins]`
/// entry of its own.
async fn chained_skill_installs(ctx: &mut TestContext) -> usize {
    ctx.symposium(&["sync"]).await.unwrap();
    installed(&skill_dirs(ctx), "w-guidance").len()
}

/// A revoked crate is not loaded even through a chained reference: the
/// registry vouches for it, but the list has the last word.
#[tokio::test]
async fn a_revoked_chained_crate_is_not_loaded() {
    with_fixture(
        TestMode::SimulationOnly,
        &["chained-crate0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            assert_eq!(chained_skill_installs(&mut ctx).await, 1, "baseline");
            write_audits(
                &mut ctx.sym,
                "crate-w",
                "format = 1\n[[revoked]]\nversions = \"<1.0.0\"\nreason = \"exfiltration\"\n",
            );
            assert_eq!(chained_skill_installs(&mut ctx).await, 0);
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// A revocation covers cargo-sourced plugins only: an audits file naming a
/// registry plugin does not switch it off.
#[tokio::test]
async fn revocations_do_not_apply_to_registry_plugins() {
    with_fixture(
        TestMode::SimulationOnly,
        &["chained-crate0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            let revoke = "format = 1\n[[revoked]]\nversions = \"*\"\nreason = \"off-task\"\n";
            let file = write_audits(&mut ctx.sym, "vouch-plugin", revoke);
            let user_plugins = file.parent().unwrap().with_file_name("user-plugins");
            std::fs::create_dir_all(&user_plugins)?;
            std::fs::write(user_plugins.join("vouch-plugin.toml"), revoke)?;
            assert_eq!(chained_skill_installs(&mut ctx).await, 1);
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// The hook path does not load a revoked crate, and its answer to the agent
/// says nothing about it, even with a malformed entry in the file to warn
/// about: warnings go to the log.
#[tokio::test]
async fn a_revoked_crate_leaves_hook_stdout_alone() {
    with_fixture(TestMode::SimulationOnly, &["audits0"], async |mut ctx| {
        ctx.sym.config.auto_update = symposium::config::AutoUpdate::Off;
        ctx.symposium(&["init", "--add-agent", "claude"]).await?;
        let skills = skill_dirs(&ctx);
        ctx.sym.config.plugins.auto_enable = vec!["crate-d".into()];
        let file = write_audits(
            &mut ctx.sym,
            "crate-d",
            "format = 1\n[[revoked]]\nversions = \"*\"\nreason = \"off-task\"\n\
             [[revoked]]\nversions = \"broken\"\n",
        );

        let session_start = async |ctx: &mut TestContext| {
            let result = ctx
                .prompt_or_hook(
                    "hello",
                    &[HookStep::session_start()],
                    symposium::hook_schema::HookAgent::Claude,
                )
                .await
                .unwrap();
            // The output parsed as JSON to get here.
            result.hooks[0].output.to_string()
        };

        let output = session_start(&mut ctx).await;
        assert!(!output.contains("revoked"), "{output}");
        assert!(!output.contains("crate-d"), "{output}");
        assert!(installed(&skills, "d-guidance").is_empty());

        // The hook does sync, so without the revocation the same setup
        // installs the skill: the assertion above is not vacuous.
        std::fs::remove_file(file)?;
        session_start(&mut ctx).await;
        assert_eq!(installed(&skills, "d-guidance").len(), 1);
        Ok(())
    })
    .await
    .unwrap();
}

/// A revocation that lands after a crate's skills were installed removes them,
/// even when the sync otherwise skips cleanup because a registry could not be
/// read: the safety net keeps what a silent source installed, never what the
/// list revoked.
#[tokio::test]
async fn a_revocation_removes_installed_skills_even_when_a_registry_is_unreadable() {
    for break_registry in [false, true] {
        with_fixture(TestMode::SimulationOnly, &["audits0"], async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            let skills = skill_dirs(&ctx);
            ctx.sym.config.plugins.used = vec![UseEntry::Global("crate-d".into())];
            ctx.symposium(&["sync"]).await?;
            assert_eq!(installed(&skills, "d-guidance").len(), 1);

            write_audits(
                &mut ctx.sym,
                "crate-d",
                "format = 1\n[[revoked]]\nversions = \"*\"\nreason = \"remote-exec\"\n",
            );
            if break_registry {
                add_unreadable_registry(&mut ctx);
            }
            // A managed skill nothing installs this run, as if its registry
            // were the unreadable one: only a full cleanup may remove it.
            let held_back = skills[0].join("held-back");
            std::fs::create_dir_all(&held_back)?;
            std::fs::write(held_back.join(".symposium"), "")?;
            std::fs::write(held_back.join("SKILL.md"), "---\nname: held-back\n---\n")?;
            ctx.symposium(&["sync"]).await?;
            assert!(
                installed(&skills, "d-guidance").is_empty(),
                "broken registry: {break_registry}"
            );
            assert_eq!(
                held_back.exists(),
                break_registry,
                "a revocation removes only its own skills while cleanup is skipped"
            );
            Ok(())
        })
        .await
        .unwrap();
    }
}

/// Configure a path registry that cannot be listed (its root is itself an
/// entry), so every sync skips the cleanup of installed skills.
fn add_unreadable_registry(ctx: &mut TestContext) {
    let dir = ctx.sym.config_dir().join("broken-registry");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Symposium.toml"), "").unwrap();
    ctx.sym.config.registries.push(RegistryConfig {
        name: "broken".into(),
        git: None,
        path: Some(dir.display().to_string()),
        auto_update: true,
    });
}

/// Retiring a revoked crate's skills runs nothing from it: its group and
/// skill predicates are never evaluated (a `shell` predicate would run its
/// command), on sync and on the hook path, and its skill still comes off,
/// with or without a full cleanup.
#[tokio::test]
async fn retiring_a_revoked_crate_runs_none_of_its_predicates() {
    for break_registry in [false, true] {
        with_fixture(TestMode::SimulationOnly, &["audits0"], async |mut ctx| {
            ctx.sym.config.auto_update = symposium::config::AutoUpdate::Off;
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            let skills = skill_dirs(&ctx);
            let krate = crate_d(&ctx);
            let group_ran = krate.with_file_name("group-predicate-ran");
            let skill_ran = krate.with_file_name("skill-predicate-ran");
            // TOML and `sh` both read a Windows path's `\` as an escape.
            let in_shell = |path: &Path| path.display().to_string().replace('\\', "/");
            std::fs::write(
                krate.join("Symposium.toml"),
                format!(
                    "[defaults]\nskills = false\n\n[[skills]]\nsource.path = \"skills\"\n\
                     predicates = [\"shell(touch {})\"]\n",
                    in_shell(&group_ran)
                ),
            )?;
            let skill_md = krate.join("skills/d-guidance/SKILL.md");
            let text = std::fs::read_to_string(&skill_md)?;
            std::fs::write(
                &skill_md,
                text.replacen(
                    "---\n",
                    &format!("---\npredicates: shell(touch {})\n", in_shell(&skill_ran)),
                    1,
                ),
            )?;
            ctx.sym.config.plugins.used = vec![UseEntry::Global("crate-d".into())];
            ctx.symposium(&["sync"]).await?;
            assert_eq!(installed(&skills, "d-guidance").len(), 1);
            // The predicates do run while the crate is active: the markers
            // below are not vacuous.
            assert!(group_ran.exists() && skill_ran.exists());
            std::fs::remove_file(&group_ran)?;
            std::fs::remove_file(&skill_ran)?;

            write_audits(
                &mut ctx.sym,
                "crate-d",
                "format = 1\n[[revoked]]\nversions = \"*\"\nreason = \"remote-exec\"\n",
            );
            if break_registry {
                add_unreadable_registry(&mut ctx);
            }
            ctx.symposium(&["sync"]).await?;
            assert!(
                installed(&skills, "d-guidance").is_empty(),
                "broken registry: {break_registry}"
            );
            ctx.prompt_or_hook(
                "hello",
                &[HookStep::session_start()],
                symposium::hook_schema::HookAgent::Claude,
            )
            .await?;
            assert!(!group_ran.exists(), "broken registry: {break_registry}");
            assert!(!skill_ran.exists(), "broken registry: {break_registry}");
            Ok(())
        })
        .await
        .unwrap();
    }
}

/// A revoked crate's MCP server comes off at user scope (the default hook
/// scope), from the agent's file and from the ledger, with or without a full
/// cleanup, and an entry the user wrote is left alone.
#[tokio::test]
async fn a_revocation_unregisters_the_crates_mcp_servers_at_user_scope() {
    for break_registry in [false, true] {
        with_fixture(TestMode::SimulationOnly, &["audits0"], async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude", "--hook-scope", "global"])
                .await?;
            std::fs::write(
                crate_d(&ctx).join("Symposium.toml"),
                "[[mcp_servers]]\nname = \"d-server\"\ncommand = \"/usr/bin/true\"\n\
                 args = []\nenv = []\n",
            )?;
            ctx.sym.config.plugins.used = vec![UseEntry::Global("crate-d".into())];
            ctx.symposium(&["sync"]).await?;

            let file = ctx.sym.home_dir().join(".claude.json");
            let read = |file: &Path| -> serde_json::Value {
                serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap()
            };
            let ledger = |sym: &Symposium| {
                symposium::state::mcp_registrations(sym.config_dir())
                    .into_iter()
                    .filter(|r| r.name == "d-server")
                    .count()
            };
            let mut config = read(&file);
            assert!(config["mcpServers"]["d-server"].is_object(), "{config}");
            assert_eq!(ledger(&ctx.sym), 1);
            config["mcpServers"]["mine"] = serde_json::json!({ "command": "/home/me/mine" });
            std::fs::write(&file, config.to_string())?;

            write_audits(
                &mut ctx.sym,
                "crate-d",
                "format = 1\n[[revoked]]\nversions = \"*\"\nreason = \"remote-exec\"\n",
            );
            if break_registry {
                add_unreadable_registry(&mut ctx);
            }
            ctx.symposium(&["sync"]).await?;
            let config = read(&file);
            assert!(
                config["mcpServers"].get("d-server").is_none(),
                "broken registry: {break_registry}: {config}"
            );
            assert!(config["mcpServers"]["mine"].is_object(), "{config}");
            assert_eq!(ledger(&ctx.sym), 0, "broken registry: {break_registry}");
            Ok(())
        })
        .await
        .unwrap();
    }
}

/// The builtin `symposium-audits` list, never fetched (offline, or the
/// repository unreachable), is no list: every lookup is "not audited", and it
/// is neither a registry nor an unreadable source, so loading reports
/// nothing and a sync still cleans up.
#[tokio::test]
async fn an_unfetched_list_answers_not_audited_and_does_not_hold_back_cleanup() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("config.toml"),
        "[defaults]\nuser-plugins = false\nsymposium-recommendations = false\n\
         symposium-audits = true\n",
    )
    .unwrap();
    let sym = Symposium::from_dir(tmp.path());

    let mut registry = None;
    let events = with_report(async || {
        registry = Some(symposium::plugins::load_registry(&sym).await);
    })
    .await;
    let registry = registry.unwrap();
    assert!(registry.plugins.is_empty());
    assert!(registry.unreadable_registries.is_empty());
    assert!(!events.iter().any(|e| e["kind"] == "warning"), "{events:?}");

    let digest = "sha256:".to_string() + &"a".repeat(64);
    assert_eq!(
        AuditIndex::configured(&sym).lookup(
            "cargo",
            "serde",
            "1.0.0",
            Some(&digest.parse().unwrap())
        ),
        Answer::Verdict(Verdict::NotAudited)
    );

    // `audits0` turns the list on and never fetches it: a sync still removes
    // a managed skill nothing installs.
    with_fixture(TestMode::SimulationOnly, &["audits0"], async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "claude"]).await?;
        assert!(!ctx.sym.audits_dir().unwrap().exists());
        let stale = ctx
            .workspace_root
            .clone()
            .unwrap()
            .join(".claude/skills/stale");
        std::fs::create_dir_all(&stale)?;
        std::fs::write(stale.join(".symposium"), "")?;
        std::fs::write(stale.join("SKILL.md"), "---\nname: stale\n---\n")?;
        ctx.symposium(&["sync"]).await?;
        assert!(!stale.exists());
        Ok(())
    })
    .await
    .unwrap();
}

/// Content in the list is never loaded as a plugin: a manifest or a skill in
/// the repository is not an entry of any registry, does not make anything
/// unreadable, and installs nothing. Its audits still count.
#[tokio::test]
async fn content_in_the_list_is_never_loaded_as_a_plugin() {
    with_fixture(TestMode::SimulationOnly, &["audits0"], async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "claude"]).await?;
        let skills = skill_dirs(&ctx);
        ctx.sym.config.plugins.used = vec![UseEntry::Global("crate-d".into())];
        write_audits(
            &mut ctx.sym,
            "crate-d",
            "format = 1\n[[revoked]]\nversions = \"*\"\nreason = \"remote-exec\"\n",
        );
        let list = ctx.sym.audits_dir().unwrap();
        std::fs::write(
            list.join("Symposium.toml"),
            "name = \"stray\"\ndepends-on = [\"*\"]\n",
        )?;
        let stray = list.join("skills/stray-skill");
        std::fs::create_dir_all(&stray)?;
        std::fs::write(
            stray.join("SKILL.md"),
            "---\nname: stray-skill\ndescription: stray\ndepends-on: \"*\"\n---\n",
        )?;

        let registry = symposium::plugins::load_registry(&ctx.sym).await;
        assert!(registry.plugins.is_empty(), "{:?}", registry.plugins);
        assert!(registry.unreadable_registries.is_empty());
        ctx.symposium(&["sync"]).await?;
        assert!(installed(&skills, "stray-skill").is_empty());
        assert!(installed(&skills, "d-guidance").is_empty());
        Ok(())
    })
    .await
    .unwrap();
}

/// `plugin digest` prints the library's digest for a directory, a crate name
/// or `name@version`, says which crate a name resolved to, and fails, saying
/// why, for a payload that has no digest.
#[tokio::test]
async fn plugin_digest_prints_the_digest_or_why_there_is_none() {
    with_fixture(TestMode::SimulationOnly, &["audits0"], async |mut ctx| {
        let expected = payload_digest(&crate_d(&ctx)).unwrap().to_string();
        let digest_event = async |ctx: &mut TestContext, target: &str| {
            let mut result = None;
            let events = with_report(async || {
                result = Some(ctx.symposium(&["plugin", "digest", target]).await);
            })
            .await;
            let event = events.into_iter().find(|e| e["kind"] == "payload_digest");
            (result.unwrap(), event)
        };

        // A crate name, `name@version` (here the release the workspace
        // resolved, served from its own copy), and a directory relative to
        // the working directory.
        for (target, krate) in [
            ("crate-d", Some("crate-d 1.2.0")),
            ("crate-d@1.2.0", Some("crate-d 1.2.0")),
            ("./deps/crate-d", None),
        ] {
            let (result, event) = digest_event(&mut ctx, target).await;
            result?;
            let event = event.expect("a payload_digest event");
            assert_eq!(event["digest"], expected.as_str(), "{target}");
            assert_eq!(event["krate"].as_str(), krate, "{target}");
        }

        let (result, _) = digest_event(&mut ctx, "crate-d@latest").await;
        let err = result.unwrap_err();
        assert!(format!("{err:#}").contains("crate-d@latest"), "{err:#}");

        // A path that is not a directory is never taken for a crate name.
        for target in ["./deps/no-such-crate", "deps/crate-d/Cargo.toml"] {
            let (result, _) = digest_event(&mut ctx, target).await;
            let err = result.unwrap_err();
            assert!(
                format!("{err:#}").contains("is not a directory"),
                "{target}: {err:#}"
            );
        }

        // A payload that cannot be pinned reports why and fails the command.
        std::fs::write(
            crate_d(&ctx).join("Symposium.toml"),
            "[[skills]]\nsource.git = \"https://github.com/example/skills\"\n",
        )?;
        let (result, event) = digest_event(&mut ctx, "./deps/crate-d").await;
        assert!(result.is_err(), "ineligible is a failure");
        let event = event.expect("a payload_digest event");
        assert!(event.get("digest").is_none());
        assert!(
            event["ineligible"].as_str().unwrap().contains("source.git"),
            "{event}"
        );
        Ok(())
    })
    .await
    .unwrap();
}
