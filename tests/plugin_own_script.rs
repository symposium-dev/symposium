//! A plugin's own hook scripts, named relative to its manifest.

use serde_json::json;
use symposium::hook_schema::HookAgent;
use symposium_testlib::{HookStep, TestMode, with_fixture};

/// The reference says these paths are relative to the manifest, but each is looked
/// up from where the hook runs, so neither script runs.
#[tokio::test(flavor = "multi_thread")]
async fn scripts_named_relative_to_the_manifest_do_not_run() {
    with_fixture(
        TestMode::SimulationOnly,
        &["plugin-own-script0"],
        async |mut ctx| {
            let result = ctx
                .prompt_or_hook(
                    "ignored",
                    &[
                        HookStep::PreToolUse {
                            tool_name: "Read".to_string(),
                            tool_input: json!({"file_path": "/tmp/x"}),
                        },
                        HookStep::PreToolUse {
                            tool_name: "Bash".to_string(),
                            tool_input: json!({"command": "true"}),
                        },
                    ],
                    HookAgent::Claude,
                )
                .await?;

            for marker in ["own-script-fired", "beside-script-fired"] {
                assert!(
                    !result.has_context_containing(marker),
                    "expected the plugin's own script not to run yet, got: {result:#?}",
                );
            }
            Ok(())
        },
    )
    .await
    .unwrap();
}
