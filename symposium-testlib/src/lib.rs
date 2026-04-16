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
use symposium::hook_schema::symposium as sym_types;
use symposium::mcp::McpArgs;
use symposium::output::Output;

// ── Agent-neutral test types ──────────────────────────────────────────

/// A single step in a hook scenario. Used by simulation mode to drive
/// `execute_hook`; ignored in agent mode (the real agent produces its own hooks).
pub enum HookStep {
    SessionStart,
    UserPromptSubmit {
        prompt: String,
    },
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

    /// Convert to a canonical symposium InputEvent, injecting cwd and session_id.
    pub fn to_input_event(&self, cwd: &str) -> sym_types::InputEvent {
        let session_id = Some("test-session-id".to_string());
        let cwd = Some(cwd.to_string());
        match self {
            Self::SessionStart => {
                sym_types::InputEvent::SessionStart(sym_types::SessionStartInput {
                    session_id,
                    cwd,
                })
            }
            Self::UserPromptSubmit { prompt } => {
                sym_types::InputEvent::UserPromptSubmit(sym_types::UserPromptSubmitInput {
                    prompt: prompt.clone(),
                    session_id,
                    cwd,
                })
            }
            Self::PreToolUse {
                tool_name,
                tool_input,
            } => sym_types::InputEvent::PreToolUse(sym_types::PreToolUseInput {
                tool_name: tool_name.clone(),
                tool_input: tool_input.clone(),
                session_id,
                cwd,
            }),
            Self::PostToolUse {
                tool_name,
                tool_input,
                tool_response,
            } => sym_types::InputEvent::PostToolUse(sym_types::PostToolUseInput {
                tool_name: tool_name.clone(),
                tool_input: tool_input.clone(),
                tool_response: tool_response.clone(),
                session_id,
                cwd,
            }),
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
    /// Handles both top-level and hookSpecificOutput-nested additionalContext.
    pub fn has_context_containing(&self, needle: &str) -> bool {
        self.hooks.iter().any(|h| {
            let top = h.output.get("additionalContext").and_then(|v| v.as_str());
            let nested = h
                .output
                .get("hookSpecificOutput")
                .and_then(|o| o.get("additionalContext"))
                .and_then(|v| v.as_str());
            top.into_iter().chain(nested).any(|s| s.contains(needle))
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

    /// Submit a scenario as a sequence of hook steps.
    ///
    /// In simulation mode (default): converts each step to the agent wire format,
    /// calls `execute_hook`, and collects results.
    ///
    /// In agent mode (`SYMPOSIUM_TEST_AGENT=claude`): sends the prompt to a real
    /// agent via the Python harness and reads the JSONL hook trace file.
    pub async fn submit(
        &self,
        prompt: &str,
        steps: &[HookStep],
        agent: HookAgent,
    ) -> anyhow::Result<SubmitResult> {
        let cwd = self
            .workspace_root
            .as_deref()
            .unwrap_or_else(|| self.sym.config_dir());

        match std::env::var("SYMPOSIUM_TEST_AGENT").ok().as_deref() {
            Some("claude") => self.submit_agent(prompt, cwd).await,
            _ => {
                self.submit_simulation(steps, agent, &cwd.to_string_lossy())
                    .await
            }
        }
    }

    async fn submit_simulation(
        &self,
        steps: &[HookStep],
        agent: HookAgent,
        cwd: &str,
    ) -> anyhow::Result<SubmitResult> {
        let mut hooks = Vec::new();
        for step in steps {
            let event = step.event();
            let sym_input = step.to_input_event(cwd);

            let handler = agent
                .event(event)
                .ok_or_else(|| anyhow::anyhow!("agent {agent:?} does not support {event:?}"))?;
            let agent_input = handler.from_symposium_input(&sym_input);
            let input_str = agent_input.to_string()?;

            let output_bytes = hook::execute_hook(&self.sym, agent, event, &input_str).await?;

            let input_val: serde_json::Value = serde_json::from_str(&input_str)?;
            let output_val: serde_json::Value = if output_bytes.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::from_slice(&output_bytes)?
            };

            hooks.push(HookTrace {
                event,
                agent,
                input: input_val,
                output: output_val,
            });
        }
        Ok(SubmitResult { hooks })
    }

    async fn submit_agent(&self, prompt: &str, cwd: &Path) -> anyhow::Result<SubmitResult> {
        let trace_path = cwd.join("hook-trace.jsonl");

        // Locate the harness script relative to the project source.
        let harness = Path::new(env!("SYMPOSIUM_FIXTURES_DIR"))
            .parent()
            .expect("fixtures dir has parent")
            .join("agent_harness/run_scenario.py");

        // Build CARGO_BIN_DIR from the binary path cargo gives us.
        let bin_exe = std::env::var("CARGO_BIN_EXE_symposium")
            .expect("CARGO_BIN_EXE_symposium must be set (run via cargo test)");
        let bin_dir = Path::new(&bin_exe)
            .parent()
            .expect("binary has parent dir")
            .to_string_lossy();

        let output = std::process::Command::new("uv")
            .args(["run", "--with", "claude-agent-sdk"])
            .arg(&harness)
            .arg("--prompt")
            .arg(prompt)
            .arg("--cwd")
            .arg(cwd)
            .arg("--trace")
            .arg(&trace_path)
            .env("CARGO_BIN_DIR", bin_dir.as_ref())
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("agent harness failed ({}): {stderr}", output.status);
        }

        // Parse the JSONL trace file.
        let trace_content = std::fs::read_to_string(&trace_path).unwrap_or_default();
        let hooks: Vec<HookTrace> = trace_content
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_str(l))
            .collect::<Result<_, _>>()?;

        Ok(SubmitResult { hooks })
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
