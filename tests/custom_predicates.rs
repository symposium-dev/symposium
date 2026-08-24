//! Integration tests for custom predicate extensions.

use std::path::Path;

use symposium_testlib::{TestMode, with_fixture};

/// Write a shell script to the given path and make it executable.
fn write_script(path: &Path, content: &str) {
    std::fs::write(path, format!("#!/bin/sh\n{content}")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// Did `skill` reach Claude Code for this workspace?
///
/// Claude takes a compiled plugin directory, so a skill it can see is the one
/// inside `.symposium/plugins/<plugin>/skills/`. The older per-skill location is
/// still checked, because an agent that cannot take a plugin directory keeps
/// receiving skills that way and these tests are about predicates, not delivery.
fn skill_reached_claude(workspace_root: &Path, skill: &str) -> bool {
    let compiled = workspace_root.join(".symposium").join("plugins");
    let in_a_plugin = std::fs::read_dir(&compiled).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry
                .path()
                .join("skills")
                .join(skill)
                .join("SKILL.md")
                .is_file()
        })
    });
    in_a_plugin
        || workspace_root
            .join(".claude")
            .join("skills")
            .join(skill)
            .join("SKILL.md")
            .is_file()
}

/// Every place a skill could have landed, for assertion messages and for the
/// "nothing was installed" checks.
fn delivered_skills(workspace_root: &Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let compiled = workspace_root.join(".symposium").join("plugins");
    if let Ok(entries) = std::fs::read_dir(&compiled) {
        for entry in entries.flatten() {
            if let Ok(skills) = std::fs::read_dir(entry.path().join("skills")) {
                found.extend(skills.flatten().map(|s| s.path()));
            }
        }
    }
    if let Ok(entries) = std::fs::read_dir(workspace_root.join(".claude").join("skills")) {
        found.extend(
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.join("SKILL.md").is_file()),
        );
    }
    found.sort();
    found
}

/// `sync` installs a skill when the custom predicate passes (exit 0).
#[tokio::test]
async fn sync_custom_predicate_installs_skill() {
    with_fixture(
        TestMode::SimulationOnly,
        &["custom-predicate0"],
        async |mut ctx| {
            let script_path = ctx.tempdir.join("bp-checker.sh");
            write_script(&script_path, "exit 0");

            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let root = ctx.workspace_root.as_ref().unwrap();
            assert!(
                skill_reached_claude(root, "bp-skill"),
                "skill should be delivered when the predicate passes; found {:?}",
                delivered_skills(root),
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// `sync` skips a skill when the custom predicate fails (exit non-zero).
#[tokio::test]
async fn sync_custom_predicate_fails_skips_skill() {
    with_fixture(
        TestMode::SimulationOnly,
        &["custom-predicate0"],
        async |mut ctx| {
            let script_path = ctx.tempdir.join("bp-checker.sh");
            write_script(&script_path, "exit 1");

            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let root = ctx.workspace_root.as_ref().unwrap();
            let entries = delivered_skills(root);
            assert!(
                entries.is_empty(),
                "no skills should be installed when predicate fails; got: {:?}",
                entries,
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// The raw argument from the predicate expression is passed to the script.
/// `predicates = ["battery_pack(cli)"]` should pass "cli" as the last arg.
#[tokio::test]
async fn sync_custom_predicate_receives_correct_argument() {
    with_fixture(
        TestMode::SimulationOnly,
        &["custom-predicate0"],
        async |mut ctx| {
            let script_path = ctx.tempdir.join("bp-checker.sh");
            // Only pass if the argument is exactly "cli"
            write_script(
                &script_path,
                r#"if [ "$1" = "cli" ]; then exit 0; else exit 1; fi"#,
            );

            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let root = ctx.workspace_root.as_ref().unwrap();
            assert!(
                skill_reached_claude(root, "bp-skill"),
                "skill should be delivered when the argument matches 'cli'; found {:?}",
                delivered_skills(root),
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// When the argument doesn't match, the predicate fails.
#[tokio::test]
async fn sync_custom_predicate_wrong_argument_fails() {
    with_fixture(
        TestMode::SimulationOnly,
        &["custom-predicate0"],
        async |mut ctx| {
            let script_path = ctx.tempdir.join("bp-checker.sh");
            // Only pass if the argument is "web" — but the fixture uses "cli"
            write_script(
                &script_path,
                r#"if [ "$1" = "web" ]; then exit 0; else exit 1; fi"#,
            );

            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let root = ctx.workspace_root.as_ref().unwrap();
            let entries = delivered_skills(root);
            assert!(
                entries.is_empty(),
                "skill should NOT be installed when argument doesn't match"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// A predicate defined by one plugin can be used by a different plugin.
/// The provider-plugin defines `my_check`; the consumer-plugin uses it.
#[tokio::test]
async fn sync_custom_predicate_cross_plugin() {
    with_fixture(
        TestMode::SimulationOnly,
        &["custom-predicate-cross0"],
        async |mut ctx| {
            let script_path = ctx.tempdir.join("cross-checker.sh");
            write_script(&script_path, "exit 0");

            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let root = ctx.workspace_root.as_ref().unwrap();
            let skills_dir = root.join(".claude").join("skills");
            assert!(
                skill_reached_claude(root, "consumer-skill"),
                "consumer plugin skill should install when provider's predicate passes; \
                 skills_dir={}, contents={:?}",
                skills_dir.display(),
                std::fs::read_dir(&skills_dir)
                    .ok()
                    .map(|d| d.flatten().map(|e| e.path()).collect::<Vec<_>>()),
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

/// Cross-plugin predicate: when the provider's predicate fails, the consumer's
/// skill is not installed.
#[tokio::test]
async fn sync_custom_predicate_cross_plugin_fails() {
    with_fixture(
        TestMode::SimulationOnly,
        &["custom-predicate-cross0"],
        async |mut ctx| {
            let script_path = ctx.tempdir.join("cross-checker.sh");
            write_script(&script_path, "exit 1");

            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let root = ctx.workspace_root.as_ref().unwrap();
            let entries = delivered_skills(root);
            assert!(
                entries.is_empty(),
                "consumer skill should NOT install when provider's predicate fails"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}
