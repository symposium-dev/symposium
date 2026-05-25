use std::{
    env,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
};

use crate::installation::{install_requirement, resolve_runnable};
use crate::plugins::{HookFormat, Installation};
use crate::{
    config::Symposium,
    hook_schema::{AgentHookInput, symposium},
    plugins::ParsedPlugin,
};
use crate::{
    crate_sources::workspace_crates,
    hook_schema::symposium::{OutputEvent, SessionStartInput},
    plugins::load_registry,
    subcommand_dispatch::applicable_subcommands,
};
use symposium_install::Runnable;

/// A hook prepared for dispatch — installation names looked up to concrete
/// `Installation` entries, so the dispatch loop never has to scan the plugin's
/// installations list again.
struct ResolvedHook {
    plugin_name: String,
    hook_name: String,
    format: HookFormat,
    requirements: Vec<Installation>,
    command: Installation,
    /// Hook-level `executable` override. Validation guarantees that if set,
    /// the command installation does not also set executable/script.
    hook_executable: Option<String>,
    /// Hook-level `script` override.
    hook_script: Option<String>,
    args: Vec<String>,
}

impl ResolvedHook {
    fn build(parsed_plugin: &ParsedPlugin, hook: &crate::plugins::Hook) -> anyhow::Result<Self> {
        let installations = &parsed_plugin.plugin.installations;
        let lookup = |name: &str| -> anyhow::Result<Installation> {
            installations
                .iter()
                .find(|i| i.name == name)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("installation `{name}` not found in plugin"))
        };

        let command = lookup(&hook.command)?;
        let requirements = hook
            .requirements
            .iter()
            .map(|name| lookup(name))
            .collect::<anyhow::Result<Vec<_>>>()?;

        Ok(Self {
            plugin_name: parsed_plugin.plugin.name.clone(),
            hook_name: hook.name.clone(),
            format: hook.format.clone(),
            requirements,
            command,
            hook_executable: hook.executable.clone(),
            hook_script: hook.script.clone(),
            args: hook.args.clone(),
        })
    }
}

enum SpawnSpec {
    Exec { path: PathBuf, args: Vec<String> },
    Script { path: PathBuf, args: Vec<String> },
}

async fn build_spawn_spec(sym: &Symposium, hook: &ResolvedHook) -> anyhow::Result<SpawnSpec> {
    let label = format!("hook `{}`", hook.hook_name);
    let runnable = resolve_runnable(
        sym,
        &hook.command,
        hook.hook_executable.as_deref(),
        hook.hook_script.as_deref(),
        &label,
    )
    .await?;

    Ok(match runnable {
        Runnable::Exec(path) => SpawnSpec::Exec {
            path,
            args: hook.args.clone(),
        },
        Runnable::Script(path) => SpawnSpec::Script {
            path,
            args: hook.args.clone(),
        },
        _ => anyhow::bail!("hook `{}`: unsupported runnable kind", hook.hook_name),
    })
}

fn spawn_from_spec(spec: SpawnSpec) -> std::io::Result<std::process::Child> {
    match spec {
        SpawnSpec::Script { path, args } => Command::new("sh")
            .arg(path)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn(),
        SpawnSpec::Exec { path, args } => Command::new(path)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn(),
    }
}

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
        let builtin_agent_output = handler.translate_output(&builtin_sym_output);
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
        .await
        .map_err(|stderr| {
            anyhow::anyhow!("plugin blocked: {}", String::from_utf8_lossy(&stderr))
        })?;

        let serialized = handler.serialize_output(&final_output);
        tracing::trace!(output_len = serialized.len(), "hook output serialized");
        Ok(serialized)
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
    tracing::trace!(?input, "hook stdin");

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
///
/// Uses per-workspace state to skip sync when `Cargo.lock` hasn't changed
/// since the last successful sync, avoiding expensive `cargo metadata` calls
/// on every hook invocation.
async fn run_auto_sync(
    sym: &Symposium,
    input: &symposium::InputEvent,
    fallback_cwd: &std::path::Path,
) {
    if !sym.config.auto_sync {
        tracing::debug!("auto-sync disabled, skipping");
        return;
    }

    let cwd = match input.cwd() {
        Some(s) => std::path::PathBuf::from(s),
        None => fallback_cwd.to_path_buf(),
    };

    // Find workspace root via `cargo locate-project` (fast, no dep resolution).
    // If we can't find one, fall through to full sync which will
    // use cargo metadata.
    let workspace_root = crate::workspace_state::find_workspace_root(sym, &cwd);

    if let Some(ref root) = workspace_root {
        let state = crate::workspace_state::WorkspaceState::load(sym, root);
        if state.sync_is_fresh(root) {
            tracing::debug!("auto-sync skipped: Cargo.lock unchanged since last sync");
            return;
        }
    }

    tracing::debug!("auto-sync running");
    let out = crate::output::Output::quiet();
    if let Err(e) = crate::sync::sync(sym, &cwd, &out).await {
        tracing::warn!(error = %e, "auto-sync during hook failed (continuing)");
        return;
    }

    // Record successful sync. If we didn't find the root earlier,
    // try again now (sync may have created Cargo.lock).
    let root = workspace_root.or_else(|| crate::workspace_state::find_workspace_root(sym, &cwd));
    if let Some(ref root) = root {
        let mut state = crate::workspace_state::WorkspaceState::load(sym, root);
        state.record_sync(root);
        state.workspace_root = Some(root.clone());
        state.save(sym, root);
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
        _ => symposium::OutputEvent::empty_for(HookEvent::PreToolUse),
    }
}

/// Handle SessionStart: orient the agent toward crate-aware tooling and, when due, nudge the
/// user to update. The two fragments are computed independently -- the discovery hint is never gated
/// behind the update-check throttle -- then joined into a single context block.
fn handle_session_start(sym: &Symposium, payload: &SessionStartInput) -> OutputEvent {
    let cwd = payload
        .cwd
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap_or_default());

    let fragments = [discovery_hint(sym, &cwd), update_nudge(sym)]
        .into_iter()
        .flatten()
        .collect::<Vec<String>>();

    if fragments.is_empty() {
        OutputEvent::empty_for(HookEvent::SessionStart)
    } else {
        OutputEvent::with_context(HookEvent::SessionStart, fragments.join("\n\n"))
    }
}

/// Suggest `cargo agents --help` when the active workspace exposes crate-aware plugin subcommands.
/// Reuses the help renderer's `applicable_subcommands`, so the hint fires only when there is actually something to discover; `None` otherwise.
fn discovery_hint(sym: &Symposium, cwd: &Path) -> Option<String> {
    let registry = load_registry(sym);
    let deps = workspace_crates(cwd)
        .into_iter()
        .map(|wcrt| (wcrt.name, wcrt.version))
        .collect::<Vec<_>>();

    let any_subcommand = applicable_subcommands(&registry, &deps).next().is_some();

    any_subcommand.then(|| {
        "This project has crate-aware tools available via `cargo agents`. \
         Run `cargo agents --help` to list them before working with the Rust code. \
         Only use tools under the 'Commands for agents' section unless the user \
         explicitly asks you to run one from 'Commands for humans'."
            .to_string()
    })
}

/// Nudge the user to update. Gated by `auto-update = \"warn\"`, the 25h throttle,
/// and a newer published version on the registry; `None` otherwise.
fn update_nudge(sym: &Symposium) -> Option<String> {
    use crate::config::AutoUpdate;
    use crate::state::CURRENT_VERSION;

    if sym.config.auto_update != AutoUpdate::Warn {
        return None;
    }
    if !crate::state::should_check_for_update(sym.config_dir()) {
        return None;
    }
    crate::state::record_update_check(sym.config_dir());

    let latest = crate::self_update::check_upgrade(sym).ok()??;
    Some(format!(
        "symposium {latest} is available (current: {CURRENT_VERSION}). \
         Run `cargo agents self-update` to upgrade."
    ))
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
pub async fn dispatch_plugin_hooks(
    sym: &Symposium,
    host_agent: HookAgent,
    event: HookEvent,
    sym_input: &symposium::InputEvent,
    original_input: &dyn AgentHookInput,
    prior_output: serde_json::Value,
) -> Result<serde_json::Value, Vec<u8>> {
    let plugins = crate::plugins::load_all_plugins(sym);
    let hooks = dispatched_hooks_for_payload(&plugins, sym_input, host_agent);

    let mut output = prior_output;

    for hook in hooks {
        tracing::info!(
            plugin = %hook.plugin_name,
            hook = %hook.hook_name,
            format = ?hook.format,
            "running plugin hook"
        );

        // Acquire each requirement (best-effort).
        for requirement in &hook.requirements {
            if let Err(e) = install_requirement(sym, requirement).await {
                tracing::error!(name = %requirement.name, error = %e, "failed to install hook requirement");
            }
        }

        // Determine stdin for the plugin based on its declared format.
        // After format selection, the only two cases are:
        // - native (matches host agent) → pass through original input
        // - symposium → deliver canonical format
        let hook_agent = hook.format.as_agent();
        let hook_input: &dyn AgentHookInput = if hook_agent == Some(host_agent) {
            original_input
        } else {
            sym_input
        };
        let stdin_str = match hook_input.to_string() {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(plugin = %hook.plugin_name, hook = %hook.hook_name, error = %e, "failed to serialize hook input");
                continue;
            }
        };

        let spawn_res = match build_spawn_spec(sym, &hook).await {
            Ok(spec) => spawn_from_spec(spec),
            Err(e) => {
                tracing::warn!(error = %e, "failed to prepare hook command");
                continue;
            }
        };

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

                tracing::trace!(?child_out, "hook finished");

                match child_out.status.code() {
                    None | Some(2) => return Err(child_out.stderr),
                    Some(0) if child_out.stdout.is_empty() => continue,
                    Some(0) => {
                        // Parse output and convert to host agent format.
                        // Two cases: native (same as host) or symposium.
                        let host_handler = host_agent.event(event);
                        let Some(host_h) = host_handler else { continue };

                        let host_json = if hook_agent == Some(host_agent) {
                            // Native format — parse as host agent output
                            match host_h.parse_output(&child_out.stdout) {
                                Ok(o) => o.to_hook_output(),
                                Err(e) => {
                                    tracing::warn!(error = %e, "failed to parse hook output");
                                    continue;
                                }
                            }
                        } else {
                            // Symposium format — parse and convert to host agent
                            match serde_json::from_slice::<serde_json::Value>(&child_out.stdout) {
                                Ok(v) => {
                                    if let Ok(sym_out) =
                                        serde_json::from_value::<symposium::OutputEvent>(v.clone())
                                    {
                                        let host_out = host_h.translate_output(&sym_out);
                                        host_out.to_hook_output()
                                    } else {
                                        v
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
    if let serde_json::Value::Object(a) = a
        && let serde_json::Value::Object(b) = b
    {
        for (k, v) in b {
            if v.is_null() {
                a.remove(&k);
            } else {
                merge(a.entry(k).or_insert(serde_json::Value::Null), v);
            }
        }

        return;
    }

    *a = b;
}

/// Match plugin hooks against the incoming event, selecting at most one hook
/// per plugin based on format priority:
/// 1. A hook whose format matches the host agent (native fidelity).
/// 2. A symposium-format hook (portable fallback).
/// 3. Otherwise, nothing fires for that plugin.
///
/// The resulting `ResolvedHook`s are ready to dispatch without further plugin
/// lookups.
fn dispatched_hooks_for_payload(
    plugins: &[ParsedPlugin],
    input: &symposium::InputEvent,
    host_agent: HookAgent,
) -> Vec<ResolvedHook> {
    tracing::trace!(?input, "matching hooks for payload");

    let mut out = Vec::new();

    for parsed_plugin in plugins {
        let mut native_match: Option<&crate::plugins::Hook> = None;
        let mut symposium_match: Option<&crate::plugins::Hook> = None;

        for hook in &parsed_plugin.plugin.hooks {
            if hook.event != input.event() {
                continue;
            }
            if let Some(matcher) = &hook.matcher
                && !input.matches_matcher(matcher)
            {
                continue;
            }

            match hook.format.as_agent() {
                Some(agent) if agent == host_agent => {
                    native_match = Some(hook);
                }
                None => {
                    // format = "symposium"
                    symposium_match = Some(hook);
                }
                Some(_) => {
                    // Different agent format — does not fire on this agent.
                }
            }
        }

        let selected = native_match.or(symposium_match);
        if let Some(hook) = selected {
            match ResolvedHook::build(parsed_plugin, hook) {
                Ok(dispatched) => out.push(dispatched),
                Err(e) => {
                    tracing::warn!(
                        plugin = %parsed_plugin.plugin.name,
                        hook = %hook.name,
                        error = %e,
                        "failed to resolve hook for dispatch"
                    );
                }
            }
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
        let input = symposium::InputEvent::PreToolUse(symposium::PreToolUseInput::new(
            "Bash".to_string(),
            serde_json::Value::default(),
            None,
            None,
        ));
        let output = dispatch_builtin(&sym, &input).await;
        assert!(output.additional_context().is_none());
    }

    #[tokio::test]
    async fn builtin_post_tool_use_returns_empty_for_now() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(tmp.path());
        let input = symposium::InputEvent::PostToolUse(symposium::PostToolUseInput::new(
            "Bash".to_string(),
            serde_json::json!({"command": "ls"}),
            serde_json::json!({"stdout": "file.rs"}),
            Some("test-session".to_string()),
            Some("/tmp".to_string()),
        ));
        let output = dispatch_builtin(&sym, &input).await;
        assert!(output.additional_context().is_none());
    }

    #[tokio::test]
    async fn builtin_user_prompt_submit_returns_empty_for_now() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(tmp.path());
        let input = symposium::InputEvent::UserPromptSubmit(symposium::UserPromptSubmitInput::new(
            "Use tokio for async".to_string(),
            Some("test-session".to_string()),
            Some("/tmp".to_string()),
        ));
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
