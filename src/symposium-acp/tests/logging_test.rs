//! Integration test for logging infrastructure

#[tokio::test]
async fn test_session_creation() {
    // Create a session logger
    let session_logger = symposium_acp::logging::SessionLogger::new()
        .await
        .expect("Failed to create session logger");

    let session_dir = session_logger.session_dir();

    // Verify session directory exists
    assert!(session_dir.exists(), "Session directory should exist");

    // Verify session.json exists
    let session_json = session_dir.join("session.json");
    assert!(session_json.exists(), "session.json should exist");

    // Read and parse session.json
    let content = tokio::fs::read_to_string(&session_json)
        .await
        .expect("Failed to read session.json");

    let metadata: symposium_acp::logging::SessionMetadata =
        serde_json::from_str(&content).expect("Failed to parse session.json");

    // Verify metadata fields
    assert!(!metadata.session_id.is_empty());
    assert!(!metadata.start_time.is_empty());
    assert!(!metadata.working_directory.is_empty());
    assert_eq!(metadata.symposium_version, env!("CARGO_PKG_VERSION"));

    println!("✓ Session created at: {}", session_dir.display());
    println!("✓ Session ID: {}", metadata.session_id);
}

#[tokio::test]
async fn test_stage_logging() {
    let session_logger = symposium_acp::logging::SessionLogger::new()
        .await
        .expect("Failed to create session logger");

    let mut stage_logger = session_logger.stage_logger("test-stage".to_string());

    // Log some test messages
    let test_msg = serde_json::json!({"jsonrpc": "2.0", "method": "test", "params": {}});
    stage_logger.log("→", test_msg.clone()).await;
    stage_logger.log("←", test_msg).await;

    // Give the logging actor time to write
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Verify stage file was created
    let stage_file = session_logger.session_dir().join("test-stage.jsonl");
    assert!(stage_file.exists(), "Stage log file should exist");

    // Read and verify contents
    let content = tokio::fs::read_to_string(&stage_file)
        .await
        .expect("Failed to read stage log");

    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 2, "Should have 2 log entries");

    println!("✓ Stage logging works");
    println!("✓ Log file: {}", stage_file.display());
}
