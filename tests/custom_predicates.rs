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

/// `sync` installs a skill when the custom predicate passes (exit 0).
#[tokio::test]
async fn sync_custom_predicate_installs_skill() {
    with_fixture(
        TestMode::SimulationOnly,
        &["custom-predicate0"],
        async |mut ctx| {
            // Write a passing predicate script
            let script_path = ctx.tempdir.join("bp-checker.sh");
            write_script(&script_path, "exit 0");

            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let skills_dir = ctx
                .workspace_root
                .as_ref()
                .unwrap()
                .join(".claude")
                .join("skills");
            let skill_dir = skills_dir.join("bp-skill");
            assert!(
                skill_dir.join("SKILL.md").exists(),
                "skill should be installed when predicate passes; skills_dir={}, contents={:?}",
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

/// `sync` skips a skill when the custom predicate fails (exit non-zero).
#[tokio::test]
async fn sync_custom_predicate_fails_skips_skill() {
    with_fixture(
        TestMode::SimulationOnly,
        &["custom-predicate0"],
        async |mut ctx| {
            // Write a failing predicate script
            let script_path = ctx.tempdir.join("bp-checker.sh");
            write_script(&script_path, "exit 1");

            ctx.symposium(&["init", "--add-agent", "claude"]).await?;
            ctx.symposium(&["sync"]).await?;

            let skills_dir = ctx
                .workspace_root
                .as_ref()
                .unwrap()
                .join(".claude")
                .join("skills");
            // The skill should NOT be installed
            let entries: Vec<_> = std::fs::read_dir(&skills_dir)
                .ok()
                .map(|d| {
                    d.flatten()
                        .filter(|e| e.path().is_dir())
                        .filter(|e| e.path().join("SKILL.md").exists())
                        .collect()
                })
                .unwrap_or_default();
            assert!(
                entries.is_empty(),
                "no skills should be installed when predicate fails; got: {:?}",
                entries.iter().map(|e| e.path()).collect::<Vec<_>>(),
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}
