//! Regression: a nested workspace created mid-session must replace the cached parent.
use symposium::hook_schema::{HookAgent, HookEvent};
use symposium_testlib::{TestMode, with_fixture};

#[tokio::test]
async fn creating_a_nested_workspace_invalidates_the_hook_workspace_cache() {
    with_fixture(TestMode::SimulationOnly, &["plugins0", "workspace0"], async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "claude"]).await?;
        ctx.sym.config.sync_debounce_secs = 0;
        let parent = ctx.workspace_root.clone().unwrap();
        let nested = parent.join("new-project");
        std::fs::create_dir(&nested)?;
        let payload = serde_json::json!({
            "session_id": "nested-project", "cwd": nested,
            "hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {},
            "tool_use_id": "nested-project"
        });
        ctx.invoke_hook(HookAgent::Claude, HookEvent::PreToolUse, &payload).await?;
        let parent_lock_mtime = std::fs::metadata(parent.join("Cargo.lock"))?.modified()?;

        std::fs::create_dir(nested.join("src"))?;
        std::fs::write(nested.join("src/lib.rs"), "")?;
        std::fs::write(nested.join("Cargo.toml"), "[package]\nname = \"new-project\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[workspace]\n")?;
        std::fs::create_dir_all(nested.join("skills/nested-guidance"))?;
        std::fs::write(nested.join("skills/nested-guidance/SKILL.md"), "---\nname: nested-guidance\ndescription: Rules for the new project.\n---\nNew project's guidance.\n")?;

        ctx.invoke_hook(HookAgent::Claude, HookEvent::PreToolUse, &payload).await?;
        assert_eq!(std::fs::metadata(parent.join("Cargo.lock"))?.modified()?, parent_lock_mtime);
        let installed = nested.join(".claude/skills");
        assert!(std::fs::read_dir(&installed).is_ok_and(|entries| entries.flatten().any(|entry| entry.path().join("SKILL.md").is_file())), "the hook must discover the new nested workspace and install its skill at {}", installed.display());
        Ok(())
    }).await.unwrap();
}
