//! Integration tests for init and sync flows.

use symposium_testlib::{TestMode, with_fixture};

/// Read the user config file from the test context.
fn read_user_config(ctx: &symposium_testlib::TestContext) -> String {
    let path = ctx.sym.config_dir().join("config.toml");
    std::fs::read_to_string(&path).unwrap_or_else(|_| "(not found)".to_string())
}

/// `init` defaults to global hook scope — hooks go to home dir.
#[tokio::test]
async fn init_defaults_to_global_hooks() {
    with_fixture(TestMode::SimulationOnly, &[], async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "claude"]).await?;

        let content = read_user_config(&ctx);

        // `hook-scope` is not found because it is the default (global) and
        // hence not serialized.
        assert!(!content.contains("hook-scope"));

        let settings = ctx.sym.home_dir().join(".claude").join("settings.json");
        assert!(settings.exists(), "global hooks should be installed");
        Ok(())
    })
    .await
    .unwrap();
}

/// `init --hook-scope project` writes the setting but does NOT install hooks.
#[tokio::test]
async fn init_hook_scope_project() {
    with_fixture(TestMode::SimulationOnly, &[], async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "claude", "--hook-scope", "project"])
            .await?;

        let content = read_user_config(&ctx);

        // `hook-scope` is serialized to be the project.
        assert!(content.contains("hook-scope = \"project\""));

        let settings = ctx.sym.home_dir().join(".claude").join("settings.json");
        assert!(
            !settings.exists(),
            "project-scope should not install global hooks"
        );
        Ok(())
    })
    .await
    .unwrap();
}

/// `init` preserves existing hook-scope when no flag is given.
#[tokio::test]
async fn init_preserves_existing_hook_scope() {
    with_fixture(TestMode::SimulationOnly, &[], async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "claude", "--hook-scope", "project"])
            .await?;
        ctx.symposium(&["init", "--add-agent", "gemini"]).await?;

        let content = read_user_config(&ctx);
        assert!(
            content.contains("hook-scope = \"project\""),
            "should preserve project scope"
        );
        Ok(())
    })
    .await
    .unwrap();
}

/// `init` creates a user config with the specified agent.
#[tokio::test]
async fn init_creates_config() {
    with_fixture(TestMode::SimulationOnly, &["plugins0"], async |mut ctx| {
        ctx.symposium(&["init", "--add-agent", "claude"]).await?;

        let config = symposium::config::Symposium::from_dir(ctx.sym.config_dir());
        assert_eq!(config.config.agents.len(), 1);

        let content = read_user_config(&ctx);
        assert!(content.contains(r#"name = "claude""#));
        assert!(!content.contains("sync-default"));
        Ok(())
    })
    .await
    .unwrap();
}

/// `sync` installs skill files into the agent's expected location.
#[tokio::test]
async fn sync_installs_skills() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugins0", "workspace0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.as_ref().unwrap();

            let skill_file = workspace_root.join(".claude/skills/serde-guidance/SKILL.md");
            assert!(
                skill_file.exists(),
                "sync should install serde-guidance skill"
            );

            // Each installed skill directory carries a `.symposium` marker so
            // future syncs (and other tools) can identify it as symposium-managed.
            let marker = workspace_root.join(".claude/skills/serde-guidance/.symposium");
            assert!(
                marker.exists(),
                "skill dir should contain .symposium marker"
            );

            // Skill dirs symposium creates get a wildcard gitignore so the
            // marker, SKILL.md, and gitignore itself stay out of version control.
            for dir in [".claude/skills", ".claude/skills/serde-guidance"] {
                let gi = workspace_root.join(dir).join(".gitignore");
                assert!(gi.exists(), "missing .gitignore in {dir}");
                let contents = std::fs::read_to_string(&gi).unwrap();
                assert_eq!(contents.trim(), "*", "unexpected .gitignore in {dir}");
            }
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// `sync` rejects malformed skill frontmatter before installing skills.
#[tokio::test]
async fn sync_skips_invalid_skill_frontmatter() {
    with_fixture(
        TestMode::SimulationOnly,
        &["invalid-skill0", "workspace0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "codex"]).await?;
            let registry = symposium::plugins::load_registry(&ctx.sym);
            assert!(
                registry.warnings.iter().any(|warning| {
                    warning.path.ends_with("bad-skill/SKILL.md")
                        && warning.message.contains("failed to parse frontmatter")
                }),
                "registry should record a warning for skipped invalid skill"
            );

            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.as_ref().unwrap();
            let skill_file = workspace_root.join(".agents/skills/rust-best-practice/SKILL.md");
            assert!(
                !skill_file.exists(),
                "sync should not install a skill with invalid YAML frontmatter"
            );

            // No marker should exist for the rejected skill.
            let marker = workspace_root.join(".agents/skills/rust-best-practice/.symposium");
            assert!(
                !marker.exists(),
                "rejected skill directory should not exist"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// `sync` removes stale skills marked by a `.symposium` file.
#[tokio::test]
async fn sync_removes_stale_skills() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugins0", "workspace0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.as_ref().unwrap();

            // Plant a fake "previously installed" skill: a marker file makes
            // the dir look symposium-managed, so the next sync should reap it.
            let fake_dir = workspace_root.join(".claude/skills/fake-old-skill");
            std::fs::create_dir_all(&fake_dir).unwrap();
            std::fs::write(fake_dir.join("SKILL.md"), "old").unwrap();
            std::fs::write(fake_dir.join(".symposium"), "").unwrap();

            ctx.symposium(&["sync"]).await?;

            assert!(
                !fake_dir.exists(),
                "stale skill should be removed after sync"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// `sync` does not touch skill directories without the `.symposium` marker
/// (user-managed).
#[tokio::test]
async fn sync_preserves_user_managed_skills() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugins0", "workspace0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.as_ref().unwrap();

            let user_skill_dir = workspace_root.join(".claude/skills/my-custom-skill");
            std::fs::create_dir_all(&user_skill_dir).unwrap();
            std::fs::write(user_skill_dir.join("SKILL.md"), "custom").unwrap();

            ctx.symposium(&["sync"]).await?;

            assert!(
                user_skill_dir.join("SKILL.md").exists(),
                "user-managed skill should not be removed"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// Copilot uses vendor-neutral `.agents/skills/` path, not `.claude/skills/`.
#[test]
fn copilot_uses_vendor_neutral_skill_path() {
    let root = std::path::Path::new("/project");
    let agent = symposium::agents::Agent::Copilot;

    let skill_dir = agent.project_skill_dir(root, "serde-guidance");
    assert_eq!(
        skill_dir,
        std::path::PathBuf::from("/project/.agents/skills/serde-guidance")
    );

    let claude_dir = symposium::agents::Agent::Claude.project_skill_dir(root, "serde-guidance");
    assert_eq!(
        claude_dir,
        std::path::PathBuf::from("/project/.claude/skills/serde-guidance")
    );
}

/// Removing an agent removes its hooks.
#[tokio::test]
async fn removing_agent_removes_hooks() {
    with_fixture(TestMode::SimulationOnly, &["plugins0"], async |mut ctx| {
        ctx.symposium(&[
            "init",
            "--hook-scope",
            "global",
            "--add-agent",
            "claude",
            "--add-agent",
            "gemini",
        ])
        .await?;

        let claude_settings = ctx.sym.home_dir().join(".claude/settings.json");
        let gemini_settings = ctx.sym.home_dir().join(".gemini/settings.json");
        assert!(claude_settings.exists(), "claude settings should exist");
        assert!(gemini_settings.exists(), "gemini settings should exist");
        assert!(
            std::fs::read_to_string(&gemini_settings)
                .unwrap()
                .contains("cargo-agents hook"),
            "gemini should have symposium hooks"
        );

        ctx.symposium(&["init", "--hook-scope", "global", "--remove-agent", "gemini"])
            .await?;

        let contents = std::fs::read_to_string(&claude_settings).unwrap();
        assert!(
            contents.contains("cargo-agents hook"),
            "claude hooks should remain"
        );

        let contents = std::fs::read_to_string(&gemini_settings).unwrap();
        assert!(
            !contents.contains("cargo-agents hook"),
            "gemini hooks should be removed"
        );
        Ok(())
    })
    .await
    .unwrap();
}

/// `--add-agent` is additive to existing agents.
#[tokio::test]
async fn add_agent_is_additive() {
    with_fixture(TestMode::SimulationOnly, &["plugins0"], async |mut ctx| {
        ctx.symposium(&["init", "--hook-scope", "global", "--add-agent", "claude"])
            .await?;
        ctx.symposium(&["init", "--hook-scope", "global", "--add-agent", "gemini"])
            .await?;

        let config = symposium::config::Symposium::from_dir(ctx.sym.config_dir());
        let agent_names: Vec<_> = config
            .config
            .agents
            .iter()
            .map(|a| a.name.as_str())
            .collect();
        assert_eq!(agent_names, vec!["claude", "gemini"]);

        let claude_settings = ctx.sym.home_dir().join(".claude/settings.json");
        let gemini_settings = ctx.sym.home_dir().join(".gemini/settings.json");
        assert!(
            std::fs::read_to_string(&claude_settings)
                .unwrap()
                .contains("cargo-agents hook")
        );
        assert!(
            std::fs::read_to_string(&gemini_settings)
                .unwrap()
                .contains("cargo-agents hook")
        );
        Ok(())
    })
    .await
    .unwrap();
}

/// `sync` filters MCP servers by their `crates` predicates.
#[tokio::test]
async fn sync_filters_mcp_servers_by_crates() {
    with_fixture(
        TestMode::SimulationOnly,
        &["mcp-filtering0", "workspace0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.as_ref().unwrap();
            let settings_path = workspace_root.join(".claude/settings.json");
            let settings = std::fs::read_to_string(&settings_path)?;

            // always-server (crates = ["*"]) → registered
            assert!(
                settings.contains("always-server"),
                "wildcard MCP server should be registered"
            );
            // serde-server (crates = ["serde"]) → registered (serde is in workspace0)
            assert!(
                settings.contains("serde-server"),
                "serde MCP server should be registered"
            );
            // inherited-server (no crates, inherits from plugin) → registered
            assert!(
                settings.contains("inherited-server"),
                "inherited MCP server should be registered"
            );
            // missing-crate-server (crates = ["reqwest"]) → NOT registered
            assert!(
                !settings.contains("missing-crate-server"),
                "reqwest MCP server should NOT be registered"
            );

            Ok(())
        },
    )
    .await
    .unwrap();
}

/// `sync` does not install skills targeting transitive dependencies.
/// workspace0 has tokio as a direct dep; mio is a transitive dep of tokio.
#[tokio::test]
async fn sync_excludes_transitive_deps() {
    with_fixture(
        TestMode::SimulationOnly,
        &["transitive-dep0", "workspace0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.as_ref().unwrap();

            // mio-guidance should NOT be installed (mio is transitive, not direct)
            let mio_skill = workspace_root.join(".claude/skills/mio-guidance/SKILL.md");
            assert!(
                !mio_skill.exists(),
                "skill targeting transitive dep (mio) should NOT be installed"
            );

            Ok(())
        },
    )
    .await
    .unwrap();
}

/// `sync` installs skills defined inside a plugin's `[[skills]]` group
/// with `source.path = "."`. The skill directory should resolve relative
/// to the plugin's parent directory, not the TOML file path itself.
#[tokio::test]
async fn sync_installs_plugin_skill_group() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-skill-group0", "workspace0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.as_ref().unwrap();

            let skill_file = workspace_root.join(".claude/skills/serde-guidance/SKILL.md");
            assert!(
                skill_file.exists(),
                "sync should install skill from plugin [[skills]] group with source.path"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// `sync` installs skills from a plugin with `crates = ["*"]`.
/// Wildcard predicates should match any workspace.
#[tokio::test]
async fn sync_installs_wildcard_plugin_skill() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-skill-group0", "workspace0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.as_ref().unwrap();

            let skill_file = workspace_root.join(".claude/skills/wildcard-guidance/SKILL.md");
            assert!(
                skill_file.exists(),
                "sync should install skill from wildcard plugin"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// `sync` installs skills from a crate source via `source.crate_path`.
///
/// Fixture layout:
/// - `crate-y` depends on `crate-x` and `crate-z` (path deps)
/// - `crate-x` ships `skills/x-guidance/SKILL.md` (default path via `source = "crate"`)
/// - `crate-z` ships `.symposium/skills/z-guidance/SKILL.md` (custom path)
#[tokio::test]
async fn sync_installs_skill_from_crate_path() {
    with_fixture(
        TestMode::SimulationOnly,
        &["crate-path0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.as_ref().unwrap();

            // crate-x: default path via source = "crate"
            let skill_file = workspace_root.join(".claude/skills/x-guidance/SKILL.md");
            assert!(
                skill_file.exists(),
                "sync should install skill from crate-x via source = \"crate\""
            );
            let content = std::fs::read_to_string(&skill_file)?;
            assert!(content.contains("Use crate-x like this"));

            // crate-z: custom path via source.crate_path = ".symposium/skills"
            let skill_file = workspace_root.join(".claude/skills/z-guidance/SKILL.md");
            assert!(
                skill_file.exists(),
                "sync should install skill from crate-z via custom crate_path"
            );
            let content = std::fs::read_to_string(&skill_file)?;
            assert!(content.contains("Use crate-z like this"));

            assert!(
                workspace_root
                    .join(".claude/skills/x-guidance/.symposium")
                    .exists()
            );
            assert!(
                workspace_root
                    .join(".claude/skills/z-guidance/.symposium")
                    .exists()
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// `crate-info` resolves a `[patch.crates-io]` crate to its local path.
///
/// Fixture layout:
/// - `patch-demo` depends on `crate-x = "0.1.0"` (crates.io)
/// - `[patch.crates-io]` overrides `crate-x` with a local path
/// - `crate-x` ships `skills/x-patched-guidance/SKILL.md`
#[tokio::test]
async fn crate_info_resolves_patched_crate_to_local_path() {
    with_fixture(TestMode::SimulationOnly, &["patch-crate0"], async |ctx| {
        let cwd = ctx.workspace_root.as_ref().unwrap();
        let result = symposium::crate_command::dispatch_crate(&ctx.sym, "crate-x", None, cwd).await;
        match result {
            symposium::crate_command::DispatchResult::Ok(output) => {
                assert!(output.contains("crate-x"), "should name crate-x: {output}");
                // The resolved path should point inside the fixture's local crate-x dir
                assert!(
                    output.contains("crate-x"),
                    "should resolve to local path: {output}"
                );
                // Should NOT contain "registry" or "crates.io" — it's a local override
                assert!(
                    !output.contains("registry"),
                    "patched crate should not resolve from registry: {output}"
                );
            }
            symposium::crate_command::DispatchResult::Err(e) => {
                panic!("crate-info should succeed for patched crate: {e}");
            }
        }
        Ok(())
    })
    .await
    .unwrap();
}

/// `sync` installs skills from a `[patch.crates-io]`-overridden crate.
#[tokio::test]
async fn sync_installs_skill_from_patched_crate() {
    with_fixture(
        TestMode::SimulationOnly,
        &["patch-crate0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.as_ref().unwrap();

            let skill_file = workspace_root.join(".claude/skills/x-patched-guidance/SKILL.md");
            assert!(
                skill_file.exists(),
                "sync should install skill from patched crate-x"
            );
            let content = std::fs::read_to_string(&skill_file)?;
            assert!(content.contains("Use patched crate-x like this"));
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// `crate-info` resolves a path dependency to its local source directory.
#[tokio::test]
async fn crate_info_resolves_path_dependency() {
    with_fixture(TestMode::SimulationOnly, &["crate-path0"], async |ctx| {
        let cwd = ctx.workspace_root.as_ref().unwrap();

        let result = symposium::crate_command::dispatch_crate(&ctx.sym, "crate-x", None, cwd).await;
        match result {
            symposium::crate_command::DispatchResult::Ok(output) => {
                assert!(output.contains("crate-x"), "should name crate-x: {output}");
                assert!(
                    !output.contains("registry"),
                    "path dep should not resolve from registry: {output}"
                );
                // The source path should point to the local crate-x directory
                assert!(
                    output.contains("crate-x"),
                    "should resolve to local crate-x path: {output}"
                );
            }
            symposium::crate_command::DispatchResult::Err(e) => {
                panic!("crate-info should succeed for path dependency: {e}");
            }
        }
        Ok(())
    })
    .await
    .unwrap();
}

/// Installing default skills in a freshly-initialized git repo must not leak
/// symposium artifacts into `git status`. The skill directories symposium
/// creates carry a wildcard `.gitignore` that hides everything they contain,
/// so `git status` should be clean after sync.
#[tokio::test]
async fn sync_installations_are_gitignored() {
    use std::process::Command;

    with_fixture(
        TestMode::SimulationOnly,
        &["plugins0", "workspace0"],
        async |mut ctx| {
            let workspace_root = ctx.workspace_root.clone().unwrap();

            // Helper: run a git command in the workspace root, bail on failure.
            // `-c core.excludesFile=/dev/null` makes the test independent of
            // the developer's global gitignore (e.g. one that hides `.claude/`),
            // so behavior matches CI.
            let git = |args: &[&str]| -> anyhow::Result<String> {
                let mut full_args = vec!["-c", "core.excludesFile=/dev/null"];
                full_args.extend_from_slice(args);
                let out = Command::new("git")
                    .args(&full_args)
                    .current_dir(&workspace_root)
                    .output()?;
                if !out.status.success() {
                    anyhow::bail!(
                        "git {args:?} failed: {}",
                        String::from_utf8_lossy(&out.stderr)
                    );
                }
                Ok(String::from_utf8(out.stdout)?)
            };

            // Fresh git repo with the fixture's project files committed.
            git(&["init", "--quiet", "--initial-branch=main"])?;
            git(&["config", "user.email", "test@example.com"])?;
            git(&["config", "user.name", "Test"])?;
            git(&["config", "commit.gpgsign", "false"])?;

            // Keep the snapshot focused on symposium-managed paths by
            // excluding test-harness infrastructure (`dot-symposium/` is
            // where the fixture plants the user-level `~/.symposium/`) and
            // `cargo metadata`'s generated `Cargo.lock`.
            std::fs::write(
                workspace_root.join(".gitignore"),
                "dot-symposium/\nCargo.lock\n",
            )?;

            git(&["add", "."])?;
            git(&["commit", "--quiet", "-m", "initial"])?;

            // Install default skills for a single agent.
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            // Sanity: the skill and marker are actually on disk.
            assert!(
                workspace_root
                    .join(".claude/skills/serde-guidance/SKILL.md")
                    .exists(),
                "skill should be installed on disk"
            );
            assert!(
                workspace_root
                    .join(".claude/skills/serde-guidance/.symposium")
                    .exists(),
                "marker should be on disk"
            );

            // Use `-uall` so untracked dirs expand to their leaf paths —
            // gives deterministic output regardless of git's collapsing rules.
            let status = git(&["status", "--porcelain", "-uall"])?;

            // Skill dirs are fully gitignored by the wildcard `.gitignore`
            // symposium drops into them, so they don't appear here.
            //
            // `.claude/settings.json` does appear: the `plugins0` fixture
            // sets `hook-scope = "project"`, so init+sync register hooks
            // into the workspace's `.claude/settings.json` rather than the
            // user's home dir. That file is the user's to commit (or not);
            // symposium doesn't gitignore it.
            expect_test::expect![[r#"
                ?? .claude/settings.json
            "#]]
            .assert_eq(&status);

            Ok(())
        },
    )
    .await
    .unwrap();
}
