//! The recorded transcripts, and the edits tests make to them.
//!
//! Each was recorded from Claude Code 2.1.316 and reduced by hand to remove personal and
//! account-specific data, keeping real event shapes, with the session, project and home
//! replaced. Helpers find lines by their event where they can, and every edit must
//! change something, so a test cannot quietly check nothing.

use serde_json::{Map, Value};
use uuid::Uuid;

pub(super) const FINISHED: &str = include_str!("transcripts/finished.jsonl");
/// Run with a model that does not exist.
pub(super) const MODEL_ERROR: &str = include_str!("transcripts/model_error.jsonl");
/// A `Skill` call for the installed fixture skill.
pub(super) const SKILL_LOADED: &str = include_str!("transcripts/skill_loaded.jsonl");
/// A `Read` of the installed `SKILL.md`, by a path relative to the project.
pub(super) const READ_LOADED: &str = include_str!("transcripts/read_loaded.jsonl");
/// A `Skill` call for a skill that does not exist.
pub(super) const SKILL_FAILED: &str = include_str!("transcripts/skill_failed.jsonl");

/// The session every fixture was recorded as.
pub(super) const SESSION: &str = "00000000-0000-4000-8000-000000000000";

pub(super) fn recorded_session() -> Uuid {
    Uuid::parse_str(SESSION).unwrap()
}

/// Whether `line` holds an event of `kind`: a `type`, or `type/subtype`.
fn is(line: &str, kind: &str) -> bool {
    let event: Value = serde_json::from_str(line).unwrap();
    let (wanted_type, wanted_subtype) = match kind.split_once('/') {
        Some((wanted_type, subtype)) => (wanted_type, Some(subtype)),
        None => (kind, None),
    };
    event["type"] == wanted_type && wanted_subtype.is_none_or(|subtype| event["subtype"] == subtype)
}

fn index(transcript: &str, matches: impl Fn(&str) -> bool) -> usize {
    transcript
        .lines()
        .position(matches)
        .expect("the fixture has no such line")
}

fn joined<'a>(lines: impl Iterator<Item = &'a str>) -> String {
    lines.map(|line| format!("{line}\n")).collect()
}

/// The first line of `transcript` holding an event of `kind`.
pub(super) fn first<'a>(transcript: &'a str, kind: &str) -> &'a str {
    transcript
        .lines()
        .nth(index(transcript, |line| is(line, kind)))
        .unwrap()
}

/// The first line of `transcript` containing `marker`.
pub(super) fn line_with<'a>(transcript: &'a str, marker: &str) -> &'a str {
    transcript
        .lines()
        .nth(index(transcript, |line| line.contains(marker)))
        .unwrap()
}

/// `transcript` up to, not including, its first event of `kind`.
pub(super) fn before(transcript: &str, kind: &str) -> String {
    joined(
        transcript
            .lines()
            .take(index(transcript, |line| is(line, kind))),
    )
}

/// `transcript` up to, not including, its first line containing `marker`.
pub(super) fn before_line_with(transcript: &str, marker: &str) -> String {
    joined(
        transcript
            .lines()
            .take(index(transcript, |line| line.contains(marker))),
    )
}

/// `transcript` from its first event of `kind` on.
pub(super) fn from(transcript: &str, kind: &str) -> String {
    joined(
        transcript
            .lines()
            .skip(index(transcript, |line| is(line, kind))),
    )
}

/// `transcript` without its events of `kind`, of which there must be one.
pub(super) fn without(transcript: &str, kind: &str) -> String {
    let kept = joined(transcript.lines().filter(|line| !is(line, kind)));
    assert_ne!(
        kept.lines().count(),
        transcript.lines().count(),
        "no {kind} event"
    );
    kept
}

/// `text` with every `from` replaced by a different `to`; `from` must be there.
pub(super) fn replaced(text: &str, from: &str, to: &str) -> String {
    assert_ne!(from, to, "a replacement must change the fixture");
    assert!(text.contains(from), "{from} is not in the fixture");
    text.replace(from, to)
}

/// `line` as if session `other` had written it.
pub(super) fn as_session(line: &str, other: Uuid) -> String {
    replaced(line, SESSION, &other.to_string())
}

/// The event on `line`, with `edit` applied to it; the edit must change it.
pub(super) fn edited(line: &str, edit: impl FnOnce(&mut Map<String, Value>)) -> String {
    let recorded: Map<String, Value> = serde_json::from_str(line).unwrap();
    let mut event = recorded.clone();
    edit(&mut event);
    assert_ne!(event, recorded, "the edit left the event as it was");
    serde_json::to_string(&event).unwrap()
}

/// `transcript` with `edit` applied to every line `selects`, of which there must be one;
/// the edit must change each.
pub(super) fn editing(
    transcript: &str,
    selects: impl Fn(&str) -> bool,
    edit: impl Fn(&mut Map<String, Value>),
) -> String {
    let mut selected = 0;
    let lines: Vec<String> = transcript
        .lines()
        .map(|line| {
            if selects(line) {
                selected += 1;
                edited(line, &edit)
            } else {
                line.to_owned()
            }
        })
        .collect();
    assert!(selected > 0, "the fixture has no line to edit");
    joined(lines.iter().map(String::as_str))
}

/// `transcript` with the session left out of its user messages, as the SDK allows.
pub(super) fn without_user_sessions(transcript: &str) -> String {
    editing(
        transcript,
        |line| is(line, "user"),
        |event| {
            event.remove("session_id").unwrap();
        },
    )
}

/// `transcript` with its first event of `kind` given a message Claude Code might send
/// in some later version.
pub(super) fn reshaped(transcript: &str, kind: &str) -> String {
    let at = index(transcript, |line| is(line, kind));
    let lines = transcript.lines().enumerate().map(|(number, line)| {
        if number == at {
            edited(line, |event| {
                event.insert("message".to_owned(), "a new shape".into());
            })
        } else {
            line.to_owned()
        }
    });
    lines.map(|line| format!("{line}\n")).collect()
}

/// Every string a JSON value holds, keys aside.
fn strings(value: &Value) -> Vec<&str> {
    match value {
        Value::String(text) => vec![text],
        Value::Array(items) => items.iter().flat_map(strings).collect(),
        Value::Object(fields) => fields.values().flat_map(strings).collect(),
        _ => Vec::new(),
    }
}

#[test]
fn fixtures_name_no_home_session_or_service_of_the_recording_machine() {
    let fixtures = [
        ("finished", FINISHED),
        ("model_error", MODEL_ERROR),
        ("skill_loaded", SKILL_LOADED),
        ("read_loaded", READ_LOADED),
        ("skill_failed", SKILL_FAILED),
    ];
    for (name, fixture) in fixtures {
        for (number, line) in fixture.lines().enumerate() {
            let event: Value = serde_json::from_str(line).unwrap();
            let at = format!("{name}.jsonl line {}", number + 1);
            assert_eq!(event["session_id"], SESSION, "{at}");
            // Account connectors, such as a mail or drive service, are listed here.
            let no_servers = match &event["mcp_servers"] {
                Value::Null => true,
                Value::Array(servers) => servers.is_empty(),
                _ => false,
            };
            assert!(no_servers, "{at} lists MCP servers");
            for text in strings(&event) {
                // The trial's own home is the one home a fixture may name.
                let text = text.replace("/trial/home/", "/trial/");
                for machine in ["/home/", "/Users/", "/mnt/", ":\\", "\\Users\\"] {
                    assert!(!text.contains(machine), "{at} holds {machine}: {text}");
                }
            }
        }
    }
}
