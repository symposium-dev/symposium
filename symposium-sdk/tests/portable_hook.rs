//! Exercise the actual portable hook process: stdin, stdout, stderr, exit
//! status, and argument handling are all part of an agent's hook protocol.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::OnceLock;

use serde_json::{Value, json};

const CONTEXT: &str = "Review tool arguments before execution.";
const DENIAL: &str = "Refusing to remove the root directory.";

/// Cargo does not expose `CARGO_BIN_EXE_*` for examples. Build this example
/// once and use Cargo's artifact path instead of assuming a target directory
/// or a platform-specific executable suffix.
fn example_binary() -> &'static Path {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY.get_or_init(|| {
        let output = Command::new(env!("CARGO"))
            .args([
                "build",
                "--locked",
                "--package",
                "symposium-sdk",
                "--example",
                "portable_hook",
                "--message-format=json",
            ])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .expect("build portable hook example");
        assert!(
            output.status.success(),
            "example build failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout),
        );
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find_map(|message| {
                (message["reason"] == "compiler-artifact"
                    && message["target"]["name"] == "portable_hook")
                    .then(|| message["executable"].as_str().map(PathBuf::from))
                    .flatten()
            })
            .expect("Cargo emitted the example executable path")
    })
}

fn run_hook(args: &[&str], input: &str) -> Output {
    let mut child = Command::new(example_binary())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run portable hook");
    // An invalid argument can make the process exit before it reads stdin.
    let write = child.stdin.take().unwrap().write_all(input.as_bytes());
    if let Err(error) = write {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
    }
    child.wait_with_output().unwrap()
}

fn tool_input(event_name: &str, command: &str) -> String {
    json!({
        "hook_event_name": event_name,
        "tool_name": "Bash",
        "tool_input": { "command": command, "working_directory": "project" },
        "session_id": "portable-session",
        "cwd": "/workspace",
    })
    .to_string()
}

fn successful_json(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "hook failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        output.stderr.is_empty(),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout must contain only the agent response: {error}: {}",
            String::from_utf8_lossy(&output.stdout),
        )
    })
}

#[test]
fn default_uses_claude_compatible_output_for_shared_input() {
    let output = run_hook(&[], &tool_input("PreToolUse", "cargo check"));
    assert_eq!(
        successful_json(&output),
        json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "additionalContext": CONTEXT,
            }
        }),
    );
}

#[test]
fn explicit_codex_emits_the_same_context_protocol() {
    let output = run_hook(&["--agent=codex"], &tool_input("PreToolUse", "cargo check"));
    assert_eq!(
        successful_json(&output),
        json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "additionalContext": CONTEXT,
            }
        }),
    );
}

#[test]
fn fallback_recognizes_kiro_and_returns_plaintext() {
    for args in [vec![], vec!["--agent=guess"]] {
        let output = run_hook(&args, &tool_input("preToolUse", "cargo check"));
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim_end(),
            CONTEXT
        );
    }
}

#[test]
fn explicit_symposium_uses_the_canonical_protocol() {
    let output = run_hook(
        &["--agent=symposium"],
        &json!({ "SessionStart": { "session_id": "session", "cwd": "/workspace" } }).to_string(),
    );
    assert_eq!(
        successful_json(&output),
        json!({ "SessionStart": {
            "additionalContext": "Prefer small changes and run relevant tests."
        } }),
    );
}

#[test]
fn explicit_selection_does_not_fall_back_to_another_agent() {
    for (args, event_name) in [
        (["--agent=claude"], "preToolUse"),
        (["--agent=kiro"], "PreToolUse"),
    ] {
        let output = run_hook(&args, &tool_input(event_name, "cargo check"));
        assert!(!output.status.success(), "{args:?} accepted {event_name}");
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}

#[test]
fn claude_and_codex_keep_denial_and_its_reason() {
    for args in [vec![], vec!["--agent=claude"], vec!["--agent=codex"]] {
        let output = run_hook(&args, &tool_input("PreToolUse", "rm -rf /"));
        let response = successful_json(&output);
        assert_eq!(response["hookSpecificOutput"]["permissionDecision"], "deny");
        assert_eq!(
            response["hookSpecificOutput"]["permissionDecisionReason"],
            DENIAL,
        );
        assert_eq!(
            response["hookSpecificOutput"]["hookEventName"],
            "PreToolUse"
        );
    }
}

#[test]
fn claude_and_codex_preserve_replacement_input_as_json() {
    for agent in ["--agent=claude", "--agent=codex"] {
        let output = run_hook(&[agent], &tool_input("PreToolUse", "cargo test"));
        let response = successful_json(&output);
        assert_eq!(
            response["hookSpecificOutput"]["updatedInput"],
            json!({ "command": "cargo test --locked", "working_directory": "project" }),
        );
    }
}

#[test]
fn kiro_denial_uses_stderr_and_exit_status() {
    for args in [vec![], vec!["--agent=kiro"]] {
        let output = run_hook(&args, &tool_input("preToolUse", "rm -rf /"));
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert_eq!(String::from_utf8(output.stderr).unwrap().trim_end(), DENIAL);
    }
}

#[test]
fn kiro_rejects_input_replacement_instead_of_silently_dropping_it() {
    let output = run_hook(&["--agent=kiro"], &tool_input("preToolUse", "cargo test"));
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8(output.stderr).unwrap();
    assert!(diagnostic.contains("kiro"), "{diagnostic}");
    assert!(diagnostic.contains("input replacement"), "{diagnostic}");
}

#[test]
fn explicit_copilot_uses_the_event_hint_and_decodes_string_tool_args() {
    let output = run_hook(
        &["--agent=copilot", "--event=pre-tool-use"],
        &json!({
            "timestamp": 1770745225000_i64,
            "toolName": "bash",
            "toolArgs": json!({ "command": "rm -rf /" }).to_string(),
            "cwd": "/workspace",
        })
        .to_string(),
    );
    assert_eq!(
        successful_json(&output),
        json!({
            "permissionDecision": "deny",
            "permissionDecisionReason": DENIAL,
            "additionalContext": DENIAL,
        }),
    );
}

#[test]
fn event_hint_supplies_a_missing_discriminator() {
    let mut payload: Value =
        serde_json::from_str(&tool_input("PreToolUse", "cargo check")).unwrap();
    payload.as_object_mut().unwrap().remove("hook_event_name");
    for args in [
        vec!["--agent=claude", "--event=pre-tool-use"],
        vec!["--agent", "claude", "--event", "pre-tool-use"],
    ] {
        let output = run_hook(&args, &payload.to_string());
        assert_eq!(
            successful_json(&output)["hookSpecificOutput"]["additionalContext"],
            CONTEXT,
        );
    }
}

#[test]
fn event_hint_cannot_override_a_conflicting_payload() {
    let output = run_hook(
        &["--event=session-start"],
        &tool_input("PreToolUse", "cargo check"),
    );
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}

#[test]
fn invalid_input_and_arguments_produce_only_diagnostics() {
    let cases: &[(&[&str], &str)] = &[
        (&[], "not JSON"),
        (&[], "[]"),
        (&[], "{}"),
        (&[], r#"{"hook_event_name":"UnknownEvent"}"#),
        (&[], r#"{"hook_event_name":"PreToolUse"}"#),
        (&["--agent=unknown"], "{}"),
        (&["--unknown"], "{}"),
        (&["--agent"], "{}"),
        (&["--event=unknown"], "{}"),
    ];
    for (args, payload) in cases {
        let output = run_hook(args, payload);
        assert!(
            !output.status.success(),
            "unexpected success: args={args:?}, payload={payload}",
        );
        assert!(output.stdout.is_empty(), "stdout leaked: {output:?}");
        assert!(
            !output.stderr.is_empty(),
            "missing error: {args:?} {payload}"
        );
    }
}

#[test]
fn an_unhandled_event_has_no_stdout() {
    let output = run_hook(&[], r#"{"hook_event_name":"Stop"}"#);
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn help_does_not_require_json_input() {
    for arg in ["--help", "-h"] {
        let output = run_hook(&[arg], "");
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let help = String::from_utf8(output.stdout).unwrap();
        assert!(help.contains("--agent"), "{help}");
        assert!(help.contains("--event"), "{help}");
    }
}
