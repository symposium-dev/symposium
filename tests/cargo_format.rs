use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    thread,
    time::{Duration, SystemTime},
};
use symposium::{
    cargo_fmt::{
        collect_rust_file_mtimes, maybe_suggest_rust_fmt, rust_files_changed_since,
        rust_fmt_suggestion_text, snapshot_rust_files,
    },
    config::{FormatReminderPolicy, Symposium},
    hook::{PostToolUsePayload, handle_post_tool_use},
    session_state::{self, SessionData},
};
use {serde_json, tempfile};

const MTIME_SLEEP_MS: u64 = 10;

fn touch(path: &Path) {
    fs::write(path, b"fn main() {}").unwrap();
}

fn make_session_with_snapshot(root: &Path) -> SessionData {
    SessionData {
        rust_file_snapshot: snapshot_rust_files(root),
        rust_fmt_reminder_sent: false,
        ..Default::default()
    }
}

fn make_temp_rs(root: &Path, name: &str) -> PathBuf {
    let path = root.join(name);
    touch(&path);
    path
}

fn modify_file(path: &Path) {
    // Sleep to ensure the mtime actually changes on filesystems.
    thread::sleep(Duration::from_millis(MTIME_SLEEP_MS));
    fs::write(path, b"fn main() {println!(\"changed\");}").unwrap()
}

#[test]
fn snapshot_rust_files_collects_only_rs_files_recursively() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    let subdir = root.join("sub");
    fs::create_dir(&subdir).unwrap();

    let rs1 = make_temp_rs(root, "a.rs");
    let rs2 = make_temp_rs(&subdir, "b.rs");
    let txt = root.join("ignore.txt");
    fs::write(&txt, b"not rust").unwrap();

    let snapshot = snapshot_rust_files(root);

    assert!(snapshot.contains_key(&rs1));
    assert!(snapshot.contains_key(&rs2));
    assert!(!snapshot.contains_key(&txt));
}

#[test]
fn collect_rust_file_mtimes_ignores_unreadable_directories() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    let rs1 = make_temp_rs(root, "a.rs");

    let mut mtimes: BTreeMap<PathBuf, SystemTime> = BTreeMap::new();
    collect_rust_file_mtimes(root, &mut mtimes);

    assert!(mtimes.contains_key(&rs1))
}

#[test]
fn rust_files_changed_since_is_false_when_nothing_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    let _rs1 = make_temp_rs(root, "a.rs");
    let snapshot = snapshot_rust_files(root);

    assert!(!rust_files_changed_since(root, &snapshot));
}

#[test]
fn rut_files_changed_since_detects_new_file() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    let snapshot = snapshot_rust_files(root);
    assert!(snapshot.is_empty());

    _ = make_temp_rs(root, "a.rs");
    assert!(rust_files_changed_since(root, &snapshot));
}

#[test]
fn rust_files_changed_since_detects_modified_file() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    let rs1 = make_temp_rs(root, "a.rs");
    let snapshot = snapshot_rust_files(root);

    modify_file(&rs1);

    assert!(rust_files_changed_since(root, &snapshot));
}

#[test]
fn rust_files_changed_since_detects_deleted_file() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    let rs1 = make_temp_rs(root, "a.rs");
    let snapshot = snapshot_rust_files(root);

    fs::remove_file(&rs1).unwrap();

    assert!(rust_files_changed_since(root, &snapshot));
}

#[test]
fn maybe_suggest_rust_fmt_returns_none_when_no_files_changed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    let mut session = make_session_with_snapshot(root);
    let policy = FormatReminderPolicy::Once;

    assert!(maybe_suggest_rust_fmt(&mut session, root, &policy).is_none());
}

#[test]
fn maybe_suggest_rust_fmt_once_sends_only_on_first_change() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    let rs1 = root.join("a.rs");
    let mut session = make_session_with_snapshot(root);
    let policy = FormatReminderPolicy::Once;

    // First change -- should remind
    touch(&rs1);
    let suggestion1 = maybe_suggest_rust_fmt(&mut session, root, &policy);
    assert!(suggestion1.is_some());
    assert!(session.rust_fmt_reminder_sent);

    // Second change -- snapshot was refreshed after first call,
    // so we need to modify the file again to trigger a change.
    // But policy is Once so reminder should not be sent again.
    modify_file(&rs1);
    let suggestion2 = maybe_suggest_rust_fmt(&mut session, root, &policy);
    assert!(suggestion2.is_none());
}

#[test]
fn maybe_suggest_rust_fmt_always_sends_on_every_change() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    let rs1 = root.join("a.rs");
    let mut session = make_session_with_snapshot(root);
    let policy = FormatReminderPolicy::Always;

    touch(&rs1);
    assert!(maybe_suggest_rust_fmt(&mut session, root, &policy).is_some());

    // Snapshot refreshed -- modify again to trigger second change.
    modify_file(&rs1);
    assert!(maybe_suggest_rust_fmt(&mut session, root, &policy).is_some());
}

#[test]
fn maybe_suggest_rust_fmt_never_sends_even_when_file_change() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    let rs1 = root.join("a.rs");
    let mut session = make_session_with_snapshot(root);
    let policy = FormatReminderPolicy::Never;

    touch(&rs1);
    assert!(maybe_suggest_rust_fmt(&mut session, root, &policy).is_none());
}

#[test]
fn rust_fmt_suggestion_text_has_expected_content() {
    let msg = rust_fmt_suggestion_text();
    assert!(msg.contains("One or more Rust source files were modified"));
    assert!(msg.contains("cargo fmt"));
}

#[tokio::test]
async fn handle_post_tool_use_sends_format_reminder() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // 1. Setup config with Always policy
    let mut sym = Symposium::from_dir(root);
    sym.config.hooks.remind_format_policy = FormatReminderPolicy::Always;

    // 2. Create an initial rust file and snapshot it.
    let rs_path = root.join("main.rs");
    fs::write(
        &rs_path,
        b"fn main() -> Result<()> { println!(\"Rust is the best\"); Ok(()) ",
    )
    .unwrap();

    let session_id = "test-session-fmt";
    let mut session = SessionData::default();
    session.rust_file_snapshot = snapshot_rust_files(root);
    session_state::save_session(&sym, session_id, &session);

    // 3. Modify the file to trigger a change.
    // Sleep briefly to ensure mtime changes.
    std::thread::sleep(Duration::from_millis(15));
    fs::write(&rs_path, b"// modified code").unwrap();

    // 4. Simulate PostToolUse event (e.g., after a Bash command);
    let payload = PostToolUsePayload {
        tool_name: "Bash".to_string(),
        tool_input: serde_json::json!({"command": "echo done"}),
        tool_response: serde_json::json!({"exit_code": 0}),
        session_id: Some(session_id.to_string()),
        cwd: Some(root.to_str().unwrap().to_string()),
    };

    let output = handle_post_tool_use(&sym, &payload).await;

    // 5. Verify the reminder is sent in the HookOutput.
    let specific = output.hook_specific_output.unwrap();
    assert_eq!(specific.hook_event_name, "PostToolUse");
    let context = specific.additional_context.unwrap();
    assert!(context.contains("cargo fmt"));
    assert!(context.contains("modified"));
}
