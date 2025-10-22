//! MCP Bridge: Bridges MCP JSON-RPC over stdio to TCP connection
//!
//! This module implements `conductor mcp $port` mode, which acts as an MCP server
//! over stdio but forwards all messages to/from a TCP connection on localhost:$port.
//!
//! The main conductor (in agent mode) listens on the TCP port and translates between
//! TCP (raw JSON-RPC) and ACP `_mcp/*` extension messages.

use anyhow::{Context, Result};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

/// Run the MCP bridge: stdio ↔ TCP
///
/// Reads MCP JSON-RPC messages from stdin, forwards to TCP connection.
/// Reads responses from TCP, writes to stdout.
pub async fn run_mcp_bridge(port: u16) -> Result<()> {
    tracing::info!("MCP bridge starting, connecting to localhost:{}", port);

    // Connect to the main conductor via TCP
    let stream = connect_with_retry(port).await?;
    let (tcp_read, mut tcp_write) = stream.into_split();

    // Set up stdio
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut stdin_reader = BufReader::new(stdin);
    let mut stdout_writer = stdout;
    let mut tcp_reader = BufReader::new(tcp_read);

    // Prepare line buffers
    let mut stdin_line = String::new();
    let mut tcp_line = String::new();

    tracing::info!("MCP bridge connected, starting message loop");

    loop {
        tokio::select! {
            // Read from stdin → send to TCP
            result = stdin_reader.read_line(&mut stdin_line) => {
                let n = result.context("Failed to read from stdin")?;

                if n == 0 {
                    tracing::info!("Stdin closed, shutting down bridge");
                    break;
                }

                // Parse to validate JSON
                let _: Value = serde_json::from_str(stdin_line.trim())
                    .context("Invalid JSON from stdin")?;

                tracing::debug!("Bridge: stdin → TCP: {}", stdin_line.trim());

                // Forward to TCP
                tcp_write.write_all(stdin_line.as_bytes()).await
                    .context("Failed to write to TCP")?;
                tcp_write.flush().await
                    .context("Failed to flush TCP")?;

                stdin_line.clear();
            }

            // Read from TCP → send to stdout
            result = tcp_reader.read_line(&mut tcp_line) => {
                let n = result.context("Failed to read from TCP")?;

                if n == 0 {
                    tracing::info!("TCP connection closed, shutting down bridge");
                    break;
                }

                // Parse to validate JSON
                let _: Value = serde_json::from_str(tcp_line.trim())
                    .context("Invalid JSON from TCP")?;

                tracing::debug!("Bridge: TCP → stdout: {}", tcp_line.trim());

                // Forward to stdout
                stdout_writer.write_all(tcp_line.as_bytes()).await
                    .context("Failed to write to stdout")?;
                stdout_writer.flush().await
                    .context("Failed to flush stdout")?;

                tcp_line.clear();
            }
        }
    }

    tracing::info!("MCP bridge shutting down");
    Ok(())
}

/// Connect to TCP port with retry logic
async fn connect_with_retry(port: u16) -> Result<TcpStream> {
    let max_retries = 10;
    let mut retry_delay_ms = 50;

    for attempt in 1..=max_retries {
        match TcpStream::connect(format!("127.0.0.1:{}", port)).await {
            Ok(stream) => {
                tracing::info!("Connected to localhost:{} on attempt {}", port, attempt);
                return Ok(stream);
            }
            Err(e) if attempt < max_retries => {
                tracing::debug!(
                    "Connection attempt {} failed: {}, retrying in {}ms",
                    attempt,
                    e,
                    retry_delay_ms
                );
                tokio::time::sleep(tokio::time::Duration::from_millis(retry_delay_ms)).await;
                retry_delay_ms = (retry_delay_ms * 2).min(1000); // Exponential backoff, max 1s
            }
            Err(e) => {
                return Err(e).context(format!(
                    "Failed to connect to localhost:{} after {} attempts",
                    port, max_retries
                ));
            }
        }
    }

    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::process::Command;

    #[tokio::test]
    async fn test_connect_with_retry_success() {
        // Set up a mock TCP server
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        // Spawn a task that accepts the connection
        let accept_task = tokio::spawn(async move {
            listener.accept().await.unwrap();
        });

        // Test connection with retry
        let result = connect_with_retry(port).await;
        assert!(result.is_ok());

        accept_task.await.unwrap();
    }

    #[tokio::test]
    async fn test_connect_with_retry_eventual_success() {
        // Start with no listener, then start one after a delay
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        // Drop the listener to make the port unavailable
        drop(listener);

        // Spawn a task that will start listening after a short delay
        let delayed_server = tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
            let listener = TcpListener::bind(format!("127.0.0.1:{}", port))
                .await
                .unwrap();
            listener.accept().await.unwrap();
        });

        // This should retry and eventually succeed
        let result = connect_with_retry(port).await;
        assert!(result.is_ok());

        delayed_server.await.unwrap();
    }

    #[tokio::test]
    async fn test_message_bridging() {
        // Set up a mock TCP server
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        // Spawn mock TCP server that echoes with modification
        let server_task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(&mut socket);
            let mut line = String::new();

            // Read message from bridge
            reader.read_line(&mut line).await.unwrap();

            // Parse and modify
            let mut msg: Value = serde_json::from_str(line.trim()).unwrap();
            if let Some(obj) = msg.as_object_mut() {
                obj.remove("id");
                obj.insert("result".to_string(), Value::String("pong".to_string()));
            }

            // Send back
            let response = format!("{}\n", serde_json::to_string(&msg).unwrap());
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.flush().await.unwrap();

            // Keep connection open briefly
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        });

        // Create a connected stream to verify the TCP side works
        let stream = TcpStream::connect(format!("127.0.0.1:{}", port))
            .await
            .unwrap();
        let (tcp_read, mut tcp_write) = stream.into_split();
        let mut tcp_reader = BufReader::new(tcp_read);

        // Send a test message
        let test_msg = r#"{"jsonrpc":"2.0","id":"test-1","method":"tools/call"}"#;
        tcp_write
            .write_all(format!("{}\n", test_msg).as_bytes())
            .await
            .unwrap();
        tcp_write.flush().await.unwrap();

        // Read response
        let mut response_line = String::new();
        tcp_reader.read_line(&mut response_line).await.unwrap();

        // Verify response
        let response: Value = serde_json::from_str(response_line.trim()).unwrap();
        assert_eq!(response["result"], "pong");

        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn test_mcp_bridge_integration() {
        // Initialize tracing for test debugging
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::from_default_env()
                    .add_directive("conductor=debug".parse().unwrap()),
            )
            .with_test_writer()
            .try_init();

        // This test simulates the full scenario:
        // Agent (test) ← stdio → conductor mcp ← TCP → Main conductor (mock server)

        // Step 1: Set up mock main conductor (TCP listener)
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        // Step 2: Spawn mock main conductor that will handle TCP messages
        let server_task = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let (tcp_read, mut tcp_write) = socket.into_split();
            let mut reader = BufReader::new(tcp_read);

            // Expect an MCP initialize request from agent
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            tracing::info!("Mock conductor received: {}", line.trim());

            let msg: Value = serde_json::from_str(line.trim()).unwrap();
            assert_eq!(msg["method"], "initialize");
            assert_eq!(msg["id"], "init-1");

            // Send initialize response
            let response = serde_json::json!({
                "jsonrpc": "2.0",
                "id": "init-1",
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "serverInfo": {"name": "test-server", "version": "1.0.0"}
                }
            });
            tcp_write
                .write_all(format!("{}\n", serde_json::to_string(&response).unwrap()).as_bytes())
                .await
                .unwrap();
            tcp_write.flush().await.unwrap();

            // Expect a tools/list request
            line.clear();
            reader.read_line(&mut line).await.unwrap();
            tracing::info!("Mock conductor received: {}", line.trim());

            let msg: Value = serde_json::from_str(line.trim()).unwrap();
            assert_eq!(msg["method"], "tools/list");
            assert_eq!(msg["id"], "tools-1");

            // Send tools/list response
            let response = serde_json::json!({
                "jsonrpc": "2.0",
                "id": "tools-1",
                "result": {
                    "tools": []
                }
            });
            tcp_write
                .write_all(format!("{}\n", serde_json::to_string(&response).unwrap()).as_bytes())
                .await
                .unwrap();
            tcp_write.flush().await.unwrap();

            // Keep connection alive briefly
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        });

        // Step 3: Build path to conductor binary
        let cargo_manifest_dir = env!("CARGO_MANIFEST_DIR");
        let conductor_binary = format!("{}/../../target/debug/conductor", cargo_manifest_dir);

        // Step 4: Spawn `conductor mcp $port` subprocess
        tracing::info!(
            "Spawning conductor binary: {} mcp {}",
            conductor_binary,
            port
        );

        let mut child = Command::new(&conductor_binary)
            .arg("mcp")
            .arg(port.to_string())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .expect("Failed to spawn conductor mcp");

        tracing::info!("Conductor mcp spawned with PID: {:?}", child.id());

        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut stdout_reader = BufReader::new(stdout);

        // Give the bridge time to connect
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Step 5: Send MCP initialize request via stdin (simulating agent)
        let init_request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "init-1",
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "test-client", "version": "1.0.0"}
            }
        });
        stdin
            .write_all(format!("{}\n", serde_json::to_string(&init_request).unwrap()).as_bytes())
            .await
            .unwrap();
        stdin.flush().await.unwrap();

        // Step 6: Read initialize response from stdout
        let mut response_line = String::new();
        let n = stdout_reader.read_line(&mut response_line).await.unwrap();
        tracing::info!("Agent received {} bytes: {:?}", n, response_line.trim());

        if response_line.trim().is_empty() {
            panic!("Received empty response from bridge");
        }

        let response: Value = serde_json::from_str(response_line.trim())
            .expect(&format!("Failed to parse JSON: {}", response_line));
        assert_eq!(response["id"], "init-1");
        assert_eq!(response["result"]["protocolVersion"], "2024-11-05");

        // Step 7: Send tools/list request
        let tools_request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "tools-1",
            "method": "tools/list",
            "params": {}
        });
        stdin
            .write_all(format!("{}\n", serde_json::to_string(&tools_request).unwrap()).as_bytes())
            .await
            .unwrap();
        stdin.flush().await.unwrap();

        // Step 8: Read tools/list response
        response_line.clear();
        stdout_reader.read_line(&mut response_line).await.unwrap();
        tracing::info!("Agent received: {}", response_line.trim());

        let response: Value = serde_json::from_str(response_line.trim()).unwrap();
        assert_eq!(response["id"], "tools-1");
        assert!(response["result"]["tools"].is_array());

        // Clean up
        drop(stdin);
        drop(stdout_reader);

        // Wait for server task to complete
        server_task.await.unwrap();

        // Wait briefly for bridge to shut down gracefully
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Try to kill child if still running
        let _ = child.kill().await;
    }
}
