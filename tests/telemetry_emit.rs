//! End-to-end telemetry emission: drives the real hook pipeline and asserts the
//! events land on disk, gated by the opt-in `[telemetry] enabled` flag.
//!
//! Only the sync-snapshot test overlays `workspace0`: `sync` needs a real Cargo
//! workspace (it errors "not in a Rust workspace" otherwise), and resolving its
//! deps costs a `cargo metadata`, so the other tests skip it.
//!
//! `stop` cannot be driven here: the harness `HookStep` has no `Stop` variant.

use chrono::{DateTime, Utc};
use serde_json::json;
use symposium::hook_schema::HookAgent;
use symposium::telemetry::{self, EventKind, TelemetryEvent};
use symposium_testlib::{HookStep, TestMode, with_fixture};

/// Across one session's wire events, the funnel is recorded once each and a tool
/// is counted exactly once — on `PostToolUse`, since `PreToolUse` is silent.
#[tokio::test(flavor = "multi_thread")]
async fn wire_funnel_records_each_event_once() {
    with_fixture(
        TestMode::SimulationOnly,
        &["telemetry-emit"],
        async |mut ctx| {
            ctx.prompt_or_hook(
                "ignored",
                &[
                    HookStep::SessionStart,
                    HookStep::UserPromptSubmit {
                        prompt: "hello".to_string(),
                    },
                    // `Read` matches no hook, so nothing spawns `sh` (Windows).
                    HookStep::PreToolUse {
                        tool_name: "Read".to_string(),
                        tool_input: json!({"file_path": "/tmp/x"}),
                    },
                    HookStep::PostToolUse {
                        tool_name: "Read".to_string(),
                        tool_input: json!({"file_path": "/tmp/x"}),
                        tool_response: json!({"ok": true}),
                    },
                ],
                HookAgent::Claude,
            )
            .await?;

            let events = telemetry::read_events(ctx.sym.config_dir());
            let kinds: Vec<&str> = events.iter().map(|event| event.kind_name()).collect();

            assert!(kinds.contains(&"session_start"), "kinds = {kinds:?}");
            assert!(kinds.contains(&"user_prompt"), "kinds = {kinds:?}");

            let tools: Vec<&str> = events
                .iter()
                .filter_map(|event| match &event.kind {
                    EventKind::ToolUse { tool } => Some(tool.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(
                tools,
                ["Read"],
                "tool counted once on PostToolUse (PreToolUse silent), kinds = {kinds:?}",
            );
            assert!(
                !kinds.contains(&"hook_invocation"),
                "the Read steps match no hook, kinds = {kinds:?}",
            );
            assert!(
                events
                    .iter()
                    .all(|event| event.session_id.as_deref() == Some("test-session-id")),
                "every event carries the wire session id",
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// The `SessionStart` sync snapshot: `sync_run` plus the plugin / skill
/// activations, for both a wildcard gate (no witness crates) and a crate gate.
#[tokio::test(flavor = "multi_thread")]
async fn session_start_records_sync_snapshot() {
    with_fixture(
        TestMode::SimulationOnly,
        &["telemetry-emit", "workspace0"],
        async |mut ctx| {
            ctx.prompt_or_hook("ignored", &[HookStep::SessionStart], HookAgent::Claude)
                .await?;

            let events = telemetry::read_events(ctx.sym.config_dir());
            let kinds: Vec<&str> = events.iter().map(|event| event.kind_name()).collect();

            let (agent, crate_count) = events
                .iter()
                .find_map(|event| match &event.kind {
                    EventKind::SessionStart { agent, crate_count } => {
                        Some((agent.as_str(), *crate_count))
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("no session_start, kinds = {kinds:?}"));

            assert_eq!(agent, "claude");
            assert!(
                crate_count.is_some_and(|count| count >= 1),
                "crate_count should reflect the fixture workspace, got {crate_count:?}",
            );
            assert!(kinds.contains(&"sync_run"), "kinds = {kinds:?}");

            assert!(
                events.iter().any(|event| matches!(
                    &event.kind,
                    EventKind::PluginActivation { plugin, crates }
                        if plugin == "skills-plugin" && crates.is_empty()
                )),
                "expected wildcard skills-plugin activation, kinds = {kinds:?}",
            );
            assert!(
                events.iter().any(|event| matches!(
                    &event.kind,
                    EventKind::SkillActivation { skill, crates, .. }
                        if skill == "example-skill" && crates.is_empty()
                )),
                "expected wildcard example-skill activation, kinds = {kinds:?}",
            );

            assert!(
                events.iter().any(|event| matches!(
                    &event.kind,
                    EventKind::PluginActivation { plugin, crates }
                        if plugin == "crate-gated-plugin" && crates.iter().any(|crate_name| crate_name == "tokio")
                )),
                "expected crate-gated-plugin activation with tokio witness, kinds = {kinds:?}",
            );
            assert!(
                events.iter().any(|event| matches!(
                    &event.kind,
                    EventKind::SkillActivation { skill, crates, .. }
                        if skill == "crate-gated-skill" && crates.iter().any(|crate_name| crate_name == "tokio")
                )),
                "expected crate-gated-skill activation with tokio witness, kinds = {kinds:?}",
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// A plugin hook that runs is timed and recorded as a `hook_invocation`.
///
/// Unix-only: a script hook runs as `sh <path>`, and MSYS `sh` on Windows cannot
/// execute a Windows absolute path, so nothing spawns. CI is Linux + macOS.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn plugin_hook_records_hook_invocation() {
    with_fixture(
        TestMode::SimulationOnly,
        &["telemetry-emit"],
        async |mut ctx| {
            ctx.prompt_or_hook(
                "ignored",
                &[HookStep::PreToolUse {
                    tool_name: "Bash".to_string(),
                    tool_input: json!({"command": "ls"}),
                }],
                HookAgent::Claude,
            )
            .await?;

            let events = telemetry::read_events(ctx.sym.config_dir());
            assert!(
                events.iter().any(|event| matches!(
                    &event.kind,
                    EventKind::HookInvocation { plugin, .. } if plugin == "hooks-plugin"
                )),
                "expected hooks-plugin invocation, kinds = {:?}",
                events
                    .iter()
                    .map(|event| event.kind_name())
                    .collect::<Vec<_>>(),
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// A hook that blocks the tool (exit 2) is still recorded. The block must not be
/// a telemetry hole, though it still propagates as an error.
///
/// Unix-only for the same reason as `plugin_hook_records_hook_invocation`.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn blocked_hook_is_still_recorded() {
    with_fixture(
        TestMode::SimulationOnly,
        &["telemetry-emit"],
        async |mut ctx| {
            let result = ctx
                .prompt_or_hook(
                    "ignored",
                    &[HookStep::PreToolUse {
                        tool_name: "Grep".to_string(),
                        tool_input: json!({"pattern": "x"}),
                    }],
                    HookAgent::Claude,
                )
                .await;
            assert!(result.is_err(), "exit 2 must propagate as a block");

            let events = telemetry::read_events(ctx.sym.config_dir());
            assert!(
                events.iter().any(|event| matches!(
                    &event.kind,
                    EventKind::HookInvocation { plugin, exit_code, .. }
                        if plugin == "hooks-plugin" && *exit_code == Some(2)
                )),
                "blocked hook should be recorded with exit_code 2, kinds = {:?}",
                events
                    .iter()
                    .map(|event| event.kind_name())
                    .collect::<Vec<_>>(),
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// With telemetry disabled (the default), a `SessionStart` pass that would
/// otherwise emit `session_start` + `sync_run` + activations writes nothing.
#[tokio::test(flavor = "multi_thread")]
async fn disabled_gate_records_nothing() {
    with_fixture(
        TestMode::SimulationOnly,
        &["telemetry-disabled"],
        async |mut ctx| {
            ctx.prompt_or_hook("ignored", &[HookStep::SessionStart], HookAgent::Claude)
                .await?;

            let events = telemetry::read_events(ctx.sym.config_dir());
            assert!(
                events.is_empty(),
                "gate off must write nothing, got: {:?}",
                events
                    .iter()
                    .map(|event| event.kind_name())
                    .collect::<Vec<_>>(),
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// `SessionStart` is the only path that runs `roll_off`, so this guards against
/// retention silently never running.
#[tokio::test(flavor = "multi_thread")]
async fn session_start_rolls_off_stale_files() {
    with_fixture(
        TestMode::SimulationOnly,
        &["telemetry-emit"],
        async |mut ctx| {
            let dir = telemetry::telemetry_dir(ctx.sym.config_dir());
            std::fs::create_dir_all(&dir).unwrap();
            let stale = dir.join("events-2000-01-01.jsonl");
            std::fs::write(&stale, "{}\n").unwrap();

            ctx.prompt_or_hook("ignored", &[HookStep::SessionStart], HookAgent::Claude)
                .await?;

            assert!(
                !stale.exists(),
                "SessionStart should roll off files past the retention window",
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// Retention is disk hygiene, not collection: data recorded before a user opted
/// out must still age out, so `roll_off` runs even with telemetry disabled.
#[tokio::test(flavor = "multi_thread")]
async fn rolls_off_stale_files_even_when_disabled() {
    with_fixture(
        TestMode::SimulationOnly,
        &["telemetry-disabled"],
        async |mut ctx| {
            let dir = telemetry::telemetry_dir(ctx.sym.config_dir());
            std::fs::create_dir_all(&dir).unwrap();
            let stale = dir.join("events-2000-01-01.jsonl");
            std::fs::write(&stale, "{}\n").unwrap();

            ctx.prompt_or_hook("ignored", &[HookStep::SessionStart], HookAgent::Claude)
                .await?;

            assert!(
                !stale.exists(),
                "opting out must not strand old data forever"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// One event of every kind through the real `record` path, locked against a
/// committed golden sample. This is what covers `stop` and `hook_invocation`,
/// which the pipeline tests cannot drive portably.
///
/// Compared as parsed JSON so serde key ordering cannot make it flaky.
#[test]
fn synthetic_events_match_golden_jsonl() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    for event in synthetic_session() {
        telemetry::record(dir, &event);
    }

    // Every synthetic event shares one UTC day, so they land in one file.
    let file = telemetry::telemetry_dir(dir).join("events-2026-07-15.jsonl");
    let got = std::fs::read_to_string(&file).expect("event file written");
    let golden = include_str!("fixtures/telemetry/expected-events.jsonl");

    assert_eq!(
        parse_jsonl(&got),
        parse_jsonl(golden),
        "on-disk JSONL drifted from golden. produced:\n{got}",
    );
}

/// One `TelemetryEvent` of every kind, stamped one second apart on 2026-07-15.
fn synthetic_session() -> Vec<TelemetryEvent> {
    let sid = || Some("test-session-id".to_string());
    let at = |secs: u32| -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(&format!("2026-07-15T10:00:0{secs}Z"))
            .unwrap()
            .with_timezone(&Utc)
    };
    let ev = |secs: u32, kind: EventKind| TelemetryEvent {
        at: at(secs),
        session_id: sid(),
        kind,
    };
    vec![
        ev(
            0,
            EventKind::SessionStart {
                agent: "claude".into(),
                crate_count: Some(2),
            },
        ),
        ev(
            1,
            EventKind::SyncRun {
                installed: 1,
                reaped: 0,
                plugins_matched: 1,
            },
        ),
        ev(
            2,
            EventKind::PluginActivation {
                plugin: "skills-plugin".into(),
                crates: vec!["acme-core".into()],
            },
        ),
        ev(
            3,
            EventKind::SkillActivation {
                skill: "example-skill".into(),
                plugin: Some("skills-plugin".into()),
                crates: vec!["acme-core".into()],
            },
        ),
        ev(4, EventKind::UserPrompt),
        ev(
            5,
            EventKind::ToolUse {
                tool: "Bash".into(),
            },
        ),
        ev(
            6,
            EventKind::HookInvocation {
                hook: "example-hook".into(),
                plugin: "hooks-plugin".into(),
                duration_ms: 4,
                exit_code: Some(0),
            },
        ),
        ev(7, EventKind::Stop),
    ]
}

fn parse_jsonl(s: &str) -> Vec<serde_json::Value> {
    s.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("valid JSON line"))
        .collect()
}
