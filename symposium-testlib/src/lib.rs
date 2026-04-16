//! Integration test harness for symposium.
//!
//! Provides `TestContext` (wrapping `Symposium::from_dir()`) and `with_fixture()`
//! for composable, isolated test environments.

use std::path::{Path, PathBuf};

use clap::Parser;
use serde::{Deserialize, Serialize};

use symposium::cli::Cli;
use symposium::config::Symposium;
use symposium::dispatch::{self, DispatchResult};
use symposium::hook;
use symposium::hook_schema::HookAgent;
use symposium::hook_schema::HookEvent;
use symposium::mcp::McpArgs;
use symposium::output::Output;

// ── Agent-neutral test types ──────────────────────────────────────────

/// A single step in a hook scenario. Used by simulation mode to drive
/// `execute_hook`; ignored in agent mode (the real agent produces its own hooks).
pub enum HookStep {
    SessionStart,
    UserPromptSubmit { prompt: String },
    PreToolUse {
        tool_name: String,
        tool_input: serde_json::Value,
    },
    PostToolUse {
        tool_name: String,
        tool_input: serde_json::Value,
        tool_response: serde_json::Value,
    },
}

impl HookStep {
    pub fn session_start() -> Self {
        Self::SessionStart
    }

    pub fn user_prompt(prompt: &str) -> Self {
        Self::UserPromptSubmit {
            prompt: prompt.to_string(),
        }
    }

    pub fn event(&self) -> HookEvent {
        match self {
            Self::SessionStart => HookEvent::SessionStart,
            Self::UserPromptSubmit { .. } => HookEvent::UserPromptSubmit,
            Self::PreToolUse { .. } => HookEvent::PreToolUse,
            Self::PostToolUse { .. } => HookEvent::PostToolUse,
        }
    }
}

/// One hook invocation's input and output, in canonical form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookTrace {
    pub event: HookEvent,
    pub agent: HookAgent,
    pub input: serde_json::Value,
    pub output: serde_json::Value,
}

/// Collected results from a `submit` call.
#[derive(Debug, Clone)]
pub struct SubmitResult {
    pub hooks: Vec<HookTrace>,
}

impl SubmitResult {
    /// Filter traces by event type.
    pub fn outputs_for(&self, event: HookEvent) -> Vec<&HookTrace> {
        self.hooks.iter().filter(|h| h.event == event).collect()
    }

    /// Check if any hook output contains the given substring in additionalContext.
    pub fn has_context_containing(&self, needle: &str) -> bool {
        self.hooks.iter().any(|h| {
            h.output
                .get("additionalContext")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s.contains(needle))
        })
    }
}

/// Test context wrapping an isolated `Symposium` instance.
pub struct TestContext {
    pub sym: Symposium,
    /// The temporary directory (kept alive for the test's duration).
    pub _tempdir: tempfile::TempDir,
    /// Root of the overlaid workspace (if a workspace fixture was included).
    pub workspace_root: Option<PathBuf>,
}

impl TestContext {
    /// Run a `symposium` command against this test context.
    pub async fn symposium(&mut self, args: &[&str]) -> anyhow::Result<()> {
        let mut full_args = vec!["symposium"];
        full_args.push("-q");
        full_args.extend_from_slice(args);

        let cli = Cli::try_parse_from(&full_args)
            .map_err(|e| anyhow::anyhow!("failed to parse args: {e}"))?;

        let out = Output::quiet();
        let cwd = self
            .workspace_root
            .clone()
            .unwrap_or_else(|| self.sym.config_dir().to_path_buf());

        match cli.command {
            Some(cmd) => symposium::cli::run(&mut self.sym, cmd, &cwd, &out).await,
            None => Ok(()),
        }
    }

    /// Call the shared dispatch function, returning the output string.
    pub async fn invoke(&self, args: &[&str]) -> Result<String, String> {
        let parsed =
            McpArgs::try_parse_from(args).map_err(|e| format!("failed to parse args: {e}"))?;
        let cwd = self
            .workspace_root
            .as_deref()
            .unwrap_or_else(|| self.sym.config_dir());
        match dispatch::dispatch(&self.sym, parsed.command, cwd, dispatch::RenderMode::Mcp).await {
            DispatchResult::Ok(output) => Ok(output),
            DispatchResult::Err(e) => Err(e),
        }
    }

    /// Run the full hook pipeline: parse → builtin → plugins → serialize.
    ///
    /// This is what `symposium hook <agent> <event>` does, minus stdin/stdout.
    /// The payload is serialized to JSON and fed through the agent's parser,
    /// so it should match the agent's expected wire format.
    pub async fn invoke_hook(
        &self,
        agent: HookAgent,
        event: hook::HookEvent,
        payload: &impl Serialize,
    ) -> anyhow::Result<Vec<u8>> {
        let input = serde_json::to_string(payload)?;
        hook::execute_hook(&self.sym, agent, event, &input).await
    }

    /// Replace temp directory paths with a stable placeholder for snapshot tests.
    pub fn normalize_paths(&self, output: &str) -> String {
        let config_dir = self.sym.config_dir().to_string_lossy().to_string();
        output.replace(&config_dir, "$CONFIG_DIR")
    }
}

/// Directories discovered while copying fixture files.
struct FixtureScanResult {
    config_dirs: Vec<PathBuf>,
    workspace_dirs: Vec<PathBuf>,
}

/// Create a test context by overlaying fixture fragments into a tempdir.
///
/// Text files (`.toml`, `.md`, `.json`, `.txt`, `.ts`, `.js`) have variables expanded:
/// - `$TEST_DIR` — the tempdir root
/// - `$BINARY` — path to the `symposium` binary (from `CARGO_BIN_EXE_symposium`)
pub fn with_fixture(fixtures: &[&str]) -> TestContext {
    let fixtures_base = Path::new(env!("SYMPOSIUM_FIXTURES_DIR"));
    let tempdir = tempfile::tempdir().expect("failed to create tempdir");
    let root = tempdir.path();

    let test_dir = root.to_str().expect("tempdir path is UTF-8");
    let binary = std::env::var("CARGO_BIN_EXE_symposium").unwrap_or_default();

    let vars = [("$TEST_DIR", test_dir), ("$BINARY", &binary)];

    let mut scan = FixtureScanResult {
        config_dirs: Vec::new(),
        workspace_dirs: Vec::new(),
    };

    for fixture_name in fixtures {
        let fixture_dir = fixtures_base.join(fixture_name);
        assert!(
            fixture_dir.is_dir(),
            "fixture not found: {}",
            fixture_dir.display()
        );
        copy_dir_recursive(&fixture_dir, root, &mut scan, &vars);
    }

    assert!(
        scan.config_dirs.len() <= 1,
        "multiple config.toml found in fixtures: {:?}",
        scan.config_dirs
    );
    let config_dir = scan
        .config_dirs
        .first()
        .cloned()
        .unwrap_or_else(|| root.to_path_buf());

    assert!(
        scan.workspace_dirs.len() <= 1,
        "multiple Cargo.toml found in fixtures: {:?}",
        scan.workspace_dirs
    );
    let workspace_root = scan.workspace_dirs.first().cloned();

    let sym = Symposium::from_dir(&config_dir);

    TestContext {
        sym,
        _tempdir: tempdir,
        workspace_root,
    }
}

/// File extensions that get variable expansion.
fn is_text_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("toml" | "md" | "json" | "txt" | "ts" | "js")
    )
}

/// Expand variables in file content.
fn expand_vars(content: &str, vars: &[(&str, &str)]) -> String {
    let mut result = content.to_string();
    for (var, value) in vars {
        result = result.replace(var, value);
    }
    result
}

/// Recursively copy a directory tree, expanding variables in text files.
fn copy_dir_recursive(src: &Path, dst: &Path, scan: &mut FixtureScanResult, vars: &[(&str, &str)]) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path, scan, vars);
        } else {
            if is_text_file(&src_path) {
                let content = std::fs::read_to_string(&src_path).unwrap();
                let expanded = expand_vars(&content, vars);
                std::fs::write(&dst_path, expanded).unwrap();
            } else {
                std::fs::copy(&src_path, &dst_path).unwrap();
            }

            let filename = entry.file_name();
            if filename == "config.toml" {
                let is_user_config = dst.file_name().is_some_and(|n| n == "dot-symposium");
                if is_user_config {
                    scan.config_dirs.push(dst.to_path_buf());
                }
            } else if filename == "Cargo.toml" {
                scan.workspace_dirs.push(dst.to_path_buf());
            }
        }
    }
}
