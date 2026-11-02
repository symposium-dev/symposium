//! Cargo-package hook scripts resolve against the dependency's own directory.

use serde_json::json;
use symposium::hook_schema::HookAgent;
use symposium_testlib::{HookStep, TestMode, with_fixture};

async fn assert_dependency_script_runs(from_metadata: bool) {
    with_fixture(
        TestMode::SimulationOnly,
        &["crate-facets0"],
        async |mut ctx| {
            ctx.sym.config.auto_sync = false;
            let package_dir = ctx.tempdir.join("crate-f");
            // The dependency must be loaded through Cargo, not rediscovered
            // as a workspace-member plugin.
            let host_manifest = ctx.tempdir.join("Cargo.toml");
            let host = std::fs::read_to_string(&host_manifest)?;
            std::fs::write(
                host_manifest,
                format!("{host}\n[workspace]\nexclude = [\"crate-f\"]\n"),
            )?;

            let hook_table = if from_metadata {
                "package.metadata.symposium.hooks"
            } else {
                "hooks"
            };
            let definition = format!(
                indoc::indoc! {r#"
                    [[{hook_table}]]
                    name = "facet-hook"
                    event = "PreToolUse"
                    matcher = "Task"
                    command = {{ script = "scripts/facet-hook.sh" }}
                    format = "symposium"
                "#},
                hook_table = hook_table,
            );
            if from_metadata {
                std::fs::remove_file(package_dir.join("Symposium.toml"))?;
                let manifest = package_dir.join("Cargo.toml");
                let package = std::fs::read_to_string(&manifest)?;
                std::fs::write(manifest, format!("{package}\n{definition}"))?;
            } else {
                std::fs::write(package_dir.join("Symposium.toml"), definition)?;
            }

            // Wrong bases contain a runnable with the same name, so a path
            // mix-up cannot pass just because a script happened to execute.
            for wrong_base in [
                ctx.tempdir.clone(),
                ctx.sym.config_dir().join("plugins/vouch-f"),
            ] {
                std::fs::create_dir_all(wrong_base.join("scripts"))?;
                std::fs::write(
                    wrong_base.join("scripts/facet-hook.sh"),
                    "#!/bin/sh\necho '{\"PreToolUse\":{\"additionalContext\":\"wrong-script-root\"}}'\n",
                )?;
            }
            let workspace = ctx.sym.workspace(&ctx.tempdir);
            let info = workspace.info().await.expect("fixture has a workspace");
            let package_dir = std::fs::canonicalize(package_dir)?;
            for member in &info.members {
                assert_ne!(
                    std::fs::canonicalize(member)?,
                    package_dir,
                    "the dependency must not be a workspace-member plugin"
                );
            }

            let result = ctx
                .prompt_or_hook(
                    "ignored",
                    &[HookStep::PreToolUse {
                        tool_name: "Task".into(),
                        tool_input: json!({}),
                    }],
                    HookAgent::Claude,
                )
                .await?;
            assert!(
                result.has_context_containing("facet-hook-output"),
                "expected the dependency's own script, got: {result:#?}"
            );
            assert!(
                !result.has_context_containing("wrong-script-root"),
                "ran a script outside the dependency: {result:#?}"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn relative_script_in_dependency_manifest_runs() {
    assert_dependency_script_runs(false).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn relative_script_in_dependency_metadata_runs() {
    assert_dependency_script_runs(true).await;
}
