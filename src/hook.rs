use std::{
    io::{Read, Write},
    process::{Command, ExitCode, Stdio},
};

use crate::plugins::ParsedPlugin;
use crate::{
    config::Symposium,
    hook_schema::{AgentHookEvent, AgentHookOutput, AgentHookPayload},
};

// Re-export hook schema types for convenience.
pub use crate::hook_schema::{
    HookAgent, HookEvent, HookOutput, HookPayload, HookSpecificOutput, HookSubPayload, PostToolUsePayload,
    PreToolUsePayload, UserPromptSubmitPayload,
};

/// CLI entry point: read payload from stdin, dispatch, print output.
pub async fn run(
    sym: &Symposium,
    agent: HookAgent,
    event: HookEvent,
    cwd: &std::path::Path,
) -> ExitCode {
    tracing::debug!("Running hook listener for agent {agent:?} and event {event:?}");

    let event_handler = agent.event(event).unwrap();

    let mut input = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut input) {
        tracing::warn!(?event, error = %e, "failed to read hook stdin");
        return ExitCode::SUCCESS;
    }
    tracing::debug!(?input);

    let payload = event_handler.parse_payload(&input);
    let payload = match payload {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(?event, error = %e, "invalid hook payload");
            return ExitCode::FAILURE;
        }
    };

    let builtin_payload = payload.to_hook_payload();

    if builtin_payload.sub_payload.hook_event() != event {
        tracing::warn!(?event, payload_event = ?builtin_payload.sub_payload.hook_event(), "hook event mismatch between CLI arg and payload");
        return ExitCode::FAILURE;
    }

    // Run sync --agent to ensure extensions are installed and hooks are current.
    // Use the payload's cwd if available, otherwise the cwd passed from main.
    let effective_cwd = builtin_payload
        .cwd()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| cwd.to_path_buf());
    let project_root = Some(effective_cwd.as_path())
        .filter(|p| p.join(".cargo-agents").is_dir());
    let out = crate::output::Output::quiet();
    if let Err(e) = crate::sync::sync_agent(sym, project_root, &out).await {
        tracing::warn!(error = %e, "sync --agent during hook failed (continuing)");
    }

    // Run built-in hook logic
    let hook_output = dispatch_builtin(sym, &builtin_payload).await;
    let hook_output = event_handler.from_hook_output(&hook_output);
    let hook_output = match hook_output {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!(?event, error = %e, "invalid hook output from builtin dispatch");
            return ExitCode::FAILURE;
        }
    };

    let plugin_output = event_handler.dispatch_plugin_hooks(sym, payload, hook_output);

    match plugin_output {
        PluginHookOutput::Success(plugin_json) => {
            std::io::stdout()
                .write_all(&serde_json::to_vec(&plugin_json).unwrap())
                .unwrap();

            ExitCode::SUCCESS
        }
        PluginHookOutput::Failure(stderr) => {
            std::io::stderr().write_all(&stderr).unwrap();

            ExitCode::FAILURE
        }
    }
}

/// Built-in hook logic. Returns typed `HookOutput` for the integration test harness.
pub async fn dispatch_builtin(sym: &Symposium, payload: &HookPayload) -> HookOutput {
    match &payload.sub_payload {
        HookSubPayload::PreToolUse(_) => {
            // No built-in logic for PreToolUse — plugin hooks only.
            HookOutput::empty()
        }
        HookSubPayload::PostToolUse(post) => handle_post_tool_use(sym, post).await,
        HookSubPayload::UserPromptSubmit(prompt) => handle_user_prompt_submit(sym, prompt).await,
    }
}

/// Handle PostToolUse: detect and record skill activations.
async fn handle_post_tool_use(sym: &Symposium, post: &PostToolUsePayload) -> HookOutput {
    let Some(ref session_id) = post.session_id else {
        return HookOutput::empty();
    };
    let Some(ref cwd_str) = post.cwd else {
        return HookOutput::empty();
    };

    let cwd = std::path::Path::new(cwd_str);
    let mut session = crate::session_state::load_session(sym, session_id);

    // Detect activation via cargo-agents crate command (Bash tool)
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
    HookOutput::empty()
}

/// Detect if a Bash tool successfully ran `cargo agents crate <name>` or
/// `cargo-agents crate <name>`.
///
/// Also matches the legacy `symposium crate` form for backward compatibility.
fn detect_crate_activation_bash(post: &PostToolUsePayload) -> Option<String> {
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
/// - `cargo agents crate <args>`
/// - `cargo-agents crate <args>`
/// - `symposium crate <args>` (legacy)
/// - `symposium.sh crate <args>` (legacy)
///
/// The command token must be preceded by a path boundary (start, whitespace, `/`, `\`).
fn find_crate_args(command: &str) -> Option<&str> {
    let needles = [
        "cargo agents crate ",
        "cargo-agents crate ",
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
fn detect_crate_activation_mcp(post: &PostToolUsePayload) -> Option<String> {
    // MCP tool names include the server prefix, e.g., "mcp__cargo_agents__rust"
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
    post: &PostToolUsePayload,
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
    prompt_payload: &UserPromptSubmitPayload,
) -> HookOutput {
    let nudge_interval = sym.config.hooks.nudge_interval;

    // If nudge-interval is 0, disable nudges entirely
    if nudge_interval == 0 {
        return HookOutput::empty();
    }

    let Some(ref session_id) = prompt_payload.session_id else {
        return HookOutput::empty();
    };
    let Some(ref cwd_str) = prompt_payload.cwd else {
        return HookOutput::empty();
    };

    let cwd = std::path::Path::new(cwd_str);

    // Compute available skills for this workspace (no caching)
    let available = crate::workspace::compute_skills_applicable_to_workspace(sym, cwd)
        .await
        .unwrap_or_default();

    if available.is_empty() {
        return HookOutput::empty();
    }

    // Extract unique crate names from available skills
    let available_crate_names: std::collections::BTreeSet<String> =
        available.iter().map(|s| s.crate_name.clone()).collect();

    // Find crate mentions in the prompt
    let mentioned = extract_crate_mentions(&prompt_payload.prompt, &available_crate_names);

    if mentioned.is_empty() {
        return HookOutput::empty();
    }

    // Load session, increment prompt count, compute nudges
    let mut session = crate::session_state::load_session(sym, session_id);
    session.increment_prompt_count();
    let nudge_crates = session.compute_nudges(&mentioned, nudge_interval);
    crate::session_state::save_session(sym, session_id, &session);

    if nudge_crates.is_empty() {
        return HookOutput::empty();
    }

    // Format nudge message
    let mut context = String::new();
    for crate_name in &nudge_crates {
        context.push_str(&format!(
            "The `{crate_name}` crate has specialized guidance available.\n\
             To load it, run: `cargo agents crate {crate_name}`\n\n"
        ));
    }

    HookOutput::with_context("UserPromptSubmit", context.trim_end().to_string())
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

/// Dispatch plugin hooks (spawn subprocesses).
pub(crate) fn dispatch_plugin_hooks<E: AgentHookEvent>(
    sym: &Symposium,
    event: &E,
    payload: &E::Payload,
    prior_output: E::Output,
) -> PluginHookOutput {
    tracing::info!(?payload, "hook invoked (builtin)");

    let plugins = crate::plugins::load_all_plugins(sym);
    let hooks = hooks_for_payload(&plugins, &payload.to_hook_payload());

    let mut output_json = prior_output;

    for (plugin_name, hook) in hooks {
        tracing::info!(?plugin_name, hook = %hook.name, cmd = %hook.command, "running plugin hook");
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
                    if let Err(e) = stdin.write_all(payload.to_string().unwrap().as_bytes()) {
                        tracing::warn!(error = %e, "failed to write hook stdin");
                    }
                }

                let output = match child.wait_with_output() {
                    Ok(output) => output,
                    Err(e) => {
                        tracing::warn!(error = %e, "failed waiting for hook process");
                        continue;
                    }
                };

                tracing::info!(?output, "hook finished");

                match output.status.code() {
                    // FIXME: I don't actually know what the semantics of hook exit codes are,
                    // but this is probably fine for now
                    None | Some(2) => {
                        return PluginHookOutput::Failure(output.stderr);
                    }
                    Some(0 | _) => {
                        let stdout = output.stdout;
                        let plugin_output = event.parse_output(&stdout);
                        match plugin_output {
                            Ok(plugin_output) => {
                                output_json = E::merge_outputs(output_json, plugin_output);
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "failed to parse hook output as JSON")
                            }
                        }
                    }
                }
            }
            Err(e) => tracing::warn!(error = %e, "failed to spawn hook command"),
        }
    }

    PluginHookOutput::Success(output_json.to_hook_output())
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
    payload: &HookPayload,
) -> Vec<(String, crate::plugins::Hook)> {
    tracing::debug!(?payload);

    let mut out = Vec::new();

    for ParsedPlugin { path: _, plugin } in plugins {
        let name = plugin.name.clone();
        for hook in &plugin.hooks {
            tracing::debug!(?hook);
            if hook.event != payload.sub_payload.hook_event() {
                continue;
            }
            if let Some(matcher) = &hook.matcher {
                if !payload.sub_payload.matches_matcher(matcher) {
                    tracing::info!(
                        ?payload,
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
    use crate::hook_schema::claude::{
        ClaudeCodeHookCommonPayload, ClaudeCodePreToolUseOutput, ClaudeCodePreToolUsePayload,
    };
    use crate::hook_schema::{Agent, claude::ClaudeCode};

    use super::*;
    use std::fs;

    use indoc::formatdoc;

    fn setup_tracing() {
        let _ = tracing_subscriber::fmt()
            .with_test_writer()
            .compact()
            .with_ansi(false)
            .with_env_filter(
                tracing_subscriber::EnvFilter::from_default_env()
                    .add_directive(tracing::Level::DEBUG.into()),
            )
            .try_init();
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn plugin_hooks_run_and_create_files() {
        setup_tracing();

        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path();

        let sym = Symposium::from_dir(home);

        // Ensure plugins dir exists and get its path.
        let plugins_dir = sym.plugins_dir();

        // Prepare two output files that the hooks will create.
        let out1 = home.join("out1.txt");
        let out2 = home.join("out2.txt");
        let out3 = home.join("out3.txt");
        let out4 = home.join("out4.txt");
        let out5 = home.join("out5.txt");

        // Create two plugin TOML files that run simple echo commands.
        let p1 = formatdoc! {r#"
            name = "plugin-one"

            [[hooks]]
            name = "write1"
            event = "PreToolUse"
            command = "sh -c 'echo plugin-one-write1 > {out1}'"
        "#, out1 = out1.display()};

        let p2 = formatdoc! {r#"
            name = "plugin-two"

            [[hooks]]
            name = "write2"
            event = "PreToolUse"
            matcher = "*"
            command = "sh -c 'echo plugin-two-write2 > {out2}'"

            [[hooks]]
            name = "write3"
            event = "PreToolUse"
            matcher = "Bash"
            command = "sh -c 'echo plugin-two-write3 > {out3}'"

            [[hooks]]
            name = "write4"
            event = "PreToolUse"
            matcher = "Bash|Read"
            command = "sh -c 'echo plugin-two-write4 > {out4}'"

            [[hooks]]
            name = "write4"
            event = "PreToolUse"
            matcher = "Read|Write"
            command = "sh -c 'echo plugin-two-write5 > {out5}'"
        "#,
            out2 = out2.display(),
            out3 = out3.display(),
            out4 = out4.display(),
            out5 = out5.display(),
        };

        fs::write(plugins_dir.join("plugin-one.toml"), p1).expect("write plugin1");
        fs::write(plugins_dir.join("plugin-two.toml"), p2).expect("write plugin2");

        let agent = ClaudeCode;
        let event_handler = agent.event(HookEvent::PreToolUse).unwrap();

        // Run the hook event via dispatch_plugin_hooks.
        let payload = ClaudeCodePreToolUsePayload {
            common_payload: ClaudeCodeHookCommonPayload {
                hook_event_name: "PreToolUse".to_string(),
            },
            tool_name: "Bash".to_string(),
            rest: serde_json::Map::new(),
        };
            let _ = event_handler.dispatch_plugin_hooks(
            &sym,
            Box::new(payload),
            Box::new(ClaudeCodePreToolUseOutput::default()),
        );

        // Verify files were created and contain expected contents.
        let got1 = fs::read_to_string(&out1).expect("read out1");
        let got2 = fs::read_to_string(&out2).expect("read out2");
        let got3 = fs::read_to_string(&out3).expect("read out3");
        let got4 = fs::read_to_string(&out4).expect("read out4");

        assert!(got1.contains("plugin-one-write1"));
        assert!(got2.contains("plugin-two-write2"));
        assert!(got3.contains("plugin-two-write3"));
        assert!(got4.contains("plugin-two-write4"));

        // No file created, matcher doesn't match
        assert!(fs::read_to_string(&out5).is_err());
    }

    #[tokio::test]
    async fn builtin_pre_tool_use_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(tmp.path());
        let payload = HookPayload {
            sub_payload: HookSubPayload::PreToolUse(PreToolUsePayload {
                tool_name: "Bash".to_string(),
            }),
            rest: serde_json::Map::new(),
        };
        let output = dispatch_builtin(&sym, &payload).await;
        assert!(output.hook_specific_output.is_none());
    }

    #[tokio::test]
    async fn builtin_post_tool_use_returns_empty_for_now() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(tmp.path());
        let payload = HookPayload {
            sub_payload: HookSubPayload::PostToolUse(PostToolUsePayload {
                tool_name: "Bash".to_string(),
                tool_input: serde_json::json!({"command": "ls"}),
                tool_response: serde_json::json!({"stdout": "file.rs"}),
                session_id: Some("test-session".to_string()),
                cwd: Some("/tmp".to_string()),
            }),
            rest: serde_json::Map::new(),
        };
        let output = dispatch_builtin(&sym, &payload).await;
        assert!(output.hook_specific_output.is_none());
    }

    #[tokio::test]
    async fn builtin_user_prompt_submit_returns_empty_for_now() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(tmp.path());
        let payload = HookPayload {
            sub_payload: HookSubPayload::UserPromptSubmit(UserPromptSubmitPayload {
                prompt: "Use tokio for async".to_string(),
                session_id: Some("test-session".to_string()),
                cwd: Some("/tmp".to_string()),
            }),
            rest: serde_json::Map::new(),
        };
        let output = dispatch_builtin(&sym, &payload).await;
        assert!(output.hook_specific_output.is_none());
    }

    #[test]
    fn hook_output_serializes_with_additional_context() {
        let output =
            HookOutput::with_context("UserPromptSubmit", "Load tokio guidance".to_string());
        let json = serde_json::to_value(&output).unwrap();
        assert_eq!(
            json["hookSpecificOutput"]["hookEventName"],
            "UserPromptSubmit"
        );
        assert_eq!(
            json["hookSpecificOutput"]["additionalContext"],
            "Load tokio guidance"
        );
    }

    #[test]
    fn hook_output_empty_serializes_without_hook_specific() {
        let output = HookOutput::empty();
        let json = serde_json::to_value(&output).unwrap();
        assert!(json.get("hookSpecificOutput").is_none());
    }

    // --- Activation detection unit tests ---

    #[test]
    fn detect_bash_crate_activation() {
        let post = PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({"command": "cargo agents crate tokio"}),
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
    fn detect_bash_crate_activation_cargo_agents_hyphen() {
        let post = PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({"command": "cargo-agents crate tokio"}),
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
        let post = PostToolUsePayload {
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
        let post = PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({"command": "cargo agents crate serde --version 1.0"}),
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
        let post = PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({"command": "cargo agents crate --list"}),
            tool_response: serde_json::json!({"exit_code": 0}),
            session_id: Some("s1".to_string()),
            cwd: Some("/tmp".to_string()),
        };
        assert_eq!(detect_crate_activation_bash(&post), None);
    }

    #[test]
    fn detect_bash_failed_not_activation() {
        let post = PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({"command": "cargo agents crate tokio"}),
            tool_response: serde_json::json!({"exit_code": 1}),
            session_id: Some("s1".to_string()),
            cwd: Some("/tmp".to_string()),
        };
        assert_eq!(detect_crate_activation_bash(&post), None);
    }

    #[test]
    fn detect_bash_crate_activation_with_path_prefix() {
        let post = PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({"command": "/home/user/.local/bin/cargo-agents crate serde"}),
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
        let post = PostToolUsePayload {
            tool_name: "mcp__cargo_agents__rust".to_string(),
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
        let post = PostToolUsePayload {
            tool_name: "mcp__cargo_agents__rust".to_string(),
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
        let post = PostToolUsePayload {
            tool_name: "mcp__cargo_agents__rust".to_string(),
            tool_input: serde_json::json!({"args": ["start"]}),
            tool_response: serde_json::json!({"output": "..."}),
            session_id: Some("s1".to_string()),
            cwd: Some("/tmp".to_string()),
        };
        assert_eq!(detect_crate_activation_mcp(&post), None);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn hook_stdout_is_merged_on_success() {
        setup_tracing();

        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path();

        let sym = Symposium::from_dir(home);

        let plugins_dir = sym.plugins_dir();

        let p1 = formatdoc! {r#"
            name = "plugin-json-a"

            [[hooks]]
            name = "json-a"
            event = "PreToolUse"
            command = "sh -c 'echo \"{{ \\\"hookEventName\\\": \\\"PreToolUse\\\", \\\"a\\\":1}}\"'"
        "#};

        let p2 = formatdoc! {r#"
            name = "plugin-json-b"

            [[hooks]]
            name = "json-b"
            event = "PreToolUse"
            command = "sh -c 'echo \"{{ \\\"hookEventName\\\": \\\"PreToolUse\\\", \\\"b\\\":2}}\"'"
        "#};

        fs::write(plugins_dir.join("plugin-json-a.toml"), p1).expect("write p1");
        fs::write(plugins_dir.join("plugin-json-b.toml"), p2).expect("write p2");

        let agent = ClaudeCode;
        let event_handler = agent.event(HookEvent::PreToolUse).unwrap();

        // Run the hook event via dispatch_plugin_hooks.
        let payload = ClaudeCodePreToolUsePayload {
            common_payload: ClaudeCodeHookCommonPayload {
                hook_event_name: "PreToolUse".to_string(),
            },
            tool_name: "Bash".to_string(),
            rest: serde_json::Map::new(),
        };
        let out = event_handler.dispatch_plugin_hooks(
            &sym,
            Box::new(payload),
            Box::new(ClaudeCodePreToolUseOutput::default()),
        );

        // Expect overall success and merged JSON containing both keys
        let PluginHookOutput::Success(val) = out else {
            panic!("expected success");
        };
        assert_eq!(val.get("a").and_then(|v| v.as_i64()), Some(1));
        assert_eq!(val.get("b").and_then(|v| v.as_i64()), Some(2));
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn hooks_exit_2_fail_fast_and_return_stderr() {
        setup_tracing();

        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path();

        let sym = Symposium::from_dir(home);

        let plugins_dir = sym.plugins_dir();

        let good = formatdoc! {r#"
            name = "plugin-good"

            [[hooks]]
            name = "good"
            event = "PreToolUse"
            command = "sh -c 'echo \"{{\\\"ok\\\":true}}\"'"
        "#};

        let bad = formatdoc! {r#"
            name = "plugin-bad"

            [[hooks]]
            name = "bad"
            event = "PreToolUse"
            command = "sh -c 'echo \\\"bad failure\\\" >&2; exit 2'"
        "#};

        // Arrange so the failing hook runs (order depends on plugin loading but two files are fine)
        fs::write(plugins_dir.join("plugin-good.toml"), good).expect("write good");
        fs::write(plugins_dir.join("plugin-bad.toml"), bad).expect("write bad");

        let agent = ClaudeCode;
        let event_handler = agent.event(HookEvent::PreToolUse).unwrap();

        // Run the hook event via dispatch_plugin_hooks.
        let payload = ClaudeCodePreToolUsePayload {
            common_payload: ClaudeCodeHookCommonPayload {
                hook_event_name: "PreToolUse".to_string(),
            },
            tool_name: "Bash".to_string(),
            rest: serde_json::Map::new(),
        };
        let out = event_handler.dispatch_plugin_hooks(
            &sym,
            Box::new(payload),
            Box::new(ClaudeCodePreToolUseOutput::default()),
        );

        // Expect failure and stderr containing our message
        let PluginHookOutput::Failure(stderr) = out else {
            panic!("expected failure");
        };
        let stderr_str = String::from_utf8_lossy(&stderr);
        assert!(stderr_str.contains("bad failure"));
    }
}
