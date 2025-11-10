//! Session logging infrastructure
//!
//! Creates session directories and logs messages at each stage of the proxy chain.
//!
//! Architecture:
//! - LoggingTransport wrappers intercept messages at each connection point
//! - All log messages are sent to a central LoggingActor
//! - LoggingActor writes messages to stage-specific JSONL files
//!
//! Directory structure:
//! ```text
//! ~/.symposium/logs/
//!   {encoded-workspace-path}/
//!     {YYYY-MM-DD}/
//!       {uuid}/
//!         session.json      ← Session metadata
//!         stage0.jsonl      ← Messages at stage 0
//!         stage1.jsonl      ← Messages at stage 1 (if present)
//!         ...
//! ```

use anyhow::{Context, Result};
use futures::channel::mpsc;
use futures::{AsyncRead, AsyncWrite, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

/// Message sent to the central logging actor
#[derive(Debug, Clone)]
pub struct LogMessage {
    /// Which stage this message is from
    pub stage: String,

    /// Direction: "→" (to successor) or "←" (from successor)
    pub direction: String,

    /// ISO 8601 timestamp
    pub timestamp: String,

    /// The actual message (raw JSON-RPC)
    pub message: serde_json::Value,
}

/// Serialized log entry for JSONL files
#[derive(Debug, Serialize, Deserialize)]
struct LogEntry {
    dir: String,
    ts: String,
    msg: serde_json::Value,
}

/// Central logging actor that writes messages to stage files
pub struct LoggingActor {
    session_dir: PathBuf,
    log_rx: mpsc::Receiver<LogMessage>,
    stage_files: HashMap<String, tokio::fs::File>,
}

impl LoggingActor {
    /// Create a new logging actor
    fn new(session_dir: PathBuf, log_rx: mpsc::Receiver<LogMessage>) -> Self {
        Self {
            session_dir,
            log_rx,
            stage_files: HashMap::new(),
        }
    }

    /// Run the logging actor (consumes self)
    pub async fn run(mut self) -> Result<()> {
        while let Some(log_msg) = self.log_rx.next().await {
            if let Err(e) = self.handle_message(log_msg).await {
                tracing::error!("Failed to log message: {}", e);
            }
        }

        // Flush all files on shutdown
        for (_, mut file) in self.stage_files.drain() {
            let _ = file.flush().await;
        }

        Ok(())
    }

    async fn handle_message(&mut self, log_msg: LogMessage) -> Result<()> {
        // Get or create file for this stage
        let file = if let Some(file) = self.stage_files.get_mut(&log_msg.stage) {
            file
        } else {
            let filename = format!("{}.jsonl", log_msg.stage);
            let path = self.session_dir.join(filename);
            let file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .await
                .context("failed to open stage log file")?;
            self.stage_files.insert(log_msg.stage.clone(), file);
            self.stage_files.get_mut(&log_msg.stage).unwrap()
        };

        // Write log entry
        let entry = LogEntry {
            dir: log_msg.direction,
            ts: log_msg.timestamp,
            msg: log_msg.message,
        };

        let json = serde_json::to_string(&entry)?;
        file.write_all(json.as_bytes()).await?;
        file.write_all(b"\n").await?;
        file.flush().await?;

        Ok(())
    }
}

/// Session logging coordinator
pub struct SessionLogger {
    session_dir: PathBuf,
    log_tx: mpsc::Sender<LogMessage>,
    _actor_handle: tokio::task::JoinHandle<Result<()>>,
}

/// Metadata about a logging session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    /// Unique session identifier
    pub session_id: String,

    /// When the session started (ISO 8601)
    pub start_time: String,

    /// Working directory where symposium was started
    pub working_directory: String,

    /// Symposium version
    pub symposium_version: String,

    /// Git branch (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,

    /// Git commit hash (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,
}

impl SessionLogger {
    /// Create a new session logger
    ///
    /// Creates the session directory structure, writes session.json,
    /// and spawns the central logging actor.
    pub async fn new() -> Result<Self> {
        let working_dir = std::env::current_dir().context("failed to get current directory")?;
        let session_id = Uuid::new_v4().to_string();

        // Create session directory
        let session_dir = create_session_directory(&working_dir, &session_id)
            .await
            .context("failed to create session directory")?;

        // Write session metadata
        let metadata = SessionMetadata {
            session_id: session_id.clone(),
            start_time: chrono::Utc::now().to_rfc3339(),
            working_directory: working_dir.display().to_string(),
            symposium_version: env!("CARGO_PKG_VERSION").to_string(),
            git_branch: get_git_branch(&working_dir).await,
            git_commit: get_git_commit(&working_dir).await,
        };

        let session_json = session_dir.join("session.json");
        let json = serde_json::to_string_pretty(&metadata)?;
        fs::write(&session_json, json)
            .await
            .context("failed to write session.json")?;

        // Create channel for logging actor
        let (log_tx, log_rx) = mpsc::channel(1024);

        // Spawn logging actor
        let actor = LoggingActor::new(session_dir.clone(), log_rx);
        let actor_handle = tokio::spawn(async move { actor.run().await });

        tracing::info!(
            "Created session {} at {}",
            session_id,
            session_dir.display()
        );

        Ok(Self {
            session_dir,
            log_tx,
            _actor_handle: actor_handle,
        })
    }

    /// Get a sender for logging messages from a specific stage
    pub fn stage_logger(&self, stage: String) -> StageLogger {
        StageLogger {
            stage,
            log_tx: self.log_tx.clone(),
        }
    }

    /// Get the session directory path
    pub fn session_dir(&self) -> &Path {
        &self.session_dir
    }
}

/// Logger for a specific stage in the proxy chain
#[derive(Clone)]
pub struct StageLogger {
    stage: String,
    log_tx: mpsc::Sender<LogMessage>,
}

impl StageLogger {
    /// Log a message at this stage
    pub async fn log(&mut self, direction: &str, message: serde_json::Value) {
        let log_msg = LogMessage {
            stage: self.stage.clone(),
            direction: direction.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            message,
        };

        // Best effort - if channel is full or closed, just drop the message
        let _ = self.log_tx.try_send(log_msg);
    }
}

/// Create the session directory structure
///
/// Returns the path to the session directory:
/// `~/.symposium/logs/{encoded-path}/{date}/{uuid}/`
async fn create_session_directory(working_dir: &Path, session_id: &str) -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME environment variable not set")?;
    let logs_root = PathBuf::from(home).join(".symposium").join("logs");

    // Encode the working directory path
    let encoded_path = encode_path(working_dir);

    // Get current date
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();

    // Build session directory path
    let session_dir = logs_root.join(encoded_path).join(date).join(session_id);

    // Create the directory
    fs::create_dir_all(&session_dir)
        .await
        .context("failed to create session directory")?;

    Ok(session_dir)
}

/// Encode a filesystem path for use in directory names
///
/// Strips leading `/` and replaces remaining `/` with `-`
fn encode_path(path: &Path) -> String {
    path.display()
        .to_string()
        .trim_start_matches('/')
        .replace('/', "-")
}

/// Get the current git branch name
async fn get_git_branch(working_dir: &Path) -> Option<String> {
    let output = tokio::process::Command::new("git")
        .arg("rev-parse")
        .arg("--abbrev-ref")
        .arg("HEAD")
        .current_dir(working_dir)
        .output()
        .await
        .ok()?;

    if output.status.success() {
        String::from_utf8(output.stdout)
            .ok()
            .map(|s| s.trim().to_string())
    } else {
        None
    }
}

/// Get the current git commit hash
async fn get_git_commit(working_dir: &Path) -> Option<String> {
    let output = tokio::process::Command::new("git")
        .arg("rev-parse")
        .arg("--short")
        .arg("HEAD")
        .current_dir(working_dir)
        .output()
        .await
        .ok()?;

    if output.status.success() {
        String::from_utf8(output.stdout)
            .ok()
            .map(|s| s.trim().to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_path() {
        assert_eq!(
            encode_path(Path::new("/Users/nikomat/dev/symposium")),
            "Users-nikomat-dev-symposium"
        );

        assert_eq!(encode_path(Path::new("/tmp")), "tmp");

        assert_eq!(encode_path(Path::new("relative/path")), "relative-path");
    }
}

/// Wrapper around AsyncRead that logs incoming messages
pub struct LoggingReader<R> {
    inner: R,
    stage_logger: StageLogger,
    buffer: Vec<u8>,
}

impl<R: AsyncRead + Unpin> LoggingReader<R> {
    pub fn new(inner: R, stage_logger: StageLogger) -> Self {
        Self {
            inner,
            stage_logger,
            buffer: Vec::new(),
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for LoggingReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.inner).poll_read(cx, buf);

        // If we successfully read data, append it to our buffer
        if let Poll::Ready(Ok(n)) = result {
            if n > 0 {
                self.buffer.extend_from_slice(&buf[..n]);

                // Try to extract complete JSON-RPC messages
                self.try_extract_messages();
            }
        }

        result
    }
}

impl<R> LoggingReader<R> {
    fn try_extract_messages(&mut self) {
        // JSON-RPC over stdio uses Content-Length framing
        // Format: "Content-Length: N\r\n\r\n{json}"

        loop {
            // Look for Content-Length header
            let header_end = if let Some(pos) = find_subsequence(&self.buffer, b"\r\n\r\n") {
                pos
            } else {
                break; // Need more data
            };

            // Parse Content-Length
            let header = String::from_utf8_lossy(&self.buffer[..header_end]);
            let content_length = if let Some(len) = parse_content_length(&header) {
                len
            } else {
                // Invalid header, skip it
                self.buffer.drain(..header_end + 4);
                continue;
            };

            // Check if we have the complete message
            let message_start = header_end + 4;
            let message_end = message_start + content_length;

            if self.buffer.len() < message_end {
                break; // Need more data
            }

            // Extract and log the message
            let message_bytes = &self.buffer[message_start..message_end];
            if let Ok(message_json) = serde_json::from_slice::<serde_json::Value>(message_bytes) {
                let mut logger = self.stage_logger.clone();
                tokio::spawn(async move {
                    logger.log("←", message_json).await;
                });
            }

            // Remove processed message from buffer
            self.buffer.drain(..message_end);
        }
    }
}

/// Wrapper around AsyncWrite that logs outgoing messages
pub struct LoggingWriter<W> {
    inner: W,
    stage_logger: StageLogger,
    buffer: Vec<u8>,
}

impl<W: AsyncWrite + Unpin> LoggingWriter<W> {
    pub fn new(inner: W, stage_logger: StageLogger) -> Self {
        Self {
            inner,
            stage_logger,
            buffer: Vec::new(),
        }
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for LoggingWriter<W> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.inner).poll_write(cx, buf);

        // If we successfully wrote data, append it to our buffer
        if let Poll::Ready(Ok(n)) = result {
            if n > 0 {
                self.buffer.extend_from_slice(&buf[..n]);

                // Try to extract complete JSON-RPC messages
                self.try_extract_messages();
            }
        }

        result
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_close(cx)
    }
}

impl<W> LoggingWriter<W> {
    fn try_extract_messages(&mut self) {
        // Same logic as LoggingReader
        loop {
            let header_end = if let Some(pos) = find_subsequence(&self.buffer, b"\r\n\r\n") {
                pos
            } else {
                break;
            };

            let header = String::from_utf8_lossy(&self.buffer[..header_end]);
            let content_length = if let Some(len) = parse_content_length(&header) {
                len
            } else {
                self.buffer.drain(..header_end + 4);
                continue;
            };

            let message_start = header_end + 4;
            let message_end = message_start + content_length;

            if self.buffer.len() < message_end {
                break;
            }

            let message_bytes = &self.buffer[message_start..message_end];
            if let Ok(message_json) = serde_json::from_slice::<serde_json::Value>(message_bytes) {
                let mut logger = self.stage_logger.clone();
                tokio::spawn(async move {
                    logger.log("→", message_json).await;
                });
            }

            self.buffer.drain(..message_end);
        }
    }
}

/// Find a subsequence in a slice
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Parse Content-Length from header
fn parse_content_length(header: &str) -> Option<usize> {
    for line in header.lines() {
        if let Some(value) = line.strip_prefix("Content-Length:") {
            return value.trim().parse().ok();
        }
    }
    None
}
