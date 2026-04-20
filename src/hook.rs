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
/// Core hook pipeline: sync → parse → builtin → plugins → serialize.
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

        // Auto-sync: install applicable skills into agent dirs (non-fatal).
        let fallback_cwd = std::env::current_dir().unwrap_or_default();
        run_auto_sync(sym, &sym_input, &fallback_cwd).await;

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
pub async fn run(sym: &Symposium, agent: HookAgent, event: HookEvent) -> ExitCode {
    tracing::debug!("Running hook listener for agent {agent:?} and event {event:?}");

    let mut input = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut input) {
        tracing::warn!(?event, error = %e, "failed to read hook stdin");
        return ExitCode::SUCCESS;
    }
    tracing::debug!(?input);

    match execute_hook(sym, agent, event, &input).await {
        Ok(bytes) => {
            write_hook_trace(agent, event, &input, &bytes);
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

/// If `SYMPOSIUM_HOOK_TRACE` is set to a file path, append a JSONL entry.
/// Used for integration testing to check what hooks occur when we invoke the agent.
fn write_hook_trace(agent: HookAgent, event: HookEvent, input: &str, output: &[u8]) {
    let Some(path) = std::env::var_os("SYMPOSIUM_HOOK_TRACE") else {
        return;
    };

    let input_val: serde_json::Value =
        serde_json::from_str(input).unwrap_or(serde_json::Value::Null);
    let output_val: serde_json::Value =
        serde_json::from_slice(output).unwrap_or(serde_json::Value::Null);

    let entry = serde_json::json!({
        "event": event,
        "agent": agent,
        "input": input_val,
        "output": output_val,
    });

    use std::fs::OpenOptions;
    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(mut f) => {
            let mut line = serde_json::to_string(&entry).unwrap();
            line.push('\n');
            if let Err(e) = f.write_all(line.as_bytes()) {
                tracing::warn!(error = %e, "failed to write hook trace");
            }
        }
        Err(e) => tracing::warn!(error = %e, "failed to open hook trace file"),
    }
}

/// Run sync if we're in a workspace directory and auto-sync is enabled. Non-fatal.
async fn run_auto_sync(
    sym: &Symposium,
    input: &symposium::InputEvent,
    fallback_cwd: &std::path::Path,
) {
    if !sym.config.auto_sync {
        return;
    }
    let cwd = match input.cwd() {
        Some(s) => std::path::PathBuf::from(s),
        None => fallback_cwd.to_path_buf(),
    };
    let out = crate::output::Output::quiet();
    if let Err(e) = crate::sync::sync(sym, &cwd, &out).await {
        tracing::warn!(error = %e, "auto-sync during hook failed (continuing)");
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

/// Handle SessionStart: return empty (no session-start-context support).
fn handle_session_start(
    _sym: &Symposium,
    _payload: &symposium::SessionStartInput,
) -> symposium::OutputEvent {
    symposium::OutputEvent::empty_for(HookEvent::SessionStart)
}

/// Handle PostToolUse: no-op for now.
async fn handle_post_tool_use(
    _sym: &Symposium,
    _post: &symposium::PostToolUseInput,
) -> symposium::OutputEvent {
    symposium::OutputEvent::empty_for(HookEvent::PostToolUse)
}

/// Handle UserPromptSubmit: no-op for now.
async fn handle_user_prompt_submit(
    _sym: &Symposium,
    _prompt_payload: &symposium::UserPromptSubmitInput,
) -> symposium::OutputEvent {
    symposium::OutputEvent::empty_for(HookEvent::UserPromptSubmit)
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
}
