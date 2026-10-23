use std::fs;
use std::path::{Path, PathBuf};

use expect_test::expect;
use symposium::hook_schema::{HookAgent, HookEvent};
use symposium_testlib::{HookStep, TestContext, TestMode, with_fixture};

fn import(ctx: &TestContext, source: &Path, out: &Path) -> String {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_cargo-agents"))
        .args(["plugin", "import"])
        .arg(source)
        .arg("--out")
        .arg(out)
        .env("SYMPOSIUM_HOME", ctx.sym.config_dir())
        .output()
        .expect("spawn cargo-agents");
    let workspace = ctx
        .workspace_root
        .clone()
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    ctx.normalize_paths(&format!(
        "exit: {}\n{}{}",
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    ))
    .replace(&workspace, "$WORKSPACE")
}

fn source(ctx: &TestContext) -> PathBuf {
    ctx.workspace_root.clone().unwrap().join("claude-plugin")
}

fn tree(root: &Path) -> String {
    let mut paths = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                paths.push(
                    path.strip_prefix(root)
                        .unwrap()
                        .display()
                        .to_string()
                        .replace('\\', "/"),
                );
            }
        }
    }
    paths.sort();
    paths.join("\n")
}

#[tokio::test]
async fn import_writes_a_package_and_reports_what_it_leaves_out() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-import0"],
        async |ctx| {
            let out = ctx.sym.config_dir().join("plugins");
            expect![[r#"
                exit: 0
                ⚠️  name `Demo_Tools` normalized to `demo-tools`
                ⚠️  `author.phone` not imported
                ⚠️  `userConfig` in `.claude-plugin/plugin.json` not imported
                ⚠️  `agents` not imported: Agent Plugins has no agents
                ⚠️  command `guide` not imported: a skill named `guide` already exists
                ⚠️  command `review_code` imported as skill `review-code`
                ⚠️  command `undescribed` not imported: its frontmatter has no `description`
                ⚠️  `Notification` hooks not imported: Symposium has no such event
                ⚠️  `PreToolUse` hook not imported: only `type: command` hooks are supported
                ⚠️  `PreToolUse` hook `true` not imported: Symposium does not support `if`
                ⚠️  `SessionStart` hook `sh "${CLAUDE_PLUGIN_ROOT}/hooks/session-start.sh"`: `timeout` dropped
                ⚠️  MCP server `local` not imported: it refers to `${CLAUDE_PLUGIN_ROOT}`, which agents do not expand outside Claude Code plugins
                ⚠️  skill `guide` refers to `${CLAUDE_PLUGIN_ROOT}`, which only Claude Code fills in for its own plugins
                Imported $WORKSPACE/claude-plugin into $CONFIG_DIR/plugins/demo-tools
                  ⚠️  demo-tools (plugin): plugin `demo-tools` references no dependency; it stays dormant until enabled with `cargo agents use demo-tools`
                  ⚠️  demo-tools (plugin): `PreToolUse` hooks have only the `claude` format, so they fire on no other agent; add a `format = "symposium"` hook for the others
                  ⚠️  demo-tools (plugin): `SessionStart` hooks have only the `claude` format, so they fire on no other agent; add a `format = "symposium"` hook for the others
                  ✅ guide (skill)
                  ✅ review-code (skill)
            "#]]
            .assert_eq(&import(&ctx, &source(&ctx), &out));

            let package = out.join("demo-tools");
            expect![[r#"
                Symposium.toml
                hooks/session-start.sh
                plugin.json
                skills/guide/SKILL.md
                skills/review-code/SKILL.md"#]]
            .assert_eq(&tree(&package));
            expect![[r#"
                {
                  "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
                  "name": "demo-tools",
                  "version": "1.2.0",
                  "description": "Demo tools for import tests",
                  "author": {
                    "name": "Demo",
                    "url": "https://example.com"
                  },
                  "license": "MIT",
                  "keywords": [
                    "demo"
                  ]
                }
            "#]]
            .assert_eq(&fs::read_to_string(package.join("plugin.json")).unwrap());
            expect![[r#"
                [[skills]]
                source.path = "skills"

                [[hooks]]
                name = "pre-tool-use-1"
                event = "PreToolUse"
                format = "claude"
                matcher = "Bash"
                command = { executable = "sh" }
                args = ["-c", "echo '{}'"]

                [[hooks]]
                name = "session-start-2"
                event = "SessionStart"
                format = "claude"
                command = { executable = "sh" }
                args = ["-c", 'sh "${CLAUDE_PLUGIN_ROOT}/hooks/session-start.sh"']

                [[mcp_servers]]
                type = "http"
                name = "docs"
                url = "https://mcp.example.com/docs"
                headers = [{ name = "X-Key", value = "k" }]

                [[mcp_servers]]
                name = "tool"
                command = "npx"
                args = ["-y", "demo-mcp"]
                env = [{ name = "DEMO_MODE", value = "1" }]
            "#]]
            .assert_eq(&fs::read_to_string(package.join("Symposium.toml")).unwrap());
            expect![[r#"
                ---
                name: review-code
                description: Review the staged changes
                argument-hint: <path>
                ---
                Review $ARGUMENTS.
            "#]]
            .assert_eq(&fs::read_to_string(package.join("skills/review-code/SKILL.md")).unwrap());

            expect![[r#"
                exit: 1
                ⚠️  name `Demo_Tools` normalized to `demo-tools`
                $WORKSPACE/claude-plugin: `$CONFIG_DIR/plugins/demo-tools` already exists
            "#]].assert_eq(&import(&ctx, &source(&ctx), &out));
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_imported_package_installs_its_skills_and_runs_its_hooks_in_claude() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-import0"],
        async |mut ctx| {
            let package = ctx.sym.config_dir().join("plugins");
            import(&ctx, &source(&ctx), &package);
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["use", "demo-tools"]).await?;

            let skills = ctx.workspace_root.clone().unwrap().join(".claude/skills");
            assert!(skills.join("guide/SKILL.md").is_file());
            assert!(skills.join("review-code/SKILL.md").is_file());

            let result = ctx
                .prompt_or_hook("ignored", &[HookStep::session_start()], HookAgent::Claude)
                .await?;
            assert!(
                result.has_context_containing("demo-tools from ")
                    && result.has_context_containing("/plugins/demo-tools"),
                "{:#?}",
                result.outputs_for(HookEvent::SessionStart),
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}
