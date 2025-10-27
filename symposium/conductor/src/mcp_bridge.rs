//! MCP Bridge: Bridges MCP JSON-RPC over stdio to TCP connection
//!
//! This module implements `conductor mcp $port` mode, which acts as an MCP server
//! over stdio but forwards all messages to/from a TCP connection on localhost:$port.
//!
//! The main conductor (in agent mode) listens on the TCP port and translates between
//! TCP (raw JSON-RPC) and ACP `_mcp/*` extension messages.

use anyhow::Context;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

/// Run the MCP bridge: stdio ↔ TCP
///
/// Reads MCP JSON-RPC messages from stdin, forwards to TCP connection.
/// Reads responses from TCP, writes to stdout.
pub async fn run_mcp_bridge(port: u16) -> Result<(), agent_client_protocol::Error> {
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
async fn connect_with_retry(port: u16) -> Result<TcpStream, agent_client_protocol::Error> {
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
                return Err(agent_client_protocol::Error::into_internal_error(e));
            }
        }
    }

    unreachable!()
}
