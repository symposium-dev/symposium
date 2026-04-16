//! Proof-of-concept dual-mode agent integration test.
//!
//! Runs in simulation mode by default (CI). Set `SYMPOSIUM_TEST_AGENT=claude`
//! to run against a real agent via the Claude Agent SDK.
//!
//! # Running
//!
//! ```bash
//! # Simulation mode (default — no agent needed)
//! cargo test --test hook_agent
//!
//! # Agent mode (requires uv + ANTHROPIC_API_KEY)
//! SYMPOSIUM_TEST_AGENT=claude cargo test --test hook_agent
//! ```

use symposium::hook_schema::HookAgent;
use symposium_testlib::{HookStep, with_fixture};

/// SessionStart should return plugin-provided context.
#[tokio::test]
async fn session_start_returns_plugin_context() {
    let ctx = with_fixture(&["plugins0"]);

    let result = ctx
        .submit("Say hello", &[HookStep::session_start()], HookAgent::Claude)
        .await
        .unwrap();

    assert_eq!(result.hooks.len(), 1);
    assert!(
        result.has_context_containing("symposium start"),
        "expected session-start-context from plugin, got: {:?}",
        result.hooks[0].output
    );
}
