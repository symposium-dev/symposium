//! Integration test harness for symposium.
//!
//! Provides `TestContext` (wrapping `Symposium::from_dir()`) and `with_fixture()`
//! for composable, isolated test environments.

use std::path::{Path, PathBuf};

use clap::Parser;
use serde::{Deserialize, Serialize};

use symposium::cli::Cli;
use symposium::config::Symposium;
use symposium::hook;
use symposium::hook_schema::HookAgent;
use symposium::hook_schema::HookEvent;
use symposium::hook_schema::symposium as sym_types;

// ── Agent-neutral test types ──────────────────────────────────────────

/// Which agent backend to use for integration tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestAgent {
    /// Real Claude session via the Claude Agent SDK.
    ClaudeSdk,
    /// ACP agent from the ACP registry (via acpr).
    Acp {
        /// Agent name in the ACP registry (e.g., "claude-acp", "gemini").
        registry_name: String,
        /// Symposium agent name (e.g., "claude", "gemini").
        agent_name: String,
    },
    /// ACP agent with a custom command (not in the registry).
    CustomAcp {
        /// Command to spawn the ACP agent.
        command: String,
        /// Symposium agent name.
        agent_name: String,
    },
}

/// Reads `SYMPOSIUM_TEST_AGENT` and returns the agent backend, if set.
pub fn test_agent() -> Option<TestAgent> {
    let val = std::env::var("SYMPOSIUM_TEST_AGENT").ok()?;
    if val.is_empty() {
        return None;
    }
    Some(match val.as_str() {
        "claude-sdk" => TestAgent::ClaudeSdk,
        "kiro-cli-acp" => TestAgent::CustomAcp {
            command: "kiro-cli acp --agent symposium".into(),
            agent_name: "kiro".into(),
        },
        // Everything else is looked up in the ACP registry.
        name => TestAgent::Acp {
            registry_name: name.into(),
            agent_name: infer_agent_name(name).into(),
        },
    })
}

/// Infer the symposium agent name from a registry name.
fn infer_agent_name(registry_name: &str) -> &str {
    match registry_name {
        "claude-acp" => "claude",
        "codex-acp" => "codex",
        "gemini" => "gemini",
        "goose" => "goose",
        "opencode" => "opencode",
        other => other,
    }
}

/// Returns `true` when running in agent mode (i.e. `SYMPOSIUM_TEST_AGENT` is set).
pub fn is_agent_mode() -> bool {
    test_agent().is_some()
}

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
    /// The agent's final text response (agent mode only).
    pub response: Option<String>,
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

/// A live agent session for the duration of a test.
pub enum AgentSession {
    /// Claude Agent SDK — shells out to Python per-prompt.
    ClaudeSdk,
    /// ACP agent with a persistent session.
    Acp(sacp::ActiveSession<'static, sacp::Agent>),
}

/// Test context wrapping an isolated `Symposium` instance.
pub struct TestContext {
    pub sym: Symposium,
    /// Root of the temporary directory. Cleaned up on success, kept on failure.
    pub tempdir: PathBuf,
    /// Handle that deletes the tempdir on drop — set to `None` on failure.
    _tempdir_guard: Option<tempfile::TempDir>,
    /// Root of the overlaid workspace (if a workspace fixture was included).
    pub workspace_root: Option<PathBuf>,
    /// Live agent session (None in simulation mode).
    session: Option<AgentSession>,
}

impl Drop for TestContext {
    fn drop(&mut self) {
        if std::thread::panicking() {
            // Leak the tempdir so it can be inspected after failure.
            if let Some(td) = self._tempdir_guard.take() {
                let path = td.keep();
                eprintln!("test failed — tempdir preserved at: {}", path.display());
            }
        }
    }
}

impl TestContext {
    /// Run a `symposium` CLI command in-process, returning captured output
    /// with temp-dir paths normalized.
    pub async fn symposium(&mut self, args: &[&str]) -> anyhow::Result<String> {
        let mut full_args = vec!["cargo-agents", "-q"];
        full_args.extend_from_slice(args);

        let cli = Cli::try_parse_from(&full_args).map_err(|e| anyhow::anyhow!("{e}"))?;

        let out = symposium::output::Output::quiet();
        let cwd = self
            .workspace_root
            .clone()
            .unwrap_or_else(|| self.sym.config_dir().to_path_buf());

        match cli.command {
            Some(cmd) => {
                symposium::cli::run(&mut self.sym, cmd, &cwd, &out).await?;
            }
            None => {}
        }

        Ok(String::new())
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
    /// In agent mode (`SYMPOSIUM_TEST_AGENT=claude-sdk`): sends the prompt to a real
    /// agent via the Python harness and reads the JSONL hook trace file.
    pub async fn prompt_or_hook(
        &mut self,
        prompt: &str,
        steps: &[HookStep],
        agent: HookAgent,
    ) -> anyhow::Result<SubmitResult> {
        if self.session.is_some() {
            let result = self.prompt(prompt).await?;
            // Verify the agent triggered the expected hooks.
            for step in steps {
                let expected_event = step.event();
                assert!(
                    result.hooks.iter().any(|h| h.event == expected_event),
                    "expected hook {expected_event:?} not found in agent trace: {:#?}",
                    result.hooks
                );
            }
            return Ok(result);
        }
        let cwd = self
            .workspace_root
            .clone()
            .unwrap_or_else(|| self.sym.config_dir().to_path_buf());
        self.submit_simulation(steps, agent, &cwd.to_string_lossy())
            .await
    }

    /// Send a prompt to a real agent and collect hook traces.
    ///
    /// Agent-only — panics if `SYMPOSIUM_TEST_AGENT` is not set.
    /// Use `is_agent_mode()` to skip the test in simulation mode.
    pub async fn prompt(&mut self, prompt: &str) -> anyhow::Result<SubmitResult> {
        let cwd = self
            .workspace_root
            .clone()
            .unwrap_or_else(|| self.sym.config_dir().to_path_buf());

        match self.session.as_mut() {
            None => panic!("prompt() requires agent mode (SYMPOSIUM_TEST_AGENT)"),
            Some(AgentSession::ClaudeSdk) => self.submit_agent(prompt, &cwd).await,
            Some(AgentSession::Acp(session)) => {
                let trace_path = cwd.join("hook-trace.jsonl");
                // Clear previous trace so we only get hooks from this prompt.
                let _ = std::fs::remove_file(&trace_path);

                session.send_prompt(prompt)?;
                let text: String = session
                    .read_to_string()
                    .await
                    .map_err(|e| anyhow::anyhow!("ACP read failed: {e}"))?;

                let trace_content = std::fs::read_to_string(&trace_path).unwrap_or_default();
                let hooks: Vec<HookTrace> = trace_content
                    .lines()
                    .filter(|l| !l.is_empty())
                    .map(|l| serde_json::from_str(l))
                    .collect::<Result<_, _>>()?;

                let response = if text.is_empty() { None } else { Some(text) };
                Ok(SubmitResult { hooks, response })
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
            let agent_input = handler.translate_input(&sym_input);
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
        Ok(SubmitResult {
            hooks,
            response: None,
        })
    }

    async fn submit_agent(&self, prompt: &str, cwd: &Path) -> anyhow::Result<SubmitResult> {
        let trace_path = cwd.join("hook-trace.jsonl");
        let response_path = cwd.join("agent-response.txt");

        // Locate the harness script relative to the project source.
        let harness = Path::new(env!("SYMPOSIUM_FIXTURES_DIR"))
            .parent()
            .expect("fixtures dir has parent")
            .join("agent_harness/run_scenario.py");

        // Build CARGO_BIN_DIR from the binary path cargo gives us.
        let bin_exe = std::env::var("CARGO_BIN_EXE_cargo-agents")
            .expect("CARGO_BIN_EXE_cargo-agents must be set (run via cargo test)");
        let bin_dir = Path::new(&bin_exe)
            .parent()
            .expect("binary has parent dir")
            .to_string_lossy();

        let output = std::process::Command::new("uv")
            .args(["run", "--no-project", "--with", "claude-agent-sdk"])
            .arg(&harness)
            .arg("--debug")
            .arg("--prompt")
            .arg(prompt)
            .arg("--cwd")
            .arg(cwd)
            .arg("--trace")
            .arg(&trace_path)
            .arg("--response")
            .arg(&response_path)
            .env("CARGO_BIN_DIR", bin_dir.as_ref())
            .env("SYMPOSIUM_HOME", self.sym.config_dir())
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("agent harness failed ({}): {stderr}", output.status);
        }

        // Forward harness debug output to test stderr.
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.is_empty() {
            eprint!("{stderr}");
        }

        // Parse the JSONL trace file.
        let trace_content = std::fs::read_to_string(&trace_path).unwrap_or_default();
        let hooks: Vec<HookTrace> = trace_content
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_str(l))
            .collect::<Result<_, _>>()?;

        let response = std::fs::read_to_string(&response_path).ok();

        Ok(SubmitResult { hooks, response })
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
/// - `$BINARY` — path to the `cargo-agents` binary (from `CARGO_BIN_EXE_cargo-agents`)
async fn setup_fixture(fixtures: &[&str]) -> TestContext {
    let fixtures_base = Path::new(env!("SYMPOSIUM_FIXTURES_DIR"));
    let tempdir = tempfile::tempdir().expect("failed to create tempdir");
    let root = tempdir.path();

    let test_dir = root.to_str().expect("tempdir path is UTF-8");
    let binary = std::env::var("CARGO_BIN_EXE_cargo-agents").unwrap_or_default();

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

    if !scan.config_dirs.is_empty() {
        assert!(
            sym.config.hook_scope == symposium::config::HookScope::Project,
            "integration test fixtures must set `hook-scope = \"project\"` in config.toml"
        );
    }

    let ctx = TestContext {
        sym,
        tempdir: root.to_path_buf(),
        _tempdir_guard: Some(tempdir),
        workspace_root,
        session: None,
    };

    ctx
}

/// Which modes a test should run in.
#[derive(Debug, Clone, Copy)]
pub enum TestMode {
    /// Only run in simulation mode (no agent).
    SimulationOnly,
    /// Only run when an agent is configured.
    AgentOnly,
    /// Run in both modes.
    Any,
}

/// Read the list of test agents from `test-agents.toml` at the repo root,
/// filtered by `SYMPOSIUM_TEST_AGENT` env var if set.
fn configured_test_agents() -> Vec<TestAgent> {
    #[derive(serde::Deserialize)]
    struct TestAgentsConfig {
        #[serde(rename = "test-agents")]
        test_agents: Vec<String>,
    }

    if std::env::var("SYMPOSIUM_ENABLE_AGENT_TESTING").is_err() {
        return vec![];
    }

    // If env var is set, use only that agent.
    if let Some(agent) = test_agent() {
        return vec![agent];
    }

    let config_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("testlib has parent")
        .join("test-agents.toml");

    let content = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => r#"test-agents = ["claude-sdk"]"#.to_string(),
    };

    let config: TestAgentsConfig =
        toml::from_str(&content).unwrap_or_else(|e| panic!("bad test-agents.toml: {e}"));

    config
        .test_agents
        .iter()
        .filter_map(|name| parse_agent_name(name))
        .collect()
}

/// Parse an agent name string into a TestAgent.
fn parse_agent_name(name: &str) -> Option<TestAgent> {
    Some(match name {
        "" => return None,
        "claude-sdk" => TestAgent::ClaudeSdk,
        "kiro-cli-acp" => TestAgent::CustomAcp {
            command: "kiro-cli acp --agent symposium".into(),
            agent_name: "kiro".into(),
        },
        name => TestAgent::Acp {
            registry_name: name.into(),
            agent_name: infer_agent_name(name).into(),
        },
    })
}

/// Set up a test fixture and run the given closure.
///
/// - `SimulationOnly`: runs once in simulation mode.
/// - `AgentOnly`: runs once per configured test agent.
/// - `Any`: runs once in simulation + once per configured test agent.
pub async fn with_fixture(
    mode: TestMode,
    fixtures: &[&str],
    f: impl AsyncFn(TestContext) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let agents = configured_test_agents();

    // Simulation run.
    if matches!(mode, TestMode::SimulationOnly | TestMode::Any) {
        let ctx = setup_fixture(fixtures).await;
        f(ctx).await?;
    }

    // Agent runs.
    if matches!(mode, TestMode::AgentOnly | TestMode::Any) {
        for agent in &agents {
            eprintln!("[test] running with agent: {agent:?}");
            let mut ctx = setup_fixture(fixtures).await;

            let agent_name = match agent {
                TestAgent::ClaudeSdk => "claude",
                TestAgent::Acp { agent_name, .. } | TestAgent::CustomAcp { agent_name, .. } => {
                    agent_name
                }
            };
            ctx.symposium(&["init", "--add-agent", agent_name])
                .await
                .expect("failed to init agent in fixture");
            if ctx.workspace_root.is_some() {
                ctx.symposium(&["sync"])
                    .await
                    .expect("failed to sync in fixture");
            }

            match agent {
                TestAgent::ClaudeSdk => {
                    ctx.session = Some(AgentSession::ClaudeSdk);
                    f(ctx).await?;
                }
                TestAgent::Acp { registry_name, .. } => {
                    run_with_acp_session(acpr::Acpr::new(registry_name), ctx, &f).await?;
                }
                TestAgent::CustomAcp { command, .. } => {
                    let acp_agent: sacp_tokio::AcpAgent = command
                        .parse()
                        .map_err(|e| anyhow::anyhow!("bad command: {e}"))?;
                    run_with_acp_session(acp_agent, ctx, &f).await?;
                }
            }
        }
    }

    Ok(())
}

/// Establish an ACP connection, create a session, and run the callback inside it.
async fn run_with_acp_session(
    agent: impl sacp::ConnectTo<sacp::Client>,
    mut ctx: TestContext,
    f: &impl AsyncFn(TestContext) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    use sacp::schema::{
        InitializeRequest, ProtocolVersion, RequestPermissionOutcome, RequestPermissionRequest,
        RequestPermissionResponse, SelectedPermissionOutcome,
    };

    let cwd = ctx
        .workspace_root
        .clone()
        .unwrap_or_else(|| ctx.sym.config_dir().to_path_buf());

    // Set env vars so the agent subprocess finds symposium and writes traces.
    let bin_exe = std::env::var("CARGO_BIN_EXE_cargo-agents")
        .expect("CARGO_BIN_EXE_cargo-agents must be set (run via cargo test)");
    let bin_dir = Path::new(&bin_exe).parent().expect("binary has parent dir");
    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    // SAFETY: called before spawning any threads for this test.
    unsafe {
        std::env::set_var("PATH", &path);
        std::env::set_var("SYMPOSIUM_HOOK_TRACE", cwd.join("hook-trace.jsonl"));
        std::env::set_var("SYMPOSIUM_HOME", ctx.sym.config_dir());
    }

    let cwd_for_session = cwd.clone();

    sacp::Client
        .builder()
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, _cx| {
                eprintln!("[acp] auto-approving: {:?}", request);
                let option_id = request.options.first().map(|opt| opt.option_id.clone());
                match option_id {
                    Some(id) => responder.respond(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(id)),
                    )),
                    None => responder.respond(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Cancelled,
                    )),
                }
            },
            sacp::on_receive_request!(),
        )
        .connect_with(agent, async |cx| {
            cx.send_request(InitializeRequest::new(ProtocolVersion::LATEST))
                .block_task()
                .await?;

            let session = cx
                .build_session(&cwd_for_session)
                .block_task()
                .start_session()
                .await?;

            ctx.session = Some(AgentSession::Acp(session));
            f(ctx).await.map_err(sacp::util::internal_error)?;
            Ok(())
        })
        .await
        .map_err(|e| anyhow::anyhow!("ACP session failed: {e}"))
}

/// File extensions that get variable expansion.
fn is_text_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("toml" | "md" | "json" | "txt" | "ts" | "js" | "sh")
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
