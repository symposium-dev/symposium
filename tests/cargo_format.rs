use std::{fs, time::Duration};
use symposium::hook::PostToolUsePayload;
use symposium_testlib::with_fixture;

#[tokio::test]
async fn fmt_not_sent_when_no_config_exist_and_no_file_activity() {
    let ctx = with_fixture(&["no_fmt", "workspace0"]);

    // Establish a baseline snapshot
    ctx.invoke_hook(PostToolUsePayload {
        tool_name: "Bash".to_string(),
        tool_input: serde_json::json!({}),
        tool_response: serde_json::json!({}),
        session_id: Some("test-no-formatting".to_string()),
        cwd: Some(ctx.tempdir.path().to_str().unwrap().to_string()),
    })
    .await;

    // Second invocation
    let output = ctx
        .invoke_hook(PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            tool_response: serde_json::json!({}),
            session_id: Some("test-no-formatting".to_string()),
            cwd: Some(ctx.tempdir.path().to_str().unwrap().to_string()),
        })
        .await;

    assert!(output.hook_specific_output.is_none());
}

#[tokio::test]
async fn never_fmt_reminder_never_sends() {
    let ctx = with_fixture(&["fmt_never", "workspace0"]);

    // Create a .rs file
    std::fs::write(ctx.tempdir.path().join("main.rs"), b"fn main() {}").unwrap();

    // First invocation
    let output = ctx
        .invoke_hook(PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            tool_response: serde_json::json!({}),
            session_id: Some("test".to_string()),
            cwd: Some(ctx.tempdir.path().to_str().unwrap().to_string()),
        })
        .await;

    assert!(output.hook_specific_output.is_none());
}

#[tokio::test]
async fn always_fmt_reminder_always_sends_on_file_activity() {
    let ctx = with_fixture(&["fmt_always", "workspace0"]);

    // Create a .rs file
    std::fs::write(ctx.tempdir.path().join("a.rs"), b"pub (crate) mod skills").unwrap();

    // First invocation
    let output = ctx
        .invoke_hook(PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            tool_response: serde_json::json!({}),
            session_id: Some("test-always".to_string()),
            cwd: Some(ctx.tempdir.path().to_str().unwrap().to_string()),
        })
        .await;

    assert!(output.hook_specific_output.is_some());

    // Format suggestion text
    let msg = output
        .hook_specific_output
        .unwrap()
        .additional_context
        .unwrap();

    assert!(msg.contains("Rust source files were modified"));

    assert!(msg.contains("cargo fmt"));

    // Create a new file to trigger a change.
    // Sleep briefly to ensure mtime changes.
    std::thread::sleep(Duration::from_millis(15));
    std::fs::write(
        ctx.tempdir.path().join("b.rs"),
        b"fn main() -> Result<()> {println!(); Ok(())}",
    )
    .unwrap();

    // Second invocation
    let output = ctx
        .invoke_hook(PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            tool_response: serde_json::json!({}),
            session_id: Some("test_always".to_string()),
            cwd: Some(ctx.tempdir.path().to_str().unwrap().to_string()),
        })
        .await;

    assert!(output.hook_specific_output.is_some());

    // Format suggestion text
    let msg = output
        .hook_specific_output
        .unwrap()
        .additional_context
        .unwrap();

    assert!(msg.contains("Rust source files were modified"));

    assert!(msg.contains("cargo fmt"));
}

#[tokio::test]
async fn once_fmt_reminder_sends_once() {
    let ctx = with_fixture(&["fmt_once", "workspace0"]);

    // Create a .rs file
    std::fs::write(ctx.tempdir.path().join("c.rs"), b"fn serve() {}").unwrap();

    // First invocation
    let output = ctx
        .invoke_hook(PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            tool_response: serde_json::json!({}),
            session_id: Some("test-once".to_string()),
            cwd: Some(ctx.tempdir.path().to_str().unwrap().to_string()),
        })
        .await;

    assert!(output.hook_specific_output.is_some());

    // Format suggestion text
    let msg = output
        .hook_specific_output
        .unwrap()
        .additional_context
        .unwrap();

    assert!(msg.contains("Rust source files were modified"));

    assert!(msg.contains("cargo fmt"));

    // Modify the file to trigger a change.
    // Sleep briefly to ensure mtime changes.
    std::thread::sleep(Duration::from_millis(15));
    std::fs::write(ctx.tempdir.path().join("c.rs"), b"// new content").unwrap();

    // Second invocation
    let output = ctx
        .invoke_hook(PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            tool_response: serde_json::json!({}),
            session_id: Some("test-once".to_string()),
            cwd: Some(ctx.tempdir.path().to_str().unwrap().to_string()),
        })
        .await;

    assert!(output.hook_specific_output.is_none());
}

#[tokio::test]
async fn once_fmt_reminder_sends_once_on_delete() {
    let ctx = with_fixture(&["fmt_once", "workspace0"]);

    std::fs::write(ctx.tempdir.path().join("main.rs"), b"fn main() {}").unwrap();

    // First invocation
    let output = ctx
        .invoke_hook(PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            tool_response: serde_json::json!({}),
            session_id: Some("test-delete".to_string()),
            cwd: Some(ctx.tempdir.path().to_str().unwrap().to_string()),
        })
        .await;

    assert!(output.hook_specific_output.is_some());

    // Format suggestion text
    let msg = output
        .hook_specific_output
        .unwrap()
        .additional_context
        .unwrap();

    assert!(msg.contains("Rust source files were modified"));

    assert!(msg.contains("cargo fmt"));

    // Remove the file to trigger a change.
    // Sleep briefly to ensure mtime changes.
    std::thread::sleep(Duration::from_millis(15));
    std::fs::remove_file(ctx.tempdir.path().join("main.rs")).unwrap();

    // Second invocation
    let output = ctx
        .invoke_hook(PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            tool_response: serde_json::json!({}),
            session_id: Some("test-delete".to_string()),
            cwd: Some(ctx.tempdir.path().to_str().unwrap().to_string()),
        })
        .await;

    assert!(output.hook_specific_output.is_none());
}

#[tokio::test]
async fn formatting_defaults_to_once_when_not_set() {
    let ctx = with_fixture(&["no_fmt", "workspace0"]);

    // Create file
    std::fs::write(ctx.tempdir.path().join("fmt.rs"), b"fn main() {}").unwrap();

    // First invocation
    let output = ctx
        .invoke_hook(PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            tool_response: serde_json::json!({}),
            session_id: Some("test-default".to_string()),
            cwd: Some(ctx.tempdir.path().to_str().unwrap().to_string()),
        })
        .await;

    assert!(output.hook_specific_output.is_some());

    // Format suggestion text
    let msg = output
        .hook_specific_output
        .unwrap()
        .additional_context
        .unwrap();

    assert!(msg.contains("Rust source files were modified"));

    assert!(msg.contains("cargo fmt"));

    // Modify the file to trigger a change.
    // Sleep briefly to ensure mtime changes.
    std::thread::sleep(Duration::from_millis(15));
    std::fs::write(ctx.tempdir.path().join("fmt.rs"), b"// Modified").unwrap();

    //  Second invocation
    let output = ctx
        .invoke_hook(PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            tool_response: serde_json::json!({}),
            session_id: Some("test-default".to_string()),
            cwd: Some(ctx.tempdir.path().to_str().unwrap().to_string()),
        })
        .await;

    assert!(output.hook_specific_output.is_none());
}

#[tokio::test]
async fn always_fmt_reminder_always_sends_for_file_modification() {
    let ctx = with_fixture(&["fmt_always", "workspace0"]);

    // Create .rs file
    std::fs::write(
        ctx.tempdir.path().join("mod.rs"),
        b"fn send_fmt_always() {}",
    )
    .unwrap();

    // First invocation
    let output = ctx
        .invoke_hook(PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            tool_response: serde_json::json!({}),
            session_id: Some("always-sends-on-file-change".to_string()),
            cwd: Some(ctx.tempdir.path().to_str().unwrap().to_string()),
        })
        .await;

    assert!(output.hook_specific_output.is_some());

    // Format suggestion text
    let msg = output
        .hook_specific_output
        .unwrap()
        .additional_context
        .unwrap();

    assert!(msg.contains("Rust source files were modified"));

    assert!(msg.contains("cargo fmt"));

    // Modify the file to trigger a change.
    // Sleep briefly to ensure mtime changes.
    std::thread::sleep(Duration::from_millis(15));
    std::fs::write(ctx.tempdir.path().join("mod.rs"), b"// Modified ").unwrap();

    let output = ctx
        .invoke_hook(PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            tool_response: serde_json::json!({}),
            session_id: Some("always-sends-on-file-change".to_string()),
            cwd: Some(ctx.tempdir.path().to_str().unwrap().to_string()),
        })
        .await;

    assert!(output.hook_specific_output.is_some());
    // Format suggestion text
    let msg = output
        .hook_specific_output
        .unwrap()
        .additional_context
        .unwrap();

    assert!(msg.contains("Rust source files were modified"));

    assert!(msg.contains("cargo fmt"));
}

#[tokio::test]
async fn always_fmt_reminder_always_sends_on_file_delete() {
    let ctx = with_fixture(&["fmt_always", "workspace0"]);

    // Create a .rs file
    std::fs::write(ctx.tempdir.path().join("a.rs"), b"fun rust() { ret 2; }").unwrap();
    std::fs::write(
        ctx.tempdir.path().join("b.rs"),
        b"// rust editions are awesome",
    )
    .unwrap();

    // First invocation
    let output = ctx
        .invoke_hook(PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            tool_response: serde_json::json!({}),
            session_id: Some("always-sends-on-file-delete".to_string()),
            cwd: Some(ctx.tempdir.path().to_str().unwrap().to_string()),
        })
        .await;

    assert!(output.hook_specific_output.is_some());

    // Format suggestion text
    let msg = output
        .hook_specific_output
        .unwrap()
        .additional_context
        .unwrap();

    assert!(msg.contains("Rust source files were modified"));

    assert!(msg.contains("cargo fmt"));

    // Delete the file to trigger a change.
    // Sleep briefly to ensure mtime changes.
    std::thread::sleep(Duration::from_millis(15));
    std::fs::remove_file(ctx.tempdir.path().join("a.rs")).unwrap();

    // Second invocation
    let output = ctx
        .invoke_hook(PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            tool_response: serde_json::json!({}),
            session_id: Some("always-sends-on-file-delete".to_string()),
            cwd: Some(ctx.tempdir.path().to_str().unwrap().to_string()),
        })
        .await;

    assert!(output.hook_specific_output.is_some());

    // Format suggestion text
    let msg = output
        .hook_specific_output
        .unwrap()
        .additional_context
        .unwrap();

    assert!(msg.contains("Rust source files were modified"));

    assert!(msg.contains("cargo fmt"));

    // Modify the file to trigger a change.
    // Sleep briefly to ensure mtime changes.
    std::thread::sleep(Duration::from_millis(15));
    std::fs::write(ctx.tempdir.path().join("b.rs"), b"fn main() {}").unwrap();

    // Third invocation
    let output = ctx
        .invoke_hook(PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            tool_response: serde_json::json!({}),
            session_id: Some("always-sends-on-file-delete".to_string()),
            cwd: Some(ctx.tempdir.path().to_str().unwrap().to_string()),
        })
        .await;

    assert!(output.hook_specific_output.is_some());

    // Format suggestion text
    let msg = output
        .hook_specific_output
        .unwrap()
        .additional_context
        .unwrap();

    assert!(msg.contains("Rust source files were modified"));

    assert!(msg.contains("cargo fmt"));
}

#[tokio::test]
async fn always_fmt_reminder_does_not_send_when_no_file_activity() {
    let ctx = with_fixture(&["fmt_always", "workspace0"]);

    // Create a .rs file
    std::fs::write(ctx.tempdir.path().join("lib.rs"), b"println!();").unwrap();

    // First invocation
    let output = ctx
        .invoke_hook(PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            tool_response: serde_json::json!({}),
            session_id: Some("always-sends-once".to_string()),
            cwd: Some(ctx.tempdir.path().to_str().unwrap().to_string()),
        })
        .await;

    assert!(output.hook_specific_output.is_some());

    // No file changes.
    // Sleep briefly to ensure mtime changes.
    std::thread::sleep(Duration::from_millis(15));
    // Second invocation
    let output = ctx
        .invoke_hook(PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            tool_response: serde_json::json!({}),
            session_id: Some("always-sends-once".to_string()),
            cwd: Some(ctx.tempdir.path().to_str().unwrap().to_string()),
        })
        .await;

    assert!(output.hook_specific_output.is_none());
}

#[tokio::test]
async fn once_fmt_reminder_format_sends_once_for_nested_dir() {
    let ctx = with_fixture(&["fmt_once", "workspace0"]);
    let src_dir = ctx.tempdir.path().join("src").join("plugins");
    fs::create_dir_all(&src_dir).unwrap();

    fs::write(src_dir.join("main.rs"), b"fn main() {}").unwrap();

    // First invocation
    let output = ctx
        .invoke_hook(PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            tool_response: serde_json::json!({}),
            session_id: Some("nested-dir-once".to_string()),
            cwd: Some(ctx.tempdir.path().to_str().unwrap().to_string()),
        })
        .await;

    assert!(output.hook_specific_output.is_some());
    // Format suggestion text
    let msg = output
        .hook_specific_output
        .unwrap()
        .additional_context
        .unwrap();

    assert!(msg.contains("Rust source files were modified"));

    assert!(msg.contains("cargo fmt"));

    // Modify file
    std::thread::sleep(Duration::from_millis(15));
    fs::write(src_dir.join("main.rs"), b"// Modified content ").unwrap();

    // Second invocation
    let output = ctx
        .invoke_hook(PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            tool_response: serde_json::json!({}),
            session_id: Some("nested-dir-once".to_string()),
            cwd: Some(ctx.tempdir.path().to_str().unwrap().to_string()),
        })
        .await;

    assert!(output.hook_specific_output.is_none())
}

#[tokio::test]
async fn always_fmt_reminder_format_sends_always_for_nested_dir() {
    let ctx = with_fixture(&["fmt_always", "workspace0"]);
    let src_dir = ctx
        .tempdir
        .path()
        .join("src")
        .join("hooks")
        .join("registry");

    let src_dir_2 = ctx.tempdir.path().join("src").join("mods");

    fs::create_dir_all(&src_dir).unwrap();
    fs::create_dir_all(&src_dir_2).unwrap();

    fs::write(
        src_dir.join("main.rs"),
        b"fn main() -> Result<()> { Ok(()); }",
    )
    .unwrap();

    fs::write(
        src_dir_2.join("mod.rs"),
        b"fn mods() -> Result<()> { Ok(()); }",
    )
    .unwrap();

    // First invocation
    let output = ctx
        .invoke_hook(PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            tool_response: serde_json::json!({}),
            session_id: Some("nested-dir-always".to_string()),
            cwd: Some(ctx.tempdir.path().to_str().unwrap().to_string()),
        })
        .await;

    assert!(output.hook_specific_output.is_some());
    // Format suggestion text
    let msg = output
        .hook_specific_output
        .unwrap()
        .additional_context
        .unwrap();

    assert!(msg.contains("Rust source files were modified"));

    assert!(msg.contains("cargo fmt"));

    // Delete file to trigger change
    std::thread::sleep(Duration::from_millis(15));
    fs::remove_file(src_dir.join("main.rs")).unwrap();

    // Second Invocation
    let output = ctx
        .invoke_hook(PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            tool_response: serde_json::json!({}),
            session_id: Some("nested-dir-always".to_string()),
            cwd: Some(ctx.tempdir.path().to_str().unwrap().to_string()),
        })
        .await;

    assert!(output.hook_specific_output.is_some());
    // Format suggestion text
    let msg = output
        .hook_specific_output
        .unwrap()
        .additional_context
        .unwrap();

    assert!(msg.contains("Rust source files were modified"));

    assert!(msg.contains("cargo fmt"));

    // Modify file
    std::thread::sleep(Duration::from_millis(15));
    fs::write(src_dir_2.join("mod.rs"), b"// Modified file").unwrap();

    // Third invocation
    let output = ctx
        .invoke_hook(PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            tool_response: serde_json::json!({}),
            session_id: Some("nested-dir-always".to_string()),
            cwd: Some(ctx.tempdir.path().to_str().unwrap().to_string()),
        })
        .await;

    // Format suggestion text
    let msg = output
        .hook_specific_output
        .unwrap()
        .additional_context
        .unwrap();

    assert!(msg.contains("Rust source files were modified"));

    assert!(msg.contains("cargo fmt"));
}

#[tokio::test]
async fn once_non_rs_file_changes_do_not_trigger_fmt_reminder() {
    let ctx = with_fixture(&["fmt_once", "workspace0"]);

    // First invocation - establish baseline snapshot
    ctx.invoke_hook(PostToolUsePayload {
        session_id: Some("non-rs-test".to_string()),
        cwd: Some(ctx.tempdir.path().to_str().unwrap().to_string()),
        tool_name: "Bash".to_string(),
        tool_input: serde_json::json!({}),
        tool_response: serde_json::json!({}),
    })
    .await;

    fs::write(ctx.tempdir.path().join("config.toml"), b"[hooks]").unwrap();

    // Second invocation - should be empty, no .rs files changed
    let output = ctx
        .invoke_hook(PostToolUsePayload {
            session_id: Some("non-rs-test".to_string()),
            cwd: Some(ctx.tempdir.path().to_str().unwrap().to_string()),
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            tool_response: serde_json::json!({}),
        })
        .await;

    assert!(output.hook_specific_output.is_none());
}

#[tokio::test]
async fn always_non_rs_file_changes_do_not_trigger_fmt_reminder() {
    let ctx = with_fixture(&["fmt_always", "workspace0"]);

    // First invocation - establish baseline snapshot
    ctx.invoke_hook(PostToolUsePayload {
        session_id: Some("non-rs-test".to_string()),
        cwd: Some(ctx.tempdir.path().to_str().unwrap().to_string()),
        tool_name: "Bash".to_string(),
        tool_input: serde_json::json!({}),
        tool_response: serde_json::json!({}),
    })
    .await;

    fs::write(ctx.tempdir.path().join("config.toml"), b"[hooks]").unwrap();

    // Second invocation - should be empty, no .rs files changed
    let output = ctx
        .invoke_hook(PostToolUsePayload {
            session_id: Some("non-rs-test".to_string()),
            cwd: Some(ctx.tempdir.path().to_str().unwrap().to_string()),
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            tool_response: serde_json::json!({}),
        })
        .await;

    assert!(output.hook_specific_output.is_none());

    std::thread::sleep(Duration::from_millis(15));
    fs::remove_file(ctx.tempdir.path().join("config.toml")).unwrap();

    // Third invocation - should be empty, no .rs files changed
    let output = ctx
        .invoke_hook(PostToolUsePayload {
            session_id: Some("non-rs-test".to_string()),
            cwd: Some(ctx.tempdir.path().to_str().unwrap().to_string()),
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            tool_response: serde_json::json!({}),
        })
        .await;

    assert!(output.hook_specific_output.is_none());
}

#[tokio::test]
async fn mixed_rs_and_a_non_rs_file_change_triggers_fmt_reminder() {
    let ctx = with_fixture(&["fmt_always", "workspace0"]);

    // change both a .rs and a non-.rs file
    fs::write(ctx.tempdir.path().join("main.rs"), b"// Mixed ").unwrap();
    fs::write(
        ctx.tempdir.path().join("readme.md"),
        b"// Project cargo agents ",
    )
    .unwrap();

    let output = ctx
        .invoke_hook(PostToolUsePayload {
            session_id: Some("mixed-rs-and-non-rs-test".to_string()),
            cwd: Some(ctx.tempdir.path().to_str().unwrap().to_string()),
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            tool_response: serde_json::json!({}),
        })
        .await;

    assert!(output.hook_specific_output.is_some());

    // Format suggestion text
    let msg = output
        .hook_specific_output
        .unwrap()
        .additional_context
        .unwrap();

    assert!(msg.contains("Rust source files were modified"));

    assert!(msg.contains("cargo fmt"));
}
