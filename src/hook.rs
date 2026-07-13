use std::{
    io::{Read, Write},
    path::PathBuf,
    process::{Command, ExitCode, Stdio},
    time::Instant,
};

use symposium_install::Runnable;

use crate::plugins::{HookFormat, Installation};
use crate::sync;
use crate::telemetry::EventKind;
use crate::{
    config::Symposium,
    hook_schema::{AgentHookInput, symposium},
    plugins::ParsedPlugin,
};
use crate::{
    help_render::{AGENTS_HEADING, HUMANS_HEADING},
    hook_schema::symposium::{OutputEvent, SessionStartInput},
    subcommand_dispatch::applicable_subcommands,
};
use crate::{
    installation::{
        AcquiredInstallation, AcquiredRunnable, acquire_installation,
        refresh_installation_if_present, resolve_runnable,
    },
    sync::sync,
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
    // Dispatch-time acquisition serves the cache (git checks debounced); the
    // `SessionStart` prewarm is what forces a freshness check once per session.
    let update = symposium_install::UpdateLevel::None;

    // Acquire requirements first so the command's PATH sees them.
    let mut acquired: Vec<AcquiredInstallation> = Vec::new();
    for requirement in &hook.requirements {
        match acquire_installation(sym, requirement, None, None, update).await {
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
        update,
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
        // SessionStart refreshes source caches and syncs unconditionally.
        let session_start = event == HookEvent::SessionStart;
        let sync_summary = run_auto_sync(sym, &mut deps, session_start).await;

        // SessionStart (once per session) also refreshes every hook's already-
        // cached source, so later events dispatch fresh binaries without per-
        // event network cost. Best-effort; gated by `auto-sync`.
        if session_start && sym.config.auto_sync {
            prewarm_hook_sources(sym, &mut deps).await;
        }

        // Builtin dispatch → symposium output → host agent output as Value
        let builtin_sym_output = dispatch_builtin(sym, &sym_input, &mut deps).await;
        let builtin_agent_output = handler.translate_output(&builtin_sym_output);
        let prior_output = builtin_agent_output.to_hook_output();

        // Plugin dispatch with format routing
        let (final_output, hook_invocations) = dispatch_plugin_hooks(
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

        // Emit telemetry for this pass. A blocking hook (exit 2 / killed) returned
        // above via `?`, so a blocked pass is never reached here and records no
        // telemetry: the block path stays free of best-effort work.
        let events = telemetry_events(agent, &sym_input, sync_summary, hook_invocations);

        if !events.is_empty() {
            let session_id = sym_input.session_id().map(str::to_string);
            for ekind in events {
                sym.record_telemetry(session_id.clone(), ekind);
            }
        }

        let serialized = handler.serialize_output(&final_output);
        tracing::trace!(output_len = serialized.len(), "hook output serialized");
        Ok(serialized)
    } else {
        // Agent doesn't support this event
        anyhow::bail!("agent {agent:?} does not support hook event {event:?}")
    }
}

/// Map a completed hook pass to its telemetry events. Pure: no I/O, no opt-in check so it is unit-testable.
///
/// Consumes `sync_summary` and `hooks` so their strings move into the events rather than being cloned.
/// `session_id` is not produced here; it rides on the `TelemetryEvent` envelope,
/// so the caller pairs each returned kind with the wire input's session id.
fn telemetry_events(
    agent: HookAgent,
    sym_input: &symposium::InputEvent,
    sync_summary: Option<sync::SyncSummary>,
    hooks: Vec<HookInvocation>,
) -> Vec<EventKind> {
    let session_start = matches!(sym_input, symposium::InputEvent::SessionStart(_));

    let mut events = Vec::new();
    if let Some(kind) = wire_event(agent, sym_input, sync_summary.as_ref()) {
        events.push(kind);
    }

    if let Some(summary) = sync_summary {
        events.extend(sync_events(summary, session_start));
    }

    events.extend(hooks.into_iter().map(|hk| EventKind::HookInvocation {
        hook: hk.hook,
        plugin: hk.plugin,
        duration_ms: hk.duration_ms,
    }));

    events
}

/// The wire-funnel event for this hook event, if any. `PreToolUse` maps to `None` so
/// a tool is counted once (on `PostToolUse`), not twice.
fn wire_event(
    agent: HookAgent,
    sym_input: &symposium::InputEvent,
    sync_summary: Option<&sync::SyncSummary>,
) -> Option<EventKind> {
    Some(match sym_input {
        symposium::InputEvent::SessionStart(_) => EventKind::SessionStart {
            agent: agent.as_str().to_string(),
            crate_count: sync_summary.map(|ss| ss.crate_count),
        },
        symposium::InputEvent::UserPromptSubmit(_) => EventKind::UserPrompt,
        symposium::InputEvent::PostToolUse(ptui) => EventKind::ToolUse {
            tool: ptui.tool_name.clone(),
        },
        symposium::InputEvent::Stop(_) => EventKind::Stop,
        _ => return None,
    })
}

/// The sync-pass events: `sync_run` whenever the body ran, plus the plugin/skills activation
/// snapshot only on the SessionStart sync (recorded once per session).
fn sync_events(summary: sync::SyncSummary, session_start: bool) -> Vec<EventKind> {
    let mut events = vec![EventKind::SyncRun {
        installed: summary.installed,
        reaped: summary.reaped,
        plugins_matched: summary.plugins.len(),
    }];

    if session_start {
        events.extend(
            summary
                .plugins
                .into_iter()
                .map(|pa| EventKind::PluginActivation {
                    plugin: pa.name,
                    crates: pa.crates,
                }),
        );

        events.extend(
            summary
                .skills
                .into_iter()
                .map(|ska| EventKind::SkillActivation {
                    skill: ska.name,
                    plugin: ska.plugin,
                    crates: ska.crates,
                }),
        );
    }

    events
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
/// On most events this uses per-workspace state to skip sync when `Cargo.lock`
/// hasn't changed since the last successful sync, avoiding expensive `cargo
/// metadata` calls on every hook invocation. If sync runs, `deps` gets
/// populated — later hook stages reuse the result.
///
/// `SessionStart` runs once per session, so there we do the real work: refresh
/// every source cache (`UpdateLevel::Check`) and sync unconditionally, ignoring
/// the `Cargo.lock` freshness gate — upstream skill changes land even when the
/// workspace's dependencies are unchanged.
async fn run_auto_sync(
    sym: &Symposium,
    deps: &mut WorkspaceDeps,
    session_start: bool,
) -> Option<sync::SyncSummary> {
    if !sym.config.auto_sync {
        tracing::debug!("auto-sync disabled, skipping");
        return None;
    }

    let cwd = deps.cwd().to_path_buf();

    // Find workspace root via `cargo locate-project` (fast, no dep resolution).
    // If we can't find one, fall through to full sync which will
    // use cargo metadata.
    let workspace_root = crate::workspace_state::find_workspace_root(sym, &cwd);

    // SessionStart always syncs (and refreshes sources); other events skip
    // when the workspace hasn't changed since the last sync.
    if !session_start && let Some(ref root) = workspace_root {
        let state = crate::workspace_state::WorkspaceState::load(sym, root);
        if state.sync_is_fresh(root) {
            tracing::debug!("auto-sync skipped: Cargo.lock unchanged since last sync");
            return None;
        }
    }

    let update = if session_start {
        symposium_install::UpdateLevel::Check
    } else {
        symposium_install::UpdateLevel::None
    };

    tracing::debug!("auto-sync running");
    let summary = match sync(sym, deps, update).await {
        Ok(summary) => summary,
        Err(err) => {
            tracing::warn!(error = %err, "auto-sync during hook failed (continuing)");
            return None;
        }
    };

    // Record successful sync. If we didn't find the root earlier,
    // try again now (sync may have created Cargo.lock).
    let root = workspace_root.or_else(|| crate::workspace_state::find_workspace_root(sym, &cwd));
    if let Some(ref root) = root {
        let mut state = crate::workspace_state::WorkspaceState::load(sym, root);
        state.record_sync(root);
        state.workspace_root = Some(root.clone());
        state.save(sym, root);
    }

    Some(summary)
}

/// Refresh the source cache for every hook the workspace could fire this
/// session. Run once on `SessionStart` (where the per-session cost is
/// acceptable) so later events dispatch fresh binaries from cache — dispatch
/// itself acquires with `UpdateLevel::None`. This is also the only path that
/// re-pulls a `cargo + git` hook binary whose branch moved, since its
/// version-keyed cache never invalidates on its own.
///
/// Refresh-only: a source that was never acquired is left alone (it installs
/// lazily when the hook first fires) — `SessionStart` updates installed tools
/// but never installs eagerly. Best-effort: failures are logged and skipped.
async fn prewarm_hook_sources(sym: &Symposium, deps: &mut WorkspaceDeps) {
    let workspace = deps.load().cloned();
    let plugins = crate::plugins::load_all_plugins(sym, workspace.as_deref());

    // Resolving the workspace runs cargo, so only do it when some hook's
    // gating references a concrete crate (mirrors dispatch).
    let pairs = if plugins.iter().any(|p| p.plugin.hooks_need_dep_resolution()) {
        crate::crate_sources::crate_pairs(deps.crates())
    } else {
        Vec::new()
    };
    let mut ctx = crate::predicate::PredicateContext::new(&pairs);

    for parsed in &plugins {
        if !parsed.applies(&mut ctx) {
            continue;
        }
        for hook in &parsed.plugin.hooks {
            if !hook.predicates.evaluate(&mut ctx) {
                continue;
            }
            let resolved = match ResolvedHook::build(parsed, hook) {
                Ok(r) => r,
                Err(e) => {
                    tracing::debug!(plugin = %parsed.plugin.name, hook = %hook.name, error = %e, "prewarm: skipping unbuildable hook");
                    continue;
                }
            };
            for req in &resolved.requirements {
                if let Err(e) = refresh_installation_if_present(sym, req, None).await {
                    tracing::debug!(name = %req.name, error = %e, "prewarm: requirement refresh failed");
                }
            }
            if let Err(e) = refresh_installation_if_present(
                sym,
                &resolved.command,
                resolved.hook_executable.as_deref(),
            )
            .await
            {
                tracing::debug!(plugin = %resolved.plugin_name, hook = %resolved.hook_name, error = %e, "prewarm: command refresh failed");
            }
        }
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
    let workspace = deps.load().cloned();
    let registry = crate::plugins::load_registry_with_workspace(sym, workspace.as_deref());
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

/// One plugin hook that ran during dispatch, with how long it took.
/// A plain record kept out of the telemetry types so dispatch stays decoupled from the
/// wire schema; `execute_hook` maps it to a `telemetry::EventKind`.
#[derive(Debug, Clone)]
pub struct HookInvocation {
    pub plugin: String,
    pub hook: String,
    pub duration_ms: u64,
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
) -> Result<(serde_json::Value, Vec<HookInvocation>), Vec<u8>> {
    let hooks = select_dispatch_hooks(sym, host_agent, sym_input, deps);

    let mut output = prior_output;
    let mut invocations = Vec::new();
    for hook in hooks {
        if let HookRun::Ran {
            invocation,
            to_merge,
        } = run_plugin_hook(sym, host_agent, event, sym_input, original_input, &hook).await?
        {
            invocations.push(invocation);
            if let Some(json) = to_merge {
                merge(&mut output, json);
            }
        }
    }
    Ok((output, invocations))
}

/// Load the plugins and resolve which of their hooks apply to this event.
fn select_dispatch_hooks(
    sym: &Symposium,
    host_agent: HookAgent,
    sym_input: &symposium::InputEvent,
    deps: &mut WorkspaceDeps,
) -> Vec<ResolvedHook> {
    let workspace = deps.load().cloned();
    let plugins = crate::plugins::load_all_plugins(sym, workspace.as_deref());

    // Resolving the workspace means running cargo, so only do it when some
    // plugin's hook gating actually references a concrete crate (a `depends-on(*)`
    // wildcard or env/shell/path predicate never needs the crate graph).
    let pairs = if plugins
        .iter()
        .any(|pplugin| pplugin.plugin.hooks_need_dep_resolution())
    {
        crate::crate_sources::crate_pairs(deps.crates())
    } else {
        Vec::new()
    };
    let mut ctx = crate::predicate::PredicateContext::new(&pairs);
    dispatched_hooks_for_payload(&plugins, sym_input, host_agent, &mut ctx)
}

/// Outcome of running one plugin hook.
enum HookRun {
    /// The hook ran; `to_merge` is its output to fold into the aggregate, if any.
    Ran {
        invocation: HookInvocation,
        to_merge: Option<serde_json::Value>,
    },
    /// The hook did not run (serialize / prepare / spawn / wait failure, already
    /// logged), so there is nothing to record or merge.
    Skipped,
}

/// Run one plugin hook end to end: route its stdin by declared format, spawn it,
/// time it, and interpret its output. `Err(stderr)` is the block signal (exit 2
/// or killed) and propagates out of dispatch.
async fn run_plugin_hook(
    sym: &Symposium,
    host_agent: HookAgent,
    event: HookEvent,
    sym_input: &symposium::InputEvent,
    original_input: &dyn AgentHookInput,
    hook: &ResolvedHook,
) -> Result<HookRun, Vec<u8>> {
    tracing::info!(
        plugin = %hook.plugin_name,
        hook = %hook.hook_name,
        format = ?hook.format,
        "running plugin hook"
    );

    // Route stdin by the hook's declared format: native (matches the host agent)
    // gets the original input, otherwise the canonical symposium format.
    let hook_agent = hook.format.as_agent();
    let hook_input: &dyn AgentHookInput = if hook_agent == Some(host_agent) {
        original_input
    } else {
        sym_input
    };
    let stdin_str = match hook_input.to_string() {
        Ok(hook_output) => hook_output,
        Err(err) => {
            tracing::error!(plugin = %hook.plugin_name, hook = %hook.hook_name, error = %err, "failed to serialize hook input");
            return Ok(HookRun::Skipped);
        }
    };

    let spawn_res = match build_spawn_spec(sym, hook).await {
        Ok(spec) => spawn_from_spec(spec),
        Err(e) => {
            tracing::warn!(error = %e, "failed to prepare hook command");
            return Ok(HookRun::Skipped);
        }
    };

    let mut child = match spawn_res {
        Ok(child) => child,
        Err(err) => {
            tracing::debug!(
                report = %crate::report::ReportEvent::HookDispatched {
                    plugin: hook.plugin_name.clone(),
                    hook: hook.hook_name.clone(),
                    exit_code: None,
                    error: Some(err.to_string()),
                },
            );
            tracing::warn!(error = %err, "failed to spawn hook command");
            return Ok(HookRun::Skipped);
        }
    };

    let start = Instant::now();
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(stdin_str.as_bytes());
    }

    let child_out = match child.wait_with_output() {
        Ok(output) => output,
        Err(err) => {
            tracing::warn!(error = %err, "failed waiting for hook process");
            return Ok(HookRun::Skipped);
        }
    };

    tracing::trace!(?child_out, "hook finished");

    let invocation = HookInvocation {
        plugin: hook.plugin_name.clone(),
        hook: hook.hook_name.clone(),
        duration_ms: start.elapsed().as_millis() as u64,
    };
    tracing::debug!(
        report = %crate::report::ReportEvent::HookDispatched {
            plugin: hook.plugin_name.clone(),
            hook: hook.hook_name.clone(),
            exit_code: child_out.status.code(),
            error: None,
        },
    );

    let to_merge = interpret_hook_output(host_agent, event, hook_agent, child_out)?;
    Ok(HookRun::Ran {
        invocation,
        to_merge,
    })
}

/// Interpret a finished hook's exit code and stdout into the JSON to merge.
/// `Err(stderr)` signals a block (exit 2 or killed); `Ok(None)` means the hook
/// produced nothing to merge.
fn interpret_hook_output(
    host_agent: HookAgent,
    event: HookEvent,
    hook_agent: Option<HookAgent>,
    child_out: std::process::Output,
) -> Result<Option<serde_json::Value>, Vec<u8>> {
    match child_out.status.code() {
        None | Some(2) => Err(child_out.stderr),
        Some(0) if child_out.stdout.is_empty() => Ok(None),
        Some(0) => {
            // Parse the hook's stdout and convert it to host-agent format. Two
            // cases: native (same as host) or the canonical symposium format.
            let Some(host_h) = host_agent.event(event) else {
                return Ok(None);
            };

            let host_json = if hook_agent == Some(host_agent) {
                match host_h.parse_output(&child_out.stdout) {
                    Ok(output) => output.to_hook_output(),
                    Err(err) => {
                        tracing::warn!(error = %err, "failed to parse hook output");
                        return Ok(None);
                    }
                }
            } else {
                match serde_json::from_slice::<serde_json::Value>(&child_out.stdout) {
                    Ok(value) => {
                        if let Ok(sym_out) =
                            serde_json::from_value::<symposium::OutputEvent>(value.clone())
                        {
                            host_h.translate_output(&sym_out).to_hook_output()
                        } else {
                            value
                        }
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "failed to parse hook output");
                        return Ok(None);
                    }
                }
            };

            Ok(Some(host_json))
        }
        Some(code) => {
            tracing::warn!(
                exit_code = code,
                "plugin hook exited with non-zero (continuing)"
            );
            Ok(None)
        }
    }
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
        // Plugin-level predicates gate every hook in the plugin. Evaluated once
        // per plugin per dispatch — keep them cheap.
        if !parsed_plugin.applies(ctx) {
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

    // --- telemetry_events mapping ---

    #[test]
    fn telemetry_events_session_start_emits_funnel_and_activations() {
        let input = symposium::InputEvent::SessionStart(symposium::SessionStartInput::new(
            Some("s1".into()),
            None,
        ));
        let summary = sync::SyncSummary {
            plugins: vec![crate::skills::PluginActivation {
                name: "example-plugin".into(),
                crates: vec!["acme-core".into()],
            }],
            skills: vec![crate::skills::SkillActivation {
                name: "example-skill".into(),
                plugin: Some("example-plugin".into()),
                crates: vec!["acme-core".into()],
            }],
            installed: 2,
            reaped: 1,
            crate_count: 5,
        };
        let events = telemetry_events(HookAgent::Claude, &input, Some(summary), vec![]);

        assert_eq!(events.len(), 4);
        match &events[0] {
            EventKind::SessionStart { agent, crate_count } => {
                assert_eq!(agent, "claude");
                assert_eq!(*crate_count, Some(5));
            }
            other => panic!("expected session_start, got {other:?}"),
        }
        assert!(matches!(
            events[1],
            EventKind::SyncRun {
                installed: 2,
                reaped: 1,
                plugins_matched: 1
            }
        ));
        assert!(matches!(events[2], EventKind::PluginActivation { .. }));
        assert!(matches!(events[3], EventKind::SkillActivation { .. }));
    }

    #[test]
    fn telemetry_events_non_session_sync_records_sync_run_but_no_activations() {
        let input = symposium::InputEvent::PostToolUse(symposium::PostToolUseInput::new(
            "Bash".into(),
            serde_json::Value::Null,
            serde_json::Value::Null,
            Some("s1".into()),
            None,
        ));
        let summary = sync::SyncSummary {
            plugins: vec![crate::skills::PluginActivation {
                name: "example-plugin".into(),
                crates: vec![],
            }],
            skills: vec![],
            installed: 0,
            reaped: 0,
            crate_count: 3,
        };
        let events = telemetry_events(HookAgent::Claude, &input, Some(summary), vec![]);

        // tool_use + sync_run, but the activation snapshot is SessionStart-only.
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], EventKind::ToolUse { .. }));
        assert!(matches!(events[1], EventKind::SyncRun { .. }));
        assert!(!events.iter().any(|e| matches!(
            e,
            EventKind::PluginActivation { .. } | EventKind::SkillActivation { .. }
        )));
    }

    #[test]
    fn telemetry_events_pre_tool_use_is_silent() {
        let input = symposium::InputEvent::PreToolUse(symposium::PreToolUseInput::new(
            "Bash".into(),
            serde_json::Value::Null,
            Some("s1".into()),
            None,
        ));
        let events = telemetry_events(HookAgent::Claude, &input, None, vec![]);
        assert!(events.is_empty());
    }

    #[test]
    fn telemetry_events_pre_tool_use_still_records_hook_invocations() {
        // "PreToolUse is silent" means only the wire funnel event; a hook that
        // ran on a PreToolUse pass must still be counted.
        let input = symposium::InputEvent::PreToolUse(symposium::PreToolUseInput::new(
            "Bash".into(),
            serde_json::Value::Null,
            Some("s1".into()),
            None,
        ));
        let hooks = vec![HookInvocation {
            plugin: "example-plugin".into(),
            hook: "format-check".into(),
            duration_ms: 3,
        }];
        let events = telemetry_events(HookAgent::Claude, &input, None, hooks);

        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0],
            EventKind::HookInvocation { duration_ms: 3, .. }
        ));
    }

    #[test]
    fn telemetry_events_maps_hook_invocations() {
        let input = symposium::InputEvent::PostToolUse(symposium::PostToolUseInput::new(
            "Edit".into(),
            serde_json::Value::Null,
            serde_json::Value::Null,
            None,
            None,
        ));
        let hooks = vec![HookInvocation {
            plugin: "example-plugin".into(),
            hook: "format-check".into(),
            duration_ms: 7,
        }];
        let events = telemetry_events(HookAgent::Claude, &input, None, hooks);

        assert_eq!(events.len(), 2);
        match &events[0] {
            EventKind::ToolUse { tool } => assert_eq!(tool, "Edit"),
            other => panic!("expected tool_use, got {other:?}"),
        }
        assert!(matches!(
            events[1],
            EventKind::HookInvocation { duration_ms: 7, .. }
        ));
    }

    // --- interpret_hook_output: exit-code handling ---

    /// A finished-process `Output` with a chosen exit code and stderr, built
    /// without spawning so the block path is testable on every OS.
    ///
    /// Unix `ExitStatus::from_raw` wants the raw `waitpid` status, which carries
    /// the exit code in the second byte, so the code is shifted left by 8.
    #[cfg(unix)]
    fn output_with_code(code: i32, stderr: &[u8]) -> std::process::Output {
        use std::os::unix::process::ExitStatusExt;
        std::process::Output {
            status: std::process::ExitStatus::from_raw(code << 8),
            stdout: Vec::new(),
            stderr: stderr.to_vec(),
        }
    }

    #[cfg(windows)]
    fn output_with_code(code: i32, stderr: &[u8]) -> std::process::Output {
        use std::os::windows::process::ExitStatusExt;
        std::process::Output {
            status: std::process::ExitStatus::from_raw(code as u32),
            stdout: Vec::new(),
            stderr: stderr.to_vec(),
        }
    }

    #[test]
    fn interpret_hook_output_blocks_on_exit_2() {
        // Exit 2 is the block signal: dispatch surfaces the hook's stderr as the
        // deny reason (Err) rather than merging any output.
        let out = output_with_code(2, b"denied by policy");
        let result = interpret_hook_output(HookAgent::Claude, HookEvent::PreToolUse, None, out);
        assert_eq!(result, Err(b"denied by policy".to_vec()));
    }

    #[test]
    fn interpret_hook_output_does_not_block_on_other_nonzero() {
        // Only exit 2 blocks. Any other non-zero code is a soft failure that
        // continues with nothing to merge.
        let out = output_with_code(1, b"just a warning");
        let result = interpret_hook_output(HookAgent::Claude, HookEvent::PreToolUse, None, out);
        assert_eq!(result, Ok(None));
    }

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
        // Plugin gate: `depends-on(*)` (always applies) plus the shell predicates.
        let plugin_predicates = std::iter::once(crate::predicate::Predicate::DependsOnWildcard)
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
            mcp_servers: vec![],
            subcommands: BTreeMap::new(),
            custom_predicates: vec![],
        };
        crate::plugins::ParsedPlugin {
            path: std::path::PathBuf::from("test.toml"),
            plugin,
            source_name: "test-source".to_string(),
            source_dir: PathBuf::from(".".to_string()),
            workspace_member: false,
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
    /// plugin regardless of its `depends-on`).
    #[test]
    fn dispatch_respects_plugin_crate_gate() {
        // Replace the wildcard plugin gate with a concrete `depends-on(serde)`.
        let mut plugin = plugin_with_hook(vec![], vec![]);
        plugin.plugin.predicates = crate::predicate::PredicateSet {
            predicates: vec![crate::predicate::Predicate::DependsOn("serde".into(), None)],
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
}
