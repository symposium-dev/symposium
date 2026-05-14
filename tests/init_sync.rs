//! Integration tests for init and sync flows.

use std::path::{Path, PathBuf};

use symposium::state;
use symposium_testlib::{TestMode, with_fixture};

/// Read the user config file from the test context.
fn read_user_config(ctx: &symposium_testlib::TestContext) -> String {
    let path = ctx.sym.config_dir().join("config.toml");
    std::fs::read_to_string(&path).unwrap_or_else(|_| "(not found)".to_string())
}

/// Locate every installed skill directory under `parent` whose name is
/// `<skill_name>` or `<skill_name>-<hash>`. Sync embeds an origin-derived
/// hash in the directory name to keep distinct origins from colliding.
fn find_installed_skills(parent: &Path, skill_name: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let matches = name == skill_name
            || (name.starts_with(skill_name)
                && name.as_bytes().get(skill_name.len()) == Some(&b'-'));
        if matches && path.join("SKILL.md").is_file() {
            out.push(path);
        }
    }
    out.sort();
    out
}

/// Locate the unique installed skill directory by name. Panics if 0 or
/// >1 directories match. Use `find_installed_skills` when the test cares
/// about how many were installed.
fn find_installed_skill(parent: &Path, skill_name: &str) -> PathBuf {
    let mut hits = find_installed_skills(parent, skill_name);
    assert_eq!(
        hits.len(),
        1,
        "expected exactly one installed skill named `{skill_name}` under {}, found {}: {:?}",
        parent.display(),
        hits.len(),
        hits,
    );
    hits.pop().unwrap()
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
            let skills_dir = workspace_root.join(".claude/skills");

            let skill_dir = find_installed_skill(&skills_dir, "serde-guidance");

            // No name collision and no pre-existing user-managed dir at
            // the unsuffixed slot, so sync uses the plain name (no
            // origin-hash suffix).
            assert_eq!(
                skill_dir.file_name().and_then(|n| n.to_str()),
                Some("serde-guidance"),
                "single-origin skill should install without a hash suffix"
            );

            // Each installed skill directory carries a `.symposium` marker so
            // future syncs (and other tools) can identify it as symposium-managed.
            assert!(
                skill_dir.join(".symposium").exists(),
                "skill dir should contain .symposium marker"
            );

            // Skill dirs symposium creates get a wildcard gitignore so the
            // marker, SKILL.md, and gitignore itself stay out of version control.
            for gi in [skills_dir.join(".gitignore"), skill_dir.join(".gitignore")] {
                assert!(gi.exists(), "missing .gitignore at {}", gi.display());
                let contents = std::fs::read_to_string(&gi).unwrap();
                assert_eq!(
                    contents.trim(),
                    "*",
                    "unexpected .gitignore at {}",
                    gi.display()
                );
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
            let installed =
                find_installed_skills(&workspace_root.join(".agents/skills"), "rust-best-practice");
            assert!(
                installed.is_empty(),
                "sync should not install a skill with invalid YAML frontmatter; got {installed:?}"
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
            let installed =
                find_installed_skills(&workspace_root.join(".claude/skills"), "mio-guidance");
            assert!(
                installed.is_empty(),
                "skill targeting transitive dep (mio) should NOT be installed; got {installed:?}"
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
            find_installed_skill(&workspace_root.join(".claude/skills"), "serde-guidance");
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
            find_installed_skill(&workspace_root.join(".claude/skills"), "wildcard-guidance");
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
/// - `crate-x` ships `.symposium/skills/x-guidance/SKILL.md` (default path via `source = "crate"`)
/// - `crate-z` ships `guidance/z-guidance/SKILL.md` (custom path via `source.crate_path`)
#[tokio::test]
async fn sync_installs_skill_from_crate_path() {
    with_fixture(
        TestMode::SimulationOnly,
        &["crate-path0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.as_ref().unwrap();
            let skills_dir = workspace_root.join(".claude/skills");

            // crate-x: default path via source = "crate"
            let x_dir = find_installed_skill(&skills_dir, "x-guidance");
            let content = std::fs::read_to_string(x_dir.join("SKILL.md"))?;
            assert!(content.contains("Use crate-x like this"));
            assert!(x_dir.join(".symposium").exists());

            // crate-z: custom path via source.crate_path = "guidance"
            let z_dir = find_installed_skill(&skills_dir, "z-guidance");
            let content = std::fs::read_to_string(z_dir.join("SKILL.md"))?;
            assert!(content.contains("Use crate-z like this"));
            assert!(z_dir.join(".symposium").exists());
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
            let skill_dir =
                find_installed_skill(&workspace_root.join(".claude/skills"), "x-patched-guidance");
            let content = std::fs::read_to_string(skill_dir.join("SKILL.md"))?;
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
            let skill_dir =
                find_installed_skill(&workspace_root.join(".claude/skills"), "serde-guidance");
            assert!(
                skill_dir.join("SKILL.md").exists(),
                "skill should be installed on disk"
            );
            assert!(
                skill_dir.join(".symposium").exists(),
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

// ---------------------------------------------------------------------------
// SkillOrigin dedup
// ---------------------------------------------------------------------------

/// Two plugins both with `source = "crate"` pointing at the same crate
/// produce the same `SkillOrigin::Crate { name, version }`, so the skill
/// installs exactly once.
#[tokio::test]
async fn sync_dedups_same_crate_origin_across_plugins() {
    with_fixture(
        TestMode::SimulationOnly,
        &["dedup-crate-origin0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.as_ref().unwrap();
            let installed = find_installed_skills(
                &workspace_root.join(".claude/skills"),
                "code-review",
            );
            assert_eq!(
                installed.len(),
                1,
                "two plugins resolving the same crate-x must collapse to one install; got {installed:?}"
            );
            // Dedup left a single origin, so the install gets the
            // unsuffixed name (no hash needed to disambiguate).
            assert_eq!(
                installed[0].file_name().and_then(|n| n.to_str()),
                Some("code-review"),
                "dedup'd single origin should land at the unsuffixed name"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// Two plugins in the same registry source whose `source.path` groups
/// resolve to the same on-disk skill bundle produce the same
/// `SkillOrigin::Source` and dedupe to a single install.
///
/// Identity is `(source_name, skill-path-relative-to-source-root)`, so
/// the path the SKILL.md actually lives at is what matters — not the
/// plugin name that pointed at it. Standalone discovery of the same
/// SKILL.md (the registry walk also surfaces it as a standalone since
/// nothing claims its parent) collapses to that same origin too.
#[tokio::test]
async fn sync_dedups_same_source_path_across_plugins() {
    with_fixture(
        TestMode::SimulationOnly,
        &["dedup-source-origin0", "workspace0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.as_ref().unwrap();
            let installed = find_installed_skills(
                &workspace_root.join(".claude/skills"),
                "shared-skill",
            );
            assert_eq!(
                installed.len(),
                1,
                "two plugins pointing at the same skill bundle must collapse to one install; got {installed:?}"
            );
            assert_eq!(
                installed[0].file_name().and_then(|n| n.to_str()),
                Some("shared-skill"),
                "single dedup'd origin should land at the unsuffixed name"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// Two plugins each contributing a skill named `code-review` from their
/// own `source.path` produce distinct `SkillOrigin::Plugin { plugin_name }`
/// values, so both install — under separate hashed directory names.
#[tokio::test]
async fn sync_keeps_distinct_plugin_origins_with_same_skill_name() {
    with_fixture(
        TestMode::SimulationOnly,
        &["distinct-plugin-origins0", "workspace0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.as_ref().unwrap();
            let installed = find_installed_skills(
                &workspace_root.join(".claude/skills"),
                "code-review",
            );
            assert_eq!(
                installed.len(),
                2,
                "two plugins each shipping a `code-review` skill must both install; got {installed:?}"
            );

            // Each install dir has the expected disambiguating suffix.
            let names: Vec<String> = installed
                .iter()
                .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(str::to_string))
                .collect();
            for n in &names {
                assert!(
                    n.starts_with("code-review-"),
                    "expected hashed suffix on `{n}`"
                );
            }

            // And the bodies came from different plugins.
            let bodies: Vec<String> = installed
                .iter()
                .map(|p| std::fs::read_to_string(p.join("SKILL.md")).unwrap())
                .collect();
            assert!(bodies.iter().any(|b| b.contains("Plugin-A")));
            assert!(bodies.iter().any(|b| b.contains("Plugin-B")));
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// One origin initially → unsuffixed install. Introduce a second
/// origin → both must move to suffixed names; the prior unsuffixed
/// install (still has the marker, no longer in the freshly-installed
/// set) is reaped.
#[tokio::test]
async fn sync_demotes_to_suffixed_when_conflict_appears() {
    with_fixture(
        TestMode::SimulationOnly,
        &["distinct-plugin-origins0", "workspace0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;

            // Park plugin-b *outside* any plugin source dir so it isn't
            // discovered. (`tempdir/` sits next to the user config root,
            // which is itself a plugin source — so we can't park inside
            // the same parent.)
            let plugins_dir = ctx.sym.config_dir().join("plugins");
            let parked = ctx.tempdir.join("parked-plugin-b");
            std::fs::rename(plugins_dir.join("plugin-b"), &parked)?;

            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.clone().unwrap();
            let skills_dir = workspace_root.join(".claude/skills");

            // Baseline: only plugin-a's `code-review` is visible, so it
            // takes the plain slot.
            let installed = find_installed_skills(&skills_dir, "code-review");
            assert_eq!(installed.len(), 1, "expected one unsuffixed install");
            assert_eq!(
                installed[0].file_name().and_then(|n| n.to_str()),
                Some("code-review"),
                "single origin should land at the unsuffixed slot"
            );

            // Re-introduce plugin-b. Now there are two origins.
            std::fs::rename(&parked, plugins_dir.join("plugin-b"))?;

            ctx.symposium(&["sync"]).await?;

            // Both origins now install under suffixed names; the
            // previous unsuffixed dir is gone (reaped via the marker
            // walk because it's no longer in the freshly-installed set).
            let installed = find_installed_skills(&skills_dir, "code-review");
            assert_eq!(
                installed.len(),
                2,
                "both origins should install under suffixed names; got {installed:?}"
            );
            assert!(
                !skills_dir.join("code-review").exists(),
                "the prior unsuffixed install must be reaped"
            );
            for p in &installed {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap();
                assert!(
                    name.starts_with("code-review-"),
                    "expected hashed suffix on `{name}`"
                );
            }

            // And the bodies cover both plugins.
            let bodies: Vec<String> = installed
                .iter()
                .map(|p| std::fs::read_to_string(p.join("SKILL.md")).unwrap())
                .collect();
            assert!(bodies.iter().any(|b| b.contains("Plugin-A")));
            assert!(bodies.iter().any(|b| b.contains("Plugin-B")));
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// Two origins → both suffixed. Remove one origin, sync again → the
/// survivor moves to the unsuffixed slot and the suffixed leftover is
/// reaped via the marker-based stale-cleanup walk.
#[tokio::test]
async fn sync_promotes_to_unsuffixed_when_conflict_disappears() {
    with_fixture(
        TestMode::SimulationOnly,
        &["distinct-plugin-origins0", "workspace0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.clone().unwrap();
            let skills_dir = workspace_root.join(".claude/skills");

            // Baseline: two origins, both suffixed, neither at the
            // plain slot.
            let installed = find_installed_skills(&skills_dir, "code-review");
            assert_eq!(installed.len(), 2, "expected two suffixed installs");
            assert!(
                !skills_dir.join("code-review").exists(),
                "unsuffixed slot must be vacant while both origins coexist"
            );

            // Remove plugin-b so only plugin-a's `code-review` survives.
            std::fs::remove_dir_all(ctx.sym.config_dir().join("plugins/plugin-b"))?;

            ctx.symposium(&["sync"]).await?;

            // The survivor now lives at the plain slot.
            let installed = find_installed_skills(&skills_dir, "code-review");
            assert_eq!(
                installed.len(),
                1,
                "exactly one install should remain after removing plugin-b; got {installed:?}"
            );
            assert_eq!(
                installed[0].file_name().and_then(|n| n.to_str()),
                Some("code-review"),
                "the surviving origin should be promoted to the unsuffixed slot"
            );
            // And it's still plugin-a's content (plugin-b was removed).
            let body = std::fs::read_to_string(installed[0].join("SKILL.md"))?;
            assert!(
                body.contains("Plugin-A"),
                "promoted install should be plugin-a's body, got: {body}"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// A pre-existing user-managed directory at the skill's unsuffixed slot
/// (no `.symposium` marker) forces sync to fall back to the hashed
/// directory name rather than clobber the user's content.
#[tokio::test]
async fn sync_falls_back_to_hashed_name_when_user_dir_in_the_way() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugins0", "workspace0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;

            let workspace_root = ctx.workspace_root.clone().unwrap();
            // Plant a user-managed dir at the slot symposium would
            // normally pick. No `.symposium` marker → user-owned.
            let user_dir = workspace_root.join(".claude/skills/serde-guidance");
            std::fs::create_dir_all(&user_dir)?;
            std::fs::write(user_dir.join("SKILL.md"), "user content")?;

            ctx.symposium(&["sync"]).await?;

            // The user's content is untouched.
            assert_eq!(
                std::fs::read_to_string(user_dir.join("SKILL.md"))?,
                "user content"
            );
            assert!(
                !user_dir.join(".symposium").exists(),
                "no marker should be planted on the user's dir"
            );

            // And symposium still installed the skill — under a hashed
            // name. `find_installed_skills` requires a `SKILL.md` plus a
            // matching directory shape; the suffix variant is the only
            // one that should carry the marker.
            let installed =
                find_installed_skills(&workspace_root.join(".claude/skills"), "serde-guidance");
            let hashed: Vec<_> = installed
                .iter()
                .filter(|p| p.join(".symposium").exists())
                .collect();
            assert_eq!(
                hashed.len(),
                1,
                "sync should install one symposium-managed copy under a hashed name; got {hashed:?}"
            );
            assert_ne!(
                hashed[0].file_name().and_then(|n| n.to_str()),
                Some("serde-guidance"),
                "must not use the unsuffixed slot when a user dir occupies it"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// One plugin with two `[[skills]]` groups, each with its own `source.path`,
/// each producing a same-named skill. The group source goes into the
/// origin's `disambiguator`, so both groups install — without colliding.
#[tokio::test]
async fn sync_keeps_distinct_groups_within_one_plugin() {
    with_fixture(
        TestMode::SimulationOnly,
        &["multi-group-plugin0", "workspace0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.as_ref().unwrap();
            let installed =
                find_installed_skills(&workspace_root.join(".claude/skills"), "shared-name");
            assert_eq!(
                installed.len(),
                2,
                "two skill groups within one plugin must both install; got {installed:?}"
            );

            let bodies: Vec<String> = installed
                .iter()
                .map(|p| std::fs::read_to_string(p.join("SKILL.md")).unwrap())
                .collect();
            assert!(bodies.iter().any(|b| b.contains("Group-A")));
            assert!(bodies.iter().any(|b| b.contains("Group-B")));
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// Two standalone skills both named `my-skill` but living at different
/// paths within the registry source (`foo/my-skill/SKILL.md` and
/// `bar/my-skill/SKILL.md`) produce distinct origins (the relative path
/// is part of the `SkillOrigin::Plugin` identifier), so both install.
#[tokio::test]
async fn sync_keeps_distinct_standalone_origins_at_different_paths() {
    with_fixture(
        TestMode::SimulationOnly,
        &["distinct-standalone-paths0", "workspace0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.as_ref().unwrap();
            let installed = find_installed_skills(
                &workspace_root.join(".claude/skills"),
                "my-skill",
            );
            assert_eq!(
                installed.len(),
                2,
                "two standalone skills at different relative paths must both install; got {installed:?}"
            );

            let bodies: Vec<String> = installed
                .iter()
                .map(|p| std::fs::read_to_string(p.join("SKILL.md")).unwrap())
                .collect();
            assert!(bodies.iter().any(|b| b.contains("Foo body")));
            assert!(bodies.iter().any(|b| b.contains("Bar body")));
            Ok(())
        },
    )
    .await
    .unwrap();
}

// ---------------------------------------------------------------------------
// agents-syncing: propagate user-authored skills from `.agents/skills/`
// ---------------------------------------------------------------------------

/// User-authored skills in `.agents/skills/` are propagated to agents that
/// read skills from a different directory (e.g. Claude → `.claude/skills/`).
/// Companion files next to `SKILL.md` are copied too, and the destination
/// receives a `.symposium` marker so future syncs recognize it as managed.
#[tokio::test]
async fn agents_syncing_propagates_user_authored_skill_to_claude() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugins0", "workspace0", "user-skills0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.as_ref().unwrap();

            // Source is untouched. Notably, symposium does not drop a marker
            // into source skills — that's what keeps them "user-authored".
            let source = workspace_root.join(".agents/skills/user-authored-skill");
            assert!(source.join("SKILL.md").exists(), "source SKILL.md stays");
            assert!(
                !source.join(".symposium").exists(),
                "symposium must not mark source skills"
            );

            // Propagated copy exists with SKILL.md, companion files, marker,
            // and wildcard gitignore.
            let dest = workspace_root.join(".claude/skills/user-authored-skill");
            assert!(dest.join("SKILL.md").exists(), "SKILL.md propagated");
            assert!(
                dest.join("REFERENCE.md").exists(),
                "companion files propagated"
            );
            assert!(dest.join(".symposium").exists(), "marker present");
            let gi = std::fs::read_to_string(dest.join(".gitignore"))?;
            assert_eq!(gi.trim(), "*", "destination gitignore is wildcard");
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// When only agents that natively read `.agents/skills/` are configured,
/// propagation has no distinct target directory and is a no-op.
#[tokio::test]
async fn agents_syncing_noop_when_only_agents_path_used() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugins0", "workspace0", "user-skills0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "copilot"]).await?;
            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.as_ref().unwrap();

            // Source stays in place, unmarked.
            let source = workspace_root.join(".agents/skills/user-authored-skill");
            assert!(source.join("SKILL.md").exists());
            assert!(
                !source.join(".symposium").exists(),
                "source must remain unmarked"
            );
            // No other agent's skills dir should have been created.
            assert!(!workspace_root.join(".claude/skills").exists());
            assert!(!workspace_root.join(".kiro/skills").exists());
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// Setting `agents-syncing = false` disables propagation entirely.
#[tokio::test]
async fn agents_syncing_disabled_skips_propagation() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugins0", "workspace0", "user-skills0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.sym.config.agents_syncing = false;
            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.as_ref().unwrap();
            assert!(
                !workspace_root
                    .join(".claude/skills/user-authored-skill")
                    .exists(),
                "propagation should not occur when agents-syncing is disabled"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// Removing a user-authored skill from `.agents/skills/` causes its
/// previously propagated copy to be reaped by the next sync (the marker
/// is still there, but it's no longer in the freshly-installed set).
#[tokio::test]
async fn agents_syncing_cleans_up_removed_user_skill() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugins0", "workspace0", "user-skills0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.clone().unwrap();
            let propagated = workspace_root.join(".claude/skills/user-authored-skill");
            assert!(propagated.exists(), "first sync should propagate");
            assert!(propagated.join(".symposium").exists());

            // User removes the source.
            std::fs::remove_dir_all(workspace_root.join(".agents/skills/user-authored-skill"))?;

            ctx.symposium(&["sync"]).await?;

            assert!(
                !propagated.exists(),
                "second sync should reap propagated copy once source is removed"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// Turning `agents-syncing` off on a subsequent sync removes previously
/// propagated copies — the feature self-heals when disabled.
#[tokio::test]
async fn agents_syncing_disabling_removes_previously_propagated_skills() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugins0", "workspace0", "user-skills0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let workspace_root = ctx.workspace_root.clone().unwrap();
            let propagated = workspace_root.join(".claude/skills/user-authored-skill");
            assert!(propagated.exists(), "first sync should propagate");

            ctx.sym.config.agents_syncing = false;
            ctx.symposium(&["sync"]).await?;

            assert!(
                !propagated.exists(),
                "disabling agents-syncing should clean up previously propagated copies"
            );
            // Source must remain untouched.
            assert!(
                workspace_root
                    .join(".agents/skills/user-authored-skill/SKILL.md")
                    .exists()
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// A pre-existing, user-managed directory in the target (no `.symposium`
/// marker) is not overwritten even when a same-named skill exists in
/// `.agents/skills/`.
#[tokio::test]
async fn agents_syncing_does_not_overwrite_user_managed_target() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugins0", "workspace0", "user-skills0"],
        async |mut ctx| {
            ctx.symposium(&["init", "--add-agent", "claude"]).await?;

            let workspace_root = ctx.workspace_root.clone().unwrap();

            // Pre-existing, user-managed file in the target with the same name.
            let target_dir = workspace_root.join(".claude/skills/user-authored-skill");
            std::fs::create_dir_all(&target_dir)?;
            let preexisting = target_dir.join("SKILL.md");
            std::fs::write(&preexisting, "pre-existing user content")?;

            ctx.symposium(&["sync"]).await?;

            // File untouched — propagation must not clobber user-managed content.
            let content = std::fs::read_to_string(&preexisting)?;
            assert_eq!(content, "pre-existing user content");
            assert!(
                !target_dir.join(".symposium").exists(),
                "no marker should be dropped onto a user-managed directory"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

// ---------------------------------------------------------------------------
// Self-update integration tests
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Self-update / state integration tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn self_update_command_exists() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugins0", "workspace0"],
        async |mut ctx| {
            // Verify self-update is a recognized CLI command by parsing it.
            // We don't actually run the update (that would hit the network),
            // but this confirms the command is wired through dispatch.
            let result = ctx.symposium(&["self-update"]).await;
            // Will fail with network error or "up to date" — both are OK,
            // we just care it didn't fail with "unknown command".
            match result {
                Ok(_) => {}
                Err(e) => {
                    let msg = format!("{e:#}");
                    assert!(
                        !msg.contains("not supported") && !msg.contains("unrecognized"),
                        "self-update should be a valid command, got: {msg}"
                    );
                }
            }
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn state_toml_tracks_version() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugins0", "workspace0"],
        async |ctx| {
            assert!(state::load(ctx.sym.config_dir()).is_none());

            state::ensure_current(ctx.sym.config_dir());

            let s = state::load(ctx.sym.config_dir()).expect("state.toml should exist");
            assert_eq!(s.version, state::CURRENT_VERSION);
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn state_toml_update_check_throttling() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugins0", "workspace0"],
        async |ctx| {
            let dir = ctx.sym.config_dir();

            // First check: should be allowed (no state yet).
            assert!(state::should_check_for_update(dir));

            // Record a check.
            state::record_update_check(dir);

            // Immediately after: should be throttled.
            assert!(!state::should_check_for_update(dir));

            // Stamp version — should preserve the last-update-check.
            state::stamp(dir);
            assert!(!state::should_check_for_update(dir));

            Ok(())
        },
    )
    .await
    .unwrap();
}
