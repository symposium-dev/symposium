use std::fs;
use std::path::{Path, PathBuf};

use expect_test::expect;
use symposium_testlib::{TestContext, TestMode, with_fixture};

use crate::SCOPES;

fn tree(dir: &Path) -> String {
    let mut lines = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for path in entries.flatten().map(|entry| entry.path()) {
            let rel = path
                .strip_prefix(dir)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if path.is_dir() {
                lines.push(format!("{rel}/"));
                stack.push(path);
            } else {
                lines.push(rel);
            }
        }
    }
    lines.sort();
    lines.join("\n")
}

fn workspace(ctx: &TestContext) -> PathBuf {
    ctx.workspace_root.clone().unwrap()
}

fn project_plugins(ctx: &TestContext) -> PathBuf {
    workspace(ctx).join(".claude/skills")
}

fn compiled(ctx: &TestContext) -> PathBuf {
    workspace(ctx).join(".symposium/plugins")
}

#[tokio::test]
async fn project_plugins_reach_claude_as_plugins_in_the_project() {
    with_fixture(
        TestMode::SimulationOnly,
        &["agent-plugin-scopes0", "member-agents-skills0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            expect![[r#"
                always-guide/
                always-guide/.claude-plugin/
                always-guide/.claude-plugin/plugin.json
                always-guide/.gitignore
                always-guide/.symposium
                always-guide/plugin.json
                always-guide/skills/
                always-guide/skills/always-guide/
                always-guide/skills/always-guide/SKILL.md
                tool/
                tool/.claude-plugin/
                tool/.claude-plugin/plugin.json
                tool/.gitignore
                tool/.symposium
                tool/plugin.json
                tool/skills/
                tool/skills/plain-notes/
                tool/skills/plain-notes/SKILL.md
                tool/skills/tool-maintainer/
                tool/skills/tool-maintainer/SKILL.md
                workspace-guide/
                workspace-guide/.claude-plugin/
                workspace-guide/.claude-plugin/plugin.json
                workspace-guide/.gitignore
                workspace-guide/.symposium
                workspace-guide/plugin.json
                workspace-guide/skills/
                workspace-guide/skills/workspace-guide/
                workspace-guide/skills/workspace-guide/SKILL.md"#]]
            .assert_eq(&tree(&project_plugins(&ctx)));
            expect![[r#"
                .gitignore
                plugins/
                plugins/always-guide/
                plugins/always-guide/.claude-plugin/
                plugins/always-guide/.claude-plugin/plugin.json
                plugins/always-guide/.symposium
                plugins/always-guide/plugin.json
                plugins/always-guide/skills/
                plugins/always-guide/skills/always-guide/
                plugins/always-guide/skills/always-guide/SKILL.md
                plugins/tool/
                plugins/tool/.claude-plugin/
                plugins/tool/.claude-plugin/plugin.json
                plugins/tool/.symposium
                plugins/tool/plugin.json
                plugins/tool/skills/
                plugins/tool/skills/plain-notes/
                plugins/tool/skills/plain-notes/SKILL.md
                plugins/tool/skills/tool-maintainer/
                plugins/tool/skills/tool-maintainer/SKILL.md
                plugins/workspace-guide/
                plugins/workspace-guide/.claude-plugin/
                plugins/workspace-guide/.claude-plugin/plugin.json
                plugins/workspace-guide/.symposium
                plugins/workspace-guide/plugin.json
                plugins/workspace-guide/skills/
                plugins/workspace-guide/skills/workspace-guide/
                plugins/workspace-guide/skills/workspace-guide/SKILL.md"#]]
            .assert_eq(&tree(&workspace(&ctx).join(".symposium")));
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn moving_a_plugin_between_scopes_leaves_nothing_behind() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "claude"]).await?;
        ctx.symposium(&["sync"]).await?;
        let user_plugin = ctx.sym.home_dir().join(".claude/skills/global-guide");
        assert!(user_plugin.is_dir());

        ctx.symposium(&["use", "--remove", "--global", "global-guide"])
            .await?;
        ctx.symposium(&["use", "global-guide"]).await?;
        assert!(!user_plugin.exists());
        assert!(
            project_plugins(&ctx)
                .join("global-guide/skills/global-guide/SKILL.md")
                .is_file()
        );

        ctx.symposium(&["use", "--remove", "global-guide"]).await?;
        ctx.symposium(&["use", "--global", "global-guide"]).await?;
        assert!(user_plugin.is_dir());
        for path in [
            compiled(&ctx).join("global-guide"),
            project_plugins(&ctx).join("global-guide"),
        ] {
            assert!(!path.exists(), "{} is left behind", path.display());
        }
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn per_skill_copies_from_before_give_way_to_the_project_plugin() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        let old_copy = project_plugins(&ctx).join("workspace-guide-1a2b3c4d");
        fs::create_dir_all(&old_copy)?;
        fs::write(old_copy.join("SKILL.md"), "old")?;
        fs::write(old_copy.join(".symposium"), "")?;

        ctx.symposium(&["init", "--add-agent", "claude"]).await?;
        ctx.symposium(&["sync"]).await?;

        assert!(!old_copy.exists());
        assert!(
            project_plugins(&ctx)
                .join("workspace-guide/skills/workspace-guide/SKILL.md")
                .is_file()
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn removing_claude_removes_its_project_plugins() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "claude", "--add-agent", "codex"])
            .await?;
        ctx.symposium(&["sync"]).await?;
        let plugin = project_plugins(&ctx).join("workspace-guide");
        assert!(plugin.is_dir());
        assert!(compiled(&ctx).join("workspace-guide").is_dir());

        ctx.symposium(&["init", "--remove-agent", "claude"]).await?;

        assert!(!plugin.exists());
        assert!(!compiled(&ctx).join("workspace-guide").exists());
        assert!(
            workspace(&ctx)
                .join(".agents/skills/workspace-guide/SKILL.md")
                .is_file(),
            "codex keeps its per-skill copy"
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn a_users_own_project_plugin_is_left_alone() {
    with_fixture(TestMode::SimulationOnly, SCOPES, async |mut ctx| {
        let users_own = project_plugins(&ctx).join("workspace-guide");
        fs::create_dir_all(users_own.join(".claude-plugin"))?;
        fs::write(users_own.join(".claude-plugin/plugin.json"), "{}")?;

        ctx.symposium(&["init", "--add-agent", "claude"]).await?;
        let events = ctx.sync_with_report(tracing::Level::INFO).await?;

        assert_eq!(
            fs::read_to_string(users_own.join(".claude-plugin/plugin.json"))?,
            "{}"
        );
        assert!(!users_own.join(".symposium").exists());
        assert!(
            events.iter().any(|e| e["kind"] == "warning"
                && e["message"]
                    .to_string()
                    .contains("not managed by symposium")),
            "{events:#?}"
        );
        Ok(())
    })
    .await
    .unwrap();
}
