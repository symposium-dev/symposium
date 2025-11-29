//! Basic integration tests for the crate sources proxy using ElizACP.

use anyhow::Result;
use expect_test::expect;
use sacp::DynComponent;
use sacp_conductor::Conductor;
use symposium_crate_sources_proxy::CrateSourcesProxy;

/// Test that the rust_crate_query tool can be invoked and triggers a new session.
///
/// This test verifies:
/// 1. The CrateSourcesProxy exposes the rust_crate_query MCP tool
/// 2. Calling the tool triggers a new session to be spawned
/// 3. The session receives the research prompt
/// 4. The proxy handles the response (even if nonsensical from Eliza)
#[tokio::test]
async fn test_rust_crate_query_with_elizacp() -> Result<()> {
    // Initialize tracing for test output
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::DEBUG.into()),
        )
        .try_init();

    // Create the component chain: CrateSourcesProxy -> ElizACP
    let proxy = CrateSourcesProxy;

    // Send a tool invocation to rust_crate_query
    // ElizACP expects format: "Use tool <server>::<tool> with <json_params>"
    let response = yopo::prompt(
        Conductor::new(
            "test-conductor".to_string(),
            vec![
                DynComponent::new(proxy),
                DynComponent::new(elizacp::ElizaAgent::new()),
            ],
            Default::default(),
        ),
        r#"Use tool rust-crate-query::rust_crate_query with {"crate_name":"serde","prompt":"What is the signature of from_value?"}"#,
    )
    .await?;

    // Verify the response matches expected output
    // The research sub-agent session is spawned successfully. Eliza responds with
    // a greeting, then calls the get_rust_crate_source tool which returns empty results.
    expect![[r#"Hello. How are you feeling today?OK: CallToolResult { content: [Annotated { raw: Text(RawTextContent { text: "{\"result\":[]}", meta: None }), annotations: None }], structured_content: Some(Object {"result": Array []}), is_error: Some(false), meta: None }"#]].assert_eq(&response);

    Ok(())
}
