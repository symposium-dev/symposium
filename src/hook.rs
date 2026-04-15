use std::{
    io::{Read, Write},
    process::{Command, ExitCode, Stdio},
};

use crate::{
    config::Symposium,
    hook_schema::{AgentHookInput, symposium},
    plugins::ParsedPlugin,
};

// Re-export hook schema types for convenience.
pub use crate::hook_schema::{HookAgent, HookEvent};
/// Core hook pipeline: parse → builtin → plugins → serialize.
///
/// Takes the raw agent wire-format input, returns agent wire-format output bytes.
/// Called by both `run()` (CLI) and the test harness.
pub async fn execute_hook(
    sym: &Symposium,
    agent: HookAgent,
    event: HookEvent,
    input: &str,
) -> anyhow::Result<Vec<u8>> {
    let event_handler = agent.event(event);

    if let Some(handler) = event_handler {
        let payload = handler.parse_input(input)?;
        let sym_input = payload.to_symposium();

        // Builtin dispatch → symposium output → host agent output as Value
        let builtin_sym_output = dispatch_builtin(sym, &sym_input).await;
        let builtin_agent_output = handler.from_symposium_output(&builtin_sym_output);
        let prior_output = builtin_agent_output.to_hook_output();

        // Plugin dispatch with format routing
        let final_output = dispatch_plugin_hooks(
            sym,
            agent,
            event,
            &sym_input,
            payload.as_ref(),
            prior_output,
        )
        .map_err(|stderr| {
            anyhow::anyhow!("plugin blocked: {}", String::from_utf8_lossy(&stderr))
        })?;

        Ok(handler.serialize_output(&final_output))
    } else {
        // Agent doesn't support this event
        anyhow::bail!("agent {agent:?} does not support hook event {event:?}")
    }
}

/// CLI entry point: read payload from stdin, dispatch, print output.
pub async fn run(
    sym: &Symposium,
    agent: HookAgent,
    event: HookEvent,
    cwd: &std::path::Path,
) -> ExitCode {
    tracing::debug!("Running hook listener for agent {agent:?} and event {event:?}");

    let mut input = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut input) {
        tracing::warn!(?event, error = %e, "failed to read hook stdin");
        return ExitCode::SUCCESS;
    }
    tracing::debug!(?input);

    // Run sync --agent as a side effect (non-fatal, CLI-only).
    if let Some(handler) = agent.event(event) {
        if let Ok(payload) = handler.parse_input(&input) {
            let sym_input = payload.to_symposium();
            run_sync_agent_sym(sym, &sym_input, cwd).await;
        }
    }

    match execute_hook(sym, agent, event, &input).await {
        Ok(bytes) => {
            if !bytes.is_empty() {
                std::io::stdout().write_all(&bytes).unwrap();
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            tracing::warn!(?event, error = %e, "hook failed");
            ExitCode::FAILURE
        }
    }
}

/// Run sync --agent if we're in a project directory. Non-fatal.
async fn run_sync_agent_sym(sym: &Symposium, input: &symposium::InputEvent, cwd: &std::path::Path) {
    let effective_cwd = input
        .cwd()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| cwd.to_path_buf());
    let project_root = Some(effective_cwd.as_path()).filter(|p| p.join(".symposium").is_dir());
    let out = crate::output::Output::quiet();
    if let Err(e) = crate::sync::sync_agent(sym, project_root, &out).await {
        tracing::warn!(error = %e, "sync --agent during hook failed (continuing)");
    }
}

/// Built-in hook logic on canonical symposium types.
pub async fn dispatch_builtin(
    sym: &Symposium,
    input: &symposium::InputEvent,
) -> symposium::OutputEvent {
    match input {
        symposium::InputEvent::PreToolUse(_) => {
            symposium::OutputEvent::empty_for(HookEvent::PreToolUse)
        }
        symposium::InputEvent::PostToolUse(post) => handle_post_tool_use(sym, post).await,
        symposium::InputEvent::UserPromptSubmit(prompt) => {
            handle_user_prompt_submit(sym, prompt).await
        }
        symposium::InputEvent::SessionStart(session) => handle_session_start(sym, session),
    }
}

/// Handle SessionStart: collect `session-start-context` from all plugins and return as context.
fn handle_session_start(
    sym: &Symposium,
    payload: &symposium::SessionStartInput,
) -> symposium::OutputEvent {
    let project_root = payload
        .cwd
        .as_deref()
        .map(std::path::Path::new)
        .filter(|p| p.join(".symposium").is_dir());
    let project_config = project_root.and_then(crate::config::ProjectConfig::load);

    let registry = crate::plugins::load_registry_with(sym, project_config.as_ref(), project_root);

    let mut context_parts: Vec<String> = Vec::new();
    for crate::plugins::ParsedPlugin { path: _, plugin } in &registry.plugins {
        if let Some(ref ctx) = plugin.session_start_context {
            context_parts.push(ctx.clone());
        }
    }

    if context_parts.is_empty() {
        symposium::OutputEvent::empty_for(HookEvent::SessionStart)
    } else {
        let context = context_parts.join("\n\n");
        symposium::OutputEvent::with_context(HookEvent::SessionStart, context)
    }
}

/// Handle PostToolUse: detect and record skill activations.
async fn handle_post_tool_use(
    sym: &Symposium,
    post: &symposium::PostToolUseInput,
) -> symposium::OutputEvent {
    let Some(ref session_id) = post.session_id else {
        return symposium::OutputEvent::empty_for(HookEvent::PostToolUse);
    };
    let Some(ref cwd_str) = post.cwd else {
        return symposium::OutputEvent::empty_for(HookEvent::PostToolUse);
    };

    let cwd = std::path::Path::new(cwd_str);
    let mut session = crate::session_state::load_session(sym, session_id);

    // Detect activation via symposium crate command (Bash tool)
    if let Some(crate_name) = detect_crate_activation_bash(post) {
        session.record_activation(&crate_name);
    }

    // Detect activation via MCP rust tool with ["crate", "<name>"]
    if let Some(crate_name) = detect_crate_activation_mcp(post) {
        session.record_activation(&crate_name);
    }

    // Detect activation via file path matching available skills
    let available = crate::workspace::compute_skills_applicable_to_workspace(sym, cwd)
        .await
        .unwrap_or_default();
    if let Some(crate_names) = detect_path_activation(&available, post) {
        for crate_name in crate_names {
            session.record_activation(&crate_name);
        }
    }

    crate::session_state::save_session(sym, session_id, &session);
    symposium::OutputEvent::empty_for(HookEvent::PostToolUse)
}

/// Detect if a Bash tool successfully ran `symposium crate <name>` or
/// `symposium crate <name>`.
///
/// Also matches the legacy `symposium crate` form for backward compatibility.
fn detect_crate_activation_bash(post: &symposium::PostToolUseInput) -> Option<String> {
    if post.tool_name != "Bash" {
        return None;
    }

    // Check for successful exit
    let exit_code = post.tool_response.get("exit_code")?.as_i64()?;
    if exit_code != 0 {
        return None;
    }

    let command = post.tool_input.get("command")?.as_str()?;

    let rest = find_crate_args(command)?;

    // First word after "crate " is the crate name (skip flags)
    let crate_name = rest.split_whitespace().find(|w| !w.starts_with('-'))?;

    if crate_name.is_empty() || crate_name == "--list" {
        return None;
    }

    Some(crate_name.to_string())
}

/// Find the arguments after a `crate ` subcommand in a command string.
///
/// Recognizes these patterns:
/// - `symposium crate <args>`
/// - `symposium crate <args>`
/// - `symposium crate <args>` (legacy)
/// - `symposium.sh crate <args>` (legacy)
///
/// The command token must be preceded by a path boundary (start, whitespace, `/`, `\`).
fn find_crate_args(command: &str) -> Option<&str> {
    let needles = [
        "symposium crate ",
        "symposium crate ",
        "symposium.sh crate ",
        "symposium crate ",
    ];
    for needle in needles {
        let mut search_from = 0;
        while let Some(pos) = command[search_from..].find(needle) {
            let abs_pos = search_from + pos;
            // Check path boundary: start-of-string, whitespace, / or \
            let boundary_ok = abs_pos == 0 || {
                let prev = command.as_bytes()[abs_pos - 1];
                prev == b' ' || prev == b'\t' || prev == b'/' || prev == b'\\'
            };
            if boundary_ok {
                return Some(&command[abs_pos + needle.len()..]);
            }
            search_from = abs_pos + 1;
        }
    }
    None
}

/// Detect if an MCP rust tool was called with ["crate", "<name>"].
fn detect_crate_activation_mcp(post: &symposium::PostToolUseInput) -> Option<String> {
    // MCP tool names include the server prefix, e.g., "mcp__symposium__rust"
    if !post.tool_name.contains("rust") {
        return None;
    }

    let args = post.tool_input.get("args")?.as_array()?;
    if args.len() >= 2 && args[0].as_str()? == "crate" {
        let name = args[1].as_str()?;
        if !name.starts_with('-') && !name.is_empty() {
            return Some(name.to_string());
        }
    }

    None
}

/// Detect if Read tool accessed a path matching an available skill directory.
fn detect_path_activation(
    available: &[crate::workspace::ApplicableSkill],
    post: &symposium::PostToolUseInput,
) -> Option<Vec<String>> {
    let target_path = match post.tool_name.as_str() {
        "Read" => post.tool_input.get("file_path")?.as_str()?,
        _ => return None,
    };

    let mut crate_names = Vec::new();
    for skill in available {
        if target_path.starts_with(&skill.skill_dir_path) {
            crate_names.push(skill.crate_name.clone());
        }
    }

    if crate_names.is_empty() {
        None
    } else {
        Some(crate_names)
    }
}

/// Handle UserPromptSubmit: scan for crate mentions and nudge about unloaded skills.
async fn handle_user_prompt_submit(
    sym: &Symposium,
    prompt_payload: &symposium::UserPromptSubmitInput,
) -> symposium::OutputEvent {
    let nudge_interval = sym.config.hooks.nudge_interval;

    if nudge_interval == 0 {
        return symposium::OutputEvent::empty_for(HookEvent::UserPromptSubmit);
    }

    let Some(ref session_id) = prompt_payload.session_id else {
        return symposium::OutputEvent::empty_for(HookEvent::UserPromptSubmit);
    };
    let Some(ref cwd_str) = prompt_payload.cwd else {
        return symposium::OutputEvent::empty_for(HookEvent::UserPromptSubmit);
    };

    let cwd = std::path::Path::new(cwd_str);

    // Compute available skills for this workspace (no caching)
    let available = crate::workspace::compute_skills_applicable_to_workspace(sym, cwd)
        .await
        .unwrap_or_default();

    if available.is_empty() {
        return symposium::OutputEvent::empty_for(HookEvent::UserPromptSubmit);
    }

    // Extract unique crate names from available skills
    let available_crate_names: std::collections::BTreeSet<String> =
        available.iter().map(|s| s.crate_name.clone()).collect();

    // Find crate mentions in the prompt
    let mentioned = extract_crate_mentions(&prompt_payload.prompt, &available_crate_names);

    if mentioned.is_empty() {
        return symposium::OutputEvent::empty_for(HookEvent::UserPromptSubmit);
    }

    // Load session, increment prompt count, compute nudges
    let mut session = crate::session_state::load_session(sym, session_id);
    session.increment_prompt_count();
    let nudge_crates = session.compute_nudges(&mentioned, nudge_interval);
    crate::session_state::save_session(sym, session_id, &session);

    if nudge_crates.is_empty() {
        return symposium::OutputEvent::empty_for(HookEvent::UserPromptSubmit);
    }

    // Format nudge message
    let mut context = String::new();
    for crate_name in &nudge_crates {
        context.push_str(&format!(
            "The `{crate_name}` crate has specialized guidance available.\n\
             To load it, run: `symposium crate {crate_name}`\n\n"
        ));
    }

    symposium::OutputEvent::with_context(
        HookEvent::UserPromptSubmit,
        context.trim_end().to_string(),
    )
}

/// Extract crate names mentioned in code-like contexts within the prompt.
///
/// Matches crate names only inside:
/// - Inline code: `foo`, `foo::Bar`
/// - Fenced code blocks
/// - Rust paths: `foo::` or `::foo`
pub fn extract_crate_mentions(
    prompt: &str,
    available_crates: &std::collections::BTreeSet<String>,
) -> Vec<String> {
    let mut found = std::collections::BTreeSet::new();

    let mut in_fenced = false;
    for line in prompt.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fenced = !in_fenced;
            continue;
        }

        if in_fenced {
            // Inside fenced code block — check entire line
            check_line_for_crates(line, available_crates, &mut found);
        } else {
            // Check inline backtick regions
            let mut rest = line;
            while let Some(start) = rest.find('`') {
                rest = &rest[start + 1..];
                if let Some(end) = rest.find('`') {
                    let code = &rest[..end];
                    check_line_for_crates(code, available_crates, &mut found);
                    rest = &rest[end + 1..];
                } else {
                    break;
                }
            }

            // Check for Rust path patterns: `foo::` or `::foo`
            check_rust_paths(line, available_crates, &mut found);
        }
    }

    found.into_iter().collect()
}

fn check_line_for_crates(
    code: &str,
    available_crates: &std::collections::BTreeSet<String>,
    found: &mut std::collections::BTreeSet<String>,
) {
    for crate_name in available_crates {
        if code.contains(crate_name.as_str()) && is_word_boundary_match(code, crate_name) {
            found.insert(crate_name.clone());
        }
    }
}

fn check_rust_paths(
    line: &str,
    available_crates: &std::collections::BTreeSet<String>,
    found: &mut std::collections::BTreeSet<String>,
) {
    for crate_name in available_crates {
        let path_prefix = format!("{crate_name}::");
        let path_suffix = format!("::{crate_name}");
        if line.contains(&path_prefix) || line.contains(&path_suffix) {
            found.insert(crate_name.clone());
        }
    }
}

fn is_word_boundary_match(text: &str, name: &str) -> bool {
    let mut start = 0;
    while let Some(pos) = text[start..].find(name) {
        let abs_pos = start + pos;
        let before_ok = abs_pos == 0 || {
            let b = text.as_bytes()[abs_pos - 1];
            !b.is_ascii_alphanumeric() && b != b'_'
        };
        let after_pos = abs_pos + name.len();
        let after_ok = after_pos >= text.len() || {
            let b = text.as_bytes()[after_pos];
            !b.is_ascii_alphanumeric() && b != b'_'
        };
        if before_ok && after_ok {
            return true;
        }
        start = abs_pos + 1;
    }
    false
}

pub enum PluginHookOutput {
    // The merged json from all plugin hooks
    Success(serde_json::Value),
    // The stderr from the first plugin hook that exited with failure
    Failure(Vec<u8>),
}

/// Dispatch plugin hooks with format routing.
///
/// Accumulates output as `serde_json::Value` in the host agent's wire format.
/// When a plugin's format matches the host agent, input/output pass through directly.
/// When formats differ, conversion goes through symposium canonical types.
///
/// Returns `Ok(json)` on success, `Err(stderr)` on exit code 2.
pub fn dispatch_plugin_hooks(
    sym: &Symposium,
    host_agent: HookAgent,
    event: HookEvent,
    sym_input: &symposium::InputEvent,
    original_input: &dyn AgentHookInput,
    prior_output: serde_json::Value,
) -> Result<serde_json::Value, Vec<u8>> {
    let plugins = crate::plugins::load_all_plugins(sym);
    let hooks = hooks_for_payload(&plugins, sym_input);

    let mut output = prior_output;

    for (plugin_name, hook) in hooks {
        tracing::info!(?plugin_name, hook = %hook.name, cmd = %hook.command, format = ?hook.format, "running plugin hook");

        // Determine stdin for the plugin based on its declared format
        let hook_agent = hook.format.as_agent();
        let temp_input: Box<dyn AgentHookInput>;
        let hook_input: &dyn AgentHookInput = if hook_agent == Some(host_agent) {
            // Same format as host — pass through original input
            original_input
        } else if let Some(target) = hook_agent {
            // Different agent format — convert symposium → target
            let handler = target.event(event);
            match handler {
                Some(h) => {
                    temp_input = h.from_symposium_input(sym_input);
                    &*temp_input
                }
                None => continue, // target agent doesn't support this event
            }
        } else {
            // Symposium format
            sym_input
        };
        let stdin_str = match hook_input.to_string() {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(?plugin_name, hook = %hook.name, error = %e, "failed to serialize hook input");
                continue;
            }
        };

        let spawn_res = Command::new("sh")
            .arg("-c")
            .arg(&hook.command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        match spawn_res {
            Ok(mut child) => {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(stdin_str.as_bytes());
                }

                let child_out = match child.wait_with_output() {
                    Ok(o) => o,
                    Err(e) => {
                        tracing::warn!(error = %e, "failed waiting for hook process");
                        continue;
                    }
                };

                tracing::info!(?child_out, "hook finished");

                match child_out.status.code() {
                    None | Some(2) => return Err(child_out.stderr),
                    Some(0) if child_out.stdout.is_empty() => continue,
                    Some(0) => {
                        // Parse output and convert to host agent format
                        let host_handler = host_agent.event(event);
                        let Some(host_h) = host_handler else { continue };

                        let host_json = if hook_agent == Some(host_agent) {
                            // Same format — parse as host agent, serialize to Value
                            match host_h.parse_output(&child_out.stdout) {
                                Ok(o) => o.to_hook_output(),
                                Err(e) => {
                                    tracing::warn!(error = %e, "failed to parse hook output");
                                    continue;
                                }
                            }
                        } else if let Some(target) = hook_agent {
                            // Different agent — parse as hook agent → symposium → host agent
                            let target_handler = target.event(event);
                            let Some(target_h) = target_handler else {
                                continue;
                            };
                            match target_h.parse_output(&child_out.stdout) {
                                Ok(hook_out) => {
                                    let sym_out = hook_out.to_symposium();
                                    let host_out = host_h.from_symposium_output(&sym_out);
                                    host_out.to_hook_output()
                                }
                                Err(e) => {
                                    tracing::warn!(error = %e, "failed to parse hook output");
                                    continue;
                                }
                            }
                        } else {
                            // Symposium format — parse as symposium → host agent
                            match serde_json::from_slice::<serde_json::Value>(&child_out.stdout) {
                                Ok(v) => {
                                    // Try to parse as symposium OutputEvent
                                    if let Ok(sym_out) =
                                        serde_json::from_value::<symposium::OutputEvent>(v.clone())
                                    {
                                        let host_out = host_h.from_symposium_output(&sym_out);
                                        host_out.to_hook_output()
                                    } else {
                                        v // fallback: use raw JSON
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!(error = %e, "failed to parse hook output");
                                    continue;
                                }
                            }
                        };

                        merge(&mut output, host_json);
                    }
                    Some(code) => {
                        tracing::warn!(
                            exit_code = code,
                            "plugin hook exited with non-zero (continuing)"
                        );
                    }
                }
            }
            Err(e) => tracing::warn!(error = %e, "failed to spawn hook command"),
        }
    }

    Ok(output)
}

/// Recursively merge two JSON objects, with `b` taking precedence over `a`.
/// Fields with null values in `b` will delete the corresponding field in `a`.
/// Fields not present in `b` will be left unchanged in `a`.
pub fn merge(a: &mut serde_json::Value, b: serde_json::Value) {
    if let serde_json::Value::Object(a) = a {
        if let serde_json::Value::Object(b) = b {
            for (k, v) in b {
                if v.is_null() {
                    a.remove(&k);
                } else {
                    merge(a.entry(k).or_insert(serde_json::Value::Null), v);
                }
            }

            return;
        }
    }

    *a = b;
}

/// Return all hooks (with their plugin name) that match the event in `payload`.
fn hooks_for_payload(
    plugins: &[crate::plugins::ParsedPlugin],
    input: &symposium::InputEvent,
) -> Vec<(String, crate::plugins::Hook)> {
    tracing::debug!(?input);

    let mut out = Vec::new();

    for ParsedPlugin { path: _, plugin } in plugins {
        let name = plugin.name.clone();
        for hook in &plugin.hooks {
            tracing::debug!(?hook);
            if hook.event != input.event() {
                continue;
            }
            if let Some(matcher) = &hook.matcher {
                if !input.matches_matcher(matcher) {
                    tracing::info!(
                        ?input,
                        ?matcher,
                        "skipping hook due to non-matching matcher"
                    );
                    continue;
                }
            }
            out.push((name.clone(), hook.clone()));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn builtin_pre_tool_use_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(tmp.path());
        let input = symposium::InputEvent::PreToolUse(symposium::PreToolUseInput {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::Value::default(),
            session_id: None,
            cwd: None,
        });
        let output = dispatch_builtin(&sym, &input).await;
        assert!(output.additional_context().is_none());
    }

    #[tokio::test]
    async fn builtin_post_tool_use_returns_empty_for_now() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(tmp.path());
        let input = symposium::InputEvent::PostToolUse(symposium::PostToolUseInput {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({"command": "ls"}),
            tool_response: serde_json::json!({"stdout": "file.rs"}),
            session_id: Some("test-session".to_string()),
            cwd: Some("/tmp".to_string()),
        });
        let output = dispatch_builtin(&sym, &input).await;
        assert!(output.additional_context().is_none());
    }

    #[tokio::test]
    async fn builtin_user_prompt_submit_returns_empty_for_now() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(tmp.path());
        let input = symposium::InputEvent::UserPromptSubmit(symposium::UserPromptSubmitInput {
            prompt: "Use tokio for async".to_string(),
            session_id: Some("test-session".to_string()),
            cwd: Some("/tmp".to_string()),
        });
        let output = dispatch_builtin(&sym, &input).await;
        assert!(output.additional_context().is_none());
    }

    #[test]
    fn symposium_output_serializes_with_additional_context() {
        let output = symposium::OutputEvent::with_context(
            HookEvent::UserPromptSubmit,
            "Load tokio guidance".to_string(),
        );
        let ctx = output.additional_context().unwrap();
        assert_eq!(ctx, "Load tokio guidance");
    }

    #[test]
    fn symposium_output_empty_has_no_context() {
        let output = symposium::OutputEvent::empty_for(HookEvent::PreToolUse);
        assert!(output.additional_context().is_none());
    }

    // --- Activation detection unit tests ---

    #[test]
    fn detect_bash_crate_activation() {
        let post = symposium::PostToolUseInput {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({"command": "symposium crate tokio"}),
            tool_response: serde_json::json!({"exit_code": 0, "stdout": "..."}),
            session_id: Some("s1".to_string()),
            cwd: Some("/tmp".to_string()),
        };
        assert_eq!(
            detect_crate_activation_bash(&post),
            Some("tokio".to_string())
        );
    }

    #[test]
    fn detect_bash_crate_activation_symposium_hyphen() {
        let post = symposium::PostToolUseInput {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({"command": "symposium crate tokio"}),
            tool_response: serde_json::json!({"exit_code": 0, "stdout": "..."}),
            session_id: Some("s1".to_string()),
            cwd: Some("/tmp".to_string()),
        };
        assert_eq!(
            detect_crate_activation_bash(&post),
            Some("tokio".to_string())
        );
    }

    #[test]
    fn detect_bash_crate_activation_legacy_symposium() {
        let post = symposium::PostToolUseInput {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({"command": "symposium crate tokio"}),
            tool_response: serde_json::json!({"exit_code": 0, "stdout": "..."}),
            session_id: Some("s1".to_string()),
            cwd: Some("/tmp".to_string()),
        };
        assert_eq!(
            detect_crate_activation_bash(&post),
            Some("tokio".to_string())
        );
    }

    #[test]
    fn detect_bash_crate_activation_with_version() {
        let post = symposium::PostToolUseInput {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({"command": "symposium crate serde --version 1.0"}),
            tool_response: serde_json::json!({"exit_code": 0}),
            session_id: Some("s1".to_string()),
            cwd: Some("/tmp".to_string()),
        };
        assert_eq!(
            detect_crate_activation_bash(&post),
            Some("serde".to_string())
        );
    }

    #[test]
    fn detect_bash_crate_list_not_activation() {
        let post = symposium::PostToolUseInput {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({"command": "symposium crate --list"}),
            tool_response: serde_json::json!({"exit_code": 0}),
            session_id: Some("s1".to_string()),
            cwd: Some("/tmp".to_string()),
        };
        assert_eq!(detect_crate_activation_bash(&post), None);
    }

    #[test]
    fn detect_bash_failed_not_activation() {
        let post = symposium::PostToolUseInput {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({"command": "symposium crate tokio"}),
            tool_response: serde_json::json!({"exit_code": 1}),
            session_id: Some("s1".to_string()),
            cwd: Some("/tmp".to_string()),
        };
        assert_eq!(detect_crate_activation_bash(&post), None);
    }

    #[test]
    fn detect_bash_crate_activation_with_path_prefix() {
        let post = symposium::PostToolUseInput {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({"command": "/home/user/.local/bin/symposium crate serde"}),
            tool_response: serde_json::json!({"exit_code": 0}),
            session_id: Some("s1".to_string()),
            cwd: Some("/tmp".to_string()),
        };
        assert_eq!(
            detect_crate_activation_bash(&post),
            Some("serde".to_string())
        );
    }

    #[test]
    fn detect_mcp_crate_activation() {
        let post = symposium::PostToolUseInput {
            tool_name: "mcp__symposium__rust".to_string(),
            tool_input: serde_json::json!({"args": ["crate", "tokio"]}),
            tool_response: serde_json::json!({"output": "..."}),
            session_id: Some("s1".to_string()),
            cwd: Some("/tmp".to_string()),
        };
        assert_eq!(
            detect_crate_activation_mcp(&post),
            Some("tokio".to_string())
        );
    }

    #[test]
    fn detect_mcp_crate_list_not_activation() {
        let post = symposium::PostToolUseInput {
            tool_name: "mcp__symposium__rust".to_string(),
            tool_input: serde_json::json!({"args": ["crate", "--list"]}),
            tool_response: serde_json::json!({"output": "..."}),
            session_id: Some("s1".to_string()),
            cwd: Some("/tmp".to_string()),
        };
        assert_eq!(detect_crate_activation_mcp(&post), None);
    }

    // --- Crate mention detection tests ---

    fn crate_set(names: &[&str]) -> std::collections::BTreeSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn extract_mentions_inline_backticks() {
        let available = crate_set(&["tokio", "serde", "log"]);
        let prompt = "I need to use `tokio` for async and `serde` for serialization";
        let found = extract_crate_mentions(prompt, &available);
        assert_eq!(found, vec!["serde", "tokio"]);
    }

    #[test]
    fn extract_mentions_fenced_code_block() {
        let available = crate_set(&["tokio", "serde"]);
        let prompt = "Here's the code:\n```rust\nuse tokio::runtime;\n```";
        let found = extract_crate_mentions(prompt, &available);
        assert_eq!(found, vec!["tokio"]);
    }

    #[test]
    fn extract_mentions_rust_path() {
        let available = crate_set(&["tokio", "serde"]);
        let prompt = "The function uses tokio::spawn to run tasks";
        let found = extract_crate_mentions(prompt, &available);
        assert_eq!(found, vec!["tokio"]);
    }

    #[test]
    fn extract_mentions_no_false_positive_plain_text() {
        let available = crate_set(&["log", "time"]);
        let prompt = "I want to log in to the server and it's time to deploy";
        let found = extract_crate_mentions(prompt, &available);
        // "log" and "time" in plain text should NOT match
        assert!(found.is_empty());
    }

    #[test]
    fn extract_mentions_word_boundary() {
        let available = crate_set(&["serde"]);
        let prompt = "Check the `serde_json` crate";
        let found = extract_crate_mentions(prompt, &available);
        // "serde" inside "serde_json" should NOT match (underscore is word char)
        assert!(found.is_empty());
    }

    #[test]
    fn extract_mentions_exact_backtick() {
        let available = crate_set(&["serde"]);
        let prompt = "Use `serde` for this";
        let found = extract_crate_mentions(prompt, &available);
        assert_eq!(found, vec!["serde"]);
    }

    #[test]
    fn detect_mcp_start_not_activation() {
        let post = symposium::PostToolUseInput {
            tool_name: "mcp__symposium__rust".to_string(),
            tool_input: serde_json::json!({"args": ["start"]}),
            tool_response: serde_json::json!({"output": "..."}),
            session_id: Some("s1".to_string()),
            cwd: Some("/tmp".to_string()),
        };
        assert_eq!(detect_crate_activation_mcp(&post), None);
    }
}
