use std::{
    io::{Read, Write},
    path::PathBuf,
    process::{Command, ExitCode, Stdio},
};

use symposium_install::Runnable;

use crate::installation::{
    AcquiredInstallation, AcquiredRunnable, acquire_installation, resolve_runnable,
};
use crate::plugins::{HookFormat, Installation};
use crate::{
    config::Symposium,
    hook_schema::{AgentHookInput, symposium},
    plugins::ParsedPlugin,
};
use crate::{
    help_render::{AGENTS_HEADING, HUMANS_HEADING},
    hook_schema::symposium::{OutputEvent, SessionStartInput},
    plugins::load_registry,
    subcommand_dispatch::applicable_subcommands,
};
use symposium_sdk::workspace::WorkspaceDeps;

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
        let plugin = &parsed_plugin.plugin;
        let lookup = |name: &str| -> anyhow::Result<Installation> {
            plugin
                .get_installation(name)
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

/// Sanitize an installation name for use as part of an env var name.
/// Replaces non-alphanumeric chars with underscore.
fn env_safe(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// Build the env vars (including augmented PATH) for the spawn.
///
/// Iterates `acquired` in order; the parent directory of each absolute
/// `runnable` is prepended to `$PATH` so later entries take precedence over
/// earlier ones (the command's own parent ends up first since it's pushed
/// last and prepended).
fn build_env(acquired: &[AcquiredInstallation]) -> Vec<(String, String)> {
    let mut env = Vec::new();
    let mut path_prefix: Vec<String> = Vec::new();

    for a in acquired {
        let key = env_safe(&a.name);
        if let Some(base) = &a.base {
            env.push((format!("SYMPOSIUM_DIR_{key}"), base.display().to_string()));
        }
        if let Some(
            AcquiredRunnable::ResolvedScript { path, .. } | AcquiredRunnable::ResolvedExec { path },
        ) = &a.runnable
        {
            env.push((format!("SYMPOSIUM_{key}"), path.display().to_string()));
            if let Some(parent) = path.parent() {
                let parent_str = parent.display().to_string();
                if !parent_str.is_empty() {
                    path_prefix.push(parent_str);
                }
            }
        }
    }

    // Command was pushed last into `acquired`; reverse so its bin dir wins
    // PATH lookup over requirements' bin dirs.
    path_prefix.reverse();

    if !path_prefix.is_empty() {
        let existing = std::env::var("PATH").unwrap_or_default();
        let joined = if existing.is_empty() {
            path_prefix.join(":")
        } else {
            format!("{}:{}", path_prefix.join(":"), existing)
        };
        env.push(("PATH".to_string(), joined));
    }

    env
}

enum SpawnSpec {
    Exec {
        path: PathBuf,
        args: Vec<String>,
        env: Vec<(String, String)>,
    },
    Script {
        path: PathBuf,
        args: Vec<String>,
        env: Vec<(String, String)>,
    },
}

async fn build_spawn_spec(sym: &Symposium, hook: &ResolvedHook) -> anyhow::Result<SpawnSpec> {
    // Acquire requirements first so the command's PATH sees them.
    let mut acquired: Vec<AcquiredInstallation> = Vec::new();
    for requirement in &hook.requirements {
        match acquire_installation(sym, requirement, None, None).await {
            Ok(a) => acquired.push(a),
            Err(e) => {
                tracing::error!(
                    name = %requirement.name,
                    error = %e,
                    "failed to install hook requirement"
                );
            }
        }
    }

    let command_acquired = acquire_installation(
        sym,
        &hook.command,
        hook.hook_executable.as_deref(),
        hook.hook_script.as_deref(),
    )
    .await?;

    acquired.push(command_acquired.clone());
    let env = build_env(&acquired);

    let label = format!("hook `{}`", hook.hook_name);
    let runnable = resolve_runnable(command_acquired, &label)?;

    Ok(match runnable {
        Runnable::Script(path) => SpawnSpec::Script {
            path,
            args: hook.args.clone(),
            env,
        },
        Runnable::Exec(path) => SpawnSpec::Exec {
            path,
            args: hook.args.clone(),
            env,
        },
    })
}

fn spawn_from_spec(spec: SpawnSpec) -> std::io::Result<std::process::Child> {
    match spec {
        SpawnSpec::Script { path, args, env } => {
            let mut cmd = Command::new("sh");
            cmd.arg(path).args(args);
            for (k, v) in env {
                cmd.env(k, v);
            }
            cmd.stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        }
        SpawnSpec::Exec { path, args, env } => {
            let mut cmd = Command::new(path);
            cmd.args(args);
            for (k, v) in env {
                cmd.env(k, v);
            }
            cmd.stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        }
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

        // Create a shared WorkspaceDeps for the entire hook invocation.
        // Auto-sync populates it if it runs; later stages reuse the cached result.
        let fallback_cwd = std::env::current_dir().unwrap_or_default();
        let cwd = match sym_input.cwd() {
            Some(s) => PathBuf::from(s),
            None => fallback_cwd,
        };
        let mut deps = sym.workspace_deps(&cwd);

        // Auto-sync: install applicable skills into agent dirs (non-fatal).
        run_auto_sync(sym, &mut deps).await;

        // Builtin dispatch → symposium output → host agent output as Value
        let builtin_sym_output = dispatch_builtin(sym, &sym_input, &mut deps).await;
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
            &mut deps,
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
///
/// If sync runs, `deps` gets populated — later hook stages reuse the result.
async fn run_auto_sync(sym: &Symposium, deps: &mut WorkspaceDeps) {
    if !sym.config.auto_sync {
        tracing::debug!("auto-sync disabled, skipping");
        return;
    }

    let cwd = deps.cwd().to_path_buf();

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
    if let Err(e) = crate::sync::sync(sym, deps).await {
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
    deps: &mut WorkspaceDeps,
) -> symposium::OutputEvent {
    match input {
        symposium::InputEvent::PreToolUse(_) => {
            symposium::OutputEvent::empty_for(HookEvent::PreToolUse)
        }
        symposium::InputEvent::PostToolUse(post) => handle_post_tool_use(sym, post).await,
        symposium::InputEvent::UserPromptSubmit(prompt) => {
            handle_user_prompt_submit(sym, prompt).await
        }
        symposium::InputEvent::SessionStart(session) => handle_session_start(sym, session, deps),
        _ => symposium::OutputEvent::empty_for(HookEvent::PreToolUse),
    }
}

/// Handle SessionStart: orient the agent toward crate-aware tooling and, when due, nudge the
/// user to update. The two fragments are computed independently -- the discovery hint is never gated
/// behind the update-check throttle -- then joined into a single context block.
fn handle_session_start(
    sym: &Symposium,
    _payload: &SessionStartInput,
    deps: &mut WorkspaceDeps,
) -> OutputEvent {
    let fragments = [discovery_hint(sym, deps), update_nudge(sym)]
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
fn discovery_hint(sym: &Symposium, deps: &mut WorkspaceDeps) -> Option<String> {
    let registry = load_registry(sym);
    let pairs = crate::crate_sources::crate_pairs(deps.crates());

    let any_subcommand = !applicable_subcommands(&registry, &pairs).is_empty();

    any_subcommand.then(|| {
        format!(
            "This project has crate-aware tools available via `cargo agents`. \
             Run `cargo agents --help` to list them before working with the Rust code. \
             Only use tools under the '{AGENTS_HEADING}' section unless the user \
             explicitly asks you to run one from '{HUMANS_HEADING}'."
        )
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
    deps: &mut WorkspaceDeps,
) -> Result<serde_json::Value, Vec<u8>> {
    let plugins = crate::plugins::load_all_plugins(sym);

    // Resolving the workspace means running cargo, so only do it when some
    // plugin's hook gating actually references a concrete crate (a `crate(*)`
    // wildcard or env/shell/path predicate never needs the crate graph).
    let pairs = if plugins
        .iter()
        .any(|p| p.plugin.hooks_need_crate_resolution())
    {
        crate::crate_sources::crate_pairs(deps.crates())
    } else {
        Vec::new()
    };
    let mut ctx = crate::predicate::PredicateContext::new(&pairs);
    let hooks = dispatched_hooks_for_payload(&plugins, sym_input, host_agent, &mut ctx);

    let mut output = prior_output;

    for hook in hooks {
        tracing::info!(
            plugin = %hook.plugin_name,
            hook = %hook.hook_name,
            format = ?hook.format,
            "running plugin hook"
        );

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

                let exit_code = child_out.status.code();
                tracing::debug!(
                    report = %crate::report::ReportEvent::HookDispatched {
                        plugin: hook.plugin_name.clone(),
                        hook: hook.hook_name.clone(),
                        exit_code,
                        error: None,
                    },
                );
                match exit_code {
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
            Err(e) => {
                tracing::debug!(
                    report = %crate::report::ReportEvent::HookDispatched {
                        plugin: hook.plugin_name.clone(),
                        hook: hook.hook_name.clone(),
                        exit_code: None,
                        error: Some(e.to_string()),
                    },
                );
                tracing::warn!(error = %e, "failed to spawn hook command");
            }
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
    ctx: &mut crate::predicate::PredicateContext,
) -> Vec<ResolvedHook> {
    tracing::trace!(?input, "matching hooks for payload");

    let mut out = Vec::new();

    for parsed_plugin in plugins {
        ctx.set_source_provenance(parsed_plugin.source_provenance.clone());
        // Plugin-level predicates gate every hook in the plugin. Evaluated once
        // per plugin per dispatch — keep them cheap.
        if !parsed_plugin.plugin.applies(ctx) {
            tracing::debug!(
                plugin = %parsed_plugin.plugin.name,
                "plugin predicates failed, skipping hooks"
            );
            continue;
        }

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
            // Hook-level predicates are evaluated at dispatch so they pick up
            // live state (file present, tool installed, crate present, …).
            if !hook.predicates.evaluate(ctx) {
                tracing::debug!(
                    report = %crate::report::ReportEvent::HookConsidered {
                        plugin: parsed_plugin.plugin.name.clone(),
                        hook: hook.name.clone(),
                        event: format!("{:?}", input.event()),
                        selected: false,
                        format: Some(format!("{:?}", hook.format)),
                        reason: Some("hook predicates not satisfied".into()),
                    },
                );
                continue;
            }
            tracing::debug!(
                report = %crate::report::ReportEvent::HookConsidered {
                    plugin: parsed_plugin.plugin.name.clone(),
                    hook: hook.name.clone(),
                    event: format!("{:?}", input.event()),
                    selected: true,
                    format: Some(format!("{:?}", hook.format)),
                    reason: None,
                },
            );
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
        } else if parsed_plugin
            .plugin
            .hooks
            .iter()
            .any(|h| h.event == input.event())
        {
            tracing::debug!(
                report = %crate::report::ReportEvent::HookConsidered {
                    plugin: parsed_plugin.plugin.name.clone(),
                    hook: "(none)".into(),
                    event: format!("{:?}", input.event()),
                    selected: false,
                    format: None,
                    reason: Some("no matching format for this agent".into()),
                },
            );
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn env_safe_sanitizes_punctuation() {
        assert_eq!(env_safe("rtk"), "rtk");
        assert_eq!(env_safe("rtk-hooks"), "rtk_hooks");
        assert_eq!(env_safe("a.b-c"), "a_b_c");
        assert_eq!(env_safe("name__req_0"), "name__req_0");
    }

    #[test]
    fn build_env_sets_dir_and_name_vars() {
        // [req (rtk), command (no-source)] order — command was pushed last,
        // so PATH should put its bin dir first.
        let acquired = vec![
            AcquiredInstallation {
                name: "rtk".to_string(),
                base: Some(PathBuf::from("/cache/rtk/1.0")),
                runnable: Some(AcquiredRunnable::ResolvedExec {
                    path: PathBuf::from("/cache/rtk/1.0/bin/rtk"),
                }),
            },
            AcquiredInstallation {
                name: "no-source".to_string(),
                base: None,
                runnable: Some(AcquiredRunnable::ResolvedExec {
                    path: PathBuf::from("/usr/local/bin/tool"),
                }),
            },
        ];
        let env: std::collections::HashMap<_, _> = build_env(&acquired).into_iter().collect();
        assert_eq!(
            env.get("SYMPOSIUM_DIR_rtk").map(String::as_str),
            Some("/cache/rtk/1.0")
        );
        assert_eq!(
            env.get("SYMPOSIUM_rtk").map(String::as_str),
            Some("/cache/rtk/1.0/bin/rtk")
        );
        // No source means no _DIR, but absolute runnable path → SYMPOSIUM_<name> set.
        assert_eq!(env.get("SYMPOSIUM_DIR_no_source"), None);
        assert_eq!(
            env.get("SYMPOSIUM_no_source").map(String::as_str),
            Some("/usr/local/bin/tool")
        );
        // Command (pushed last) wins PATH lookup, so its parent comes first.
        let path = env.get("PATH").expect("PATH set");
        assert!(path.starts_with("/usr/local/bin"), "PATH = {path}");
        assert!(path.contains("/cache/rtk/1.0/bin"), "PATH = {path}");
    }

    #[test]
    fn build_env_no_runnable_no_vars() {
        // Pure-setup installation: no runnable means no SYMPOSIUM_<name>
        // and no PATH contribution. SYMPOSIUM_DIR_<name> still gets set
        // when there's a managed base dir.
        let acquired = vec![AcquiredInstallation {
            name: "setup".to_string(),
            base: Some(PathBuf::from("/cache/setup")),
            runnable: None,
        }];
        let env: std::collections::HashMap<_, _> = build_env(&acquired).into_iter().collect();
        assert_eq!(
            env.get("SYMPOSIUM_DIR_setup").map(String::as_str),
            Some("/cache/setup")
        );
        assert!(env.get("SYMPOSIUM_setup").is_none());
        assert!(env.get("PATH").is_none());
    }

    #[test]
    fn build_env_global_cargo_skips_env_and_path() {
        // Global cargo: PathLookup runnable. Nothing exposed.
        let acquired = vec![AcquiredInstallation {
            name: "rg".to_string(),
            base: None,
            runnable: Some(AcquiredRunnable::GlobalExec {
                path: PathBuf::from("rg".to_string()),
            }),
        }];
        let env: std::collections::HashMap<_, _> = build_env(&acquired).into_iter().collect();
        assert!(env.get("SYMPOSIUM_DIR_rg").is_none());
        assert!(env.get("SYMPOSIUM_rg").is_none());
        assert!(env.get("PATH").is_none());
    }

    #[tokio::test]
    async fn builtin_pre_tool_use_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(tmp.path());
        let mut deps = sym.workspace_deps(tmp.path());
        let input = symposium::InputEvent::PreToolUse(symposium::PreToolUseInput::new(
            "Bash".to_string(),
            serde_json::Value::default(),
            None,
            None,
        ));
        let output = dispatch_builtin(&sym, &input, &mut deps).await;
        assert!(output.additional_context().is_none());
    }

    #[tokio::test]
    async fn builtin_post_tool_use_returns_empty_for_now() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(tmp.path());
        let mut deps = sym.workspace_deps(tmp.path());
        let input = symposium::InputEvent::PostToolUse(symposium::PostToolUseInput::new(
            "Bash".to_string(),
            serde_json::json!({"command": "ls"}),
            serde_json::json!({"stdout": "file.rs"}),
            Some("test-session".to_string()),
            Some("/tmp".to_string()),
        ));
        let output = dispatch_builtin(&sym, &input, &mut deps).await;
        assert!(output.additional_context().is_none());
    }

    #[tokio::test]
    async fn builtin_user_prompt_submit_returns_empty_for_now() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(tmp.path());
        let mut deps = sym.workspace_deps(tmp.path());
        let input = symposium::InputEvent::UserPromptSubmit(symposium::UserPromptSubmitInput::new(
            "Use tokio for async".to_string(),
            Some("test-session".to_string()),
            Some("/tmp".to_string()),
        ));
        let output = dispatch_builtin(&sym, &input, &mut deps).await;
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

    /// Helper: build a minimal plugin with a single PreToolUse hook backed
    /// by a no-op script installation (no `source`, just an on-disk script).
    fn plugin_with_hook(
        plugin_shell: Vec<&str>,
        hook_shell: Vec<&str>,
    ) -> crate::plugins::ParsedPlugin {
        use crate::plugins::{Hook, HookFormat, Installation, Plugin};

        let install = Installation {
            name: "no-op".into(),
            requirements: vec![],
            install_commands: vec![],
            source: None,
            executable: None,
            script: Some("/bin/true".into()),
            args: vec![],
        };
        let hook = Hook {
            name: "h".into(),
            event: HookEvent::PreToolUse,
            agent: None,
            matcher: None,
            requirements: vec![],
            command: "no-op".into(),
            executable: None,
            script: None,
            args: vec![],
            format: HookFormat::Symposium,
            predicates: crate::predicate::PredicateSet {
                predicates: hook_shell
                    .into_iter()
                    .map(|c| crate::predicate::Predicate::Shell(c.into()))
                    .collect(),
            },
        };
        // Plugin gate: `crate(*)` (always applies) plus the shell predicates.
        let plugin_predicates = std::iter::once(crate::predicate::Predicate::CrateWildcard)
            .chain(
                plugin_shell
                    .into_iter()
                    .map(|c| crate::predicate::Predicate::Shell(c.into())),
            )
            .collect();
        let plugin = Plugin {
            name: "test-plugin".into(),
            predicates: crate::predicate::PredicateSet {
                predicates: plugin_predicates,
            },
            installations: vec![install],
            hooks: vec![hook],
            skills: vec![],
            plugin_sources: vec![],
            mcp_servers: vec![],
            subcommands: BTreeMap::new(),
            custom_predicates: vec![],
            discovery: Default::default(),
        };
        crate::plugins::ParsedPlugin {
            path: std::path::PathBuf::from("test.toml"),
            plugin,
            source_name: "test-source".to_string(),
            source_dir: PathBuf::from(".".to_string()),
            source_provenance: std::collections::BTreeSet::new(),
        }
    }

    fn pre_tool_use_input() -> symposium::InputEvent {
        symposium::InputEvent::PreToolUse(symposium::PreToolUseInput::new(
            "Bash".into(),
            serde_json::json!({}),
            None,
            None,
        ))
    }

    #[test]
    fn dispatch_skips_when_plugin_predicate_fails() {
        let plugin = plugin_with_hook(vec!["false"], vec![]);
        let hooks = dispatched_hooks_for_payload(
            &[plugin],
            &pre_tool_use_input(),
            HookAgent::Claude,
            &mut crate::predicate::PredicateContext::new(&[]),
        );
        assert!(hooks.is_empty(), "plugin-level false should drop all hooks");
    }

    #[test]
    fn dispatch_skips_when_hook_predicate_fails() {
        let plugin = plugin_with_hook(vec![], vec!["false"]);
        let hooks = dispatched_hooks_for_payload(
            &[plugin],
            &pre_tool_use_input(),
            HookAgent::Claude,
            &mut crate::predicate::PredicateContext::new(&[]),
        );
        assert!(hooks.is_empty(), "hook-level false should drop the hook");
    }

    #[test]
    fn dispatch_includes_when_predicates_pass() {
        let plugin = plugin_with_hook(vec!["true"], vec!["true"]);
        let hooks = dispatched_hooks_for_payload(
            &[plugin],
            &pre_tool_use_input(),
            HookAgent::Claude,
            &mut crate::predicate::PredicateContext::new(&[]),
        );
        assert_eq!(hooks.len(), 1);
    }

    /// A plugin gated on a concrete crate must not dispatch its hooks in a
    /// workspace that lacks the crate (regression: hooks used to fire for every
    /// plugin regardless of its `crates`).
    #[test]
    fn dispatch_respects_plugin_crate_gate() {
        // Replace the wildcard plugin gate with a concrete `crate(serde)`.
        let mut plugin = plugin_with_hook(vec![], vec![]);
        plugin.plugin.predicates = crate::predicate::PredicateSet {
            predicates: vec![crate::predicate::Predicate::Crate("serde".into(), None)],
        };

        // No serde in the workspace → the hook is skipped.
        let empty = dispatched_hooks_for_payload(
            &[plugin.clone()],
            &pre_tool_use_input(),
            HookAgent::Claude,
            &mut crate::predicate::PredicateContext::new(&[]),
        );
        assert!(
            empty.is_empty(),
            "crate-gated hook should not fire without the crate"
        );

        // serde present → the hook fires.
        let deps = vec![("serde".to_string(), semver::Version::new(1, 0, 0))];
        let matched = dispatched_hooks_for_payload(
            &[plugin],
            &pre_tool_use_input(),
            HookAgent::Claude,
            &mut crate::predicate::PredicateContext::new(&deps),
        );
        assert_eq!(
            matched.len(),
            1,
            "crate-gated hook should fire when the crate is present"
        );
    }

    #[test]
    fn dispatch_respects_installed_provenance_predicate() {
        let mut plugin = plugin_with_hook(vec![], vec![]);
        plugin.plugin.predicates = crate::predicate::PredicateSet {
            predicates: vec![crate::predicate::Predicate::Installed],
        };

        // Without installed provenance → hook skipped.
        let empty = dispatched_hooks_for_payload(
            &[plugin.clone()],
            &pre_tool_use_input(),
            HookAgent::Claude,
            &mut crate::predicate::PredicateContext::new(&[]),
        );
        assert!(
            empty.is_empty(),
            "installed() should fail without provenance"
        );

        // With installed provenance → hook fires.
        plugin.source_provenance =
            std::collections::BTreeSet::from([crate::crate_sources::SourceProvenance::Installed]);
        let matched = dispatched_hooks_for_payload(
            &[plugin],
            &pre_tool_use_input(),
            HookAgent::Claude,
            &mut crate::predicate::PredicateContext::new(&[]),
        );
        assert_eq!(matched.len(), 1, "installed() should pass with provenance");
    }

    #[test]
    fn dispatch_respects_workspace_provenance_predicate() {
        let mut plugin = plugin_with_hook(vec![], vec![]);
        plugin.plugin.predicates = crate::predicate::PredicateSet {
            predicates: vec![crate::predicate::Predicate::Workspace],
        };
        plugin.source_provenance =
            std::collections::BTreeSet::from([crate::crate_sources::SourceProvenance::Workspace]);

        let matched = dispatched_hooks_for_payload(
            &[plugin],
            &pre_tool_use_input(),
            HookAgent::Claude,
            &mut crate::predicate::PredicateContext::new(&[]),
        );
        assert_eq!(
            matched.len(),
            1,
            "workspace() should pass for workspace source"
        );
    }

    #[test]
    fn dispatch_per_plugin_provenance_isolation() {
        // Two plugins: first is workspace-only, second is installed-only.
        // A workspace() predicate should match only the first.
        let mut workspace_plugin = plugin_with_hook(vec![], vec![]);
        workspace_plugin.plugin.name = "ws-plugin".into();
        workspace_plugin.plugin.predicates = crate::predicate::PredicateSet {
            predicates: vec![crate::predicate::Predicate::Workspace],
        };
        workspace_plugin.source_provenance =
            std::collections::BTreeSet::from([crate::crate_sources::SourceProvenance::Workspace]);

        let mut installed_plugin = plugin_with_hook(vec![], vec![]);
        installed_plugin.plugin.name = "inst-plugin".into();
        installed_plugin.plugin.predicates = crate::predicate::PredicateSet {
            predicates: vec![crate::predicate::Predicate::Workspace],
        };
        installed_plugin.source_provenance =
            std::collections::BTreeSet::from([crate::crate_sources::SourceProvenance::Installed]);

        let hooks = dispatched_hooks_for_payload(
            &[workspace_plugin, installed_plugin],
            &pre_tool_use_input(),
            HookAgent::Claude,
            &mut crate::predicate::PredicateContext::new(&[]),
        );
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0].plugin_name, "ws-plugin");
    }
}
