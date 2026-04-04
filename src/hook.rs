use std::io::{Read, Write};
use std::process::{Command, ExitCode, Stdio};

use serde::{Deserialize, Serialize};

use crate::config::Symposium;
use crate::plugins::ParsedPlugin;

#[derive(Debug, Clone, clap::ValueEnum, Serialize, Deserialize, PartialEq, Eq)]
pub enum HookEvent {
    #[value(name = "pre-tool-use")]
    #[serde(rename = "PreToolUse")]
    PreToolUse,

    #[value(name = "post-tool-use")]
    #[serde(rename = "PostToolUse")]
    PostToolUse,

    #[value(name = "user-prompt-submit")]
    #[serde(rename = "UserPromptSubmit")]
    UserPromptSubmit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookPayload {
    #[serde(flatten)]
    pub sub_payload: HookSubPayload,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "hook_event_name")]
pub enum HookSubPayload {
    #[serde(rename = "PreToolUse")]
    PreToolUse(PreToolUsePayload),

    #[serde(rename = "PostToolUse")]
    PostToolUse(PostToolUsePayload),

    #[serde(rename = "UserPromptSubmit")]
    UserPromptSubmit(UserPromptSubmitPayload),
}

impl HookSubPayload {
    pub fn hook_event(&self) -> HookEvent {
        match self {
            HookSubPayload::PreToolUse(_) => HookEvent::PreToolUse,
            HookSubPayload::PostToolUse(_) => HookEvent::PostToolUse,
            HookSubPayload::UserPromptSubmit(_) => HookEvent::UserPromptSubmit,
        }
    }

    #[tracing::instrument(ret)]
    pub fn matches_matcher(&self, matcher: &str) -> bool {
        // TODO: I'm not sure what exactly Claude's rules are, but this is fine for now
        if matcher == "*" {
            return true;
        }
        match self {
            HookSubPayload::PreToolUse(payload) => matcher.contains(&payload.tool_name),
            HookSubPayload::PostToolUse(payload) => matcher.contains(&payload.tool_name),
            HookSubPayload::UserPromptSubmit(_) => true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreToolUsePayload {
    pub tool_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostToolUsePayload {
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    pub tool_response: serde_json::Value,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPromptSubmitPayload {
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

/// Structured output from built-in hook logic.
///
/// Serialized to JSON on stdout for Claude Code to consume.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HookOutput {
    /// If set, injected into the LLM conversation as additional context.
    #[serde(rename = "hookSpecificOutput", skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<HookSpecificOutput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookSpecificOutput {
    #[serde(rename = "hookEventName")]
    pub hook_event_name: String,
    #[serde(rename = "additionalContext", skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
}

impl HookOutput {
    /// Create a HookOutput with additional context for the given event.
    pub fn with_context(event_name: &str, context: String) -> Self {
        Self {
            hook_specific_output: Some(HookSpecificOutput {
                hook_event_name: event_name.to_string(),
                additional_context: Some(context),
            }),
        }
    }

    /// Create an empty HookOutput (no additional context).
    pub fn empty() -> Self {
        Self::default()
    }
}

/// CLI entry point: read payload from stdin, dispatch, print output.
pub async fn run(sym: &Symposium, event: HookEvent) -> ExitCode {
    let mut input = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut input) {
        tracing::warn!(?event, error = %e, "failed to read hook stdin");
        return ExitCode::SUCCESS;
    }

    let payload = serde_json::from_str::<HookPayload>(&input);
    let Ok(payload) = payload else {
        tracing::warn!(
            ?event,
            error = "invalid hook payload",
            "failed to parse hook stdin as JSON"
        );
        return ExitCode::FAILURE;
    };

    if payload.sub_payload.hook_event() != event {
        tracing::warn!(?event, payload_event = ?payload.sub_payload.hook_event(), "hook event mismatch between CLI arg and payload");
        return ExitCode::FAILURE;
    }

    // Run built-in hook logic
    let hook_output = dispatch_builtin(sym, &payload).await;

    // Run plugin hooks (for PreToolUse only, for now)
    dispatch_plugin_hooks(sym, &payload);

    // Emit structured output if there's anything to say
    if hook_output.hook_specific_output.is_some() {
        if let Ok(json) = serde_json::to_string(&hook_output) {
            println!("{json}");
        }
    }

    ExitCode::SUCCESS
}

/// Built-in hook logic. Returns typed `HookOutput` for the integration test harness.
pub async fn dispatch_builtin(sym: &Symposium, payload: &HookPayload) -> HookOutput {
    tracing::info!(?payload, "hook invoked (builtin)");

    match &payload.sub_payload {
        HookSubPayload::PreToolUse(_) => {
            // No built-in logic for PreToolUse — plugin hooks only.
            HookOutput::empty()
        }
        HookSubPayload::PostToolUse(post) => {
            handle_post_tool_use(sym, post).await
        }
        HookSubPayload::UserPromptSubmit(prompt) => {
            handle_user_prompt_submit(sym, prompt).await
        }
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

    // Open DB and ensure workspace data is fresh
    let mut db = match crate::state::open_db(sym.config_dir()).await {
        Ok(db) => db,
        Err(e) => {
            tracing::warn!(error = %e, "failed to open state DB in PostToolUse");
            return HookOutput::empty();
        }
    };

    if let Err(e) = crate::workspace::ensure_fresh(sym, &mut db, cwd).await {
        tracing::warn!(error = %e, "failed to refresh workspace in PostToolUse");
        return HookOutput::empty();
    }

    // Detect activation via symposium crate command (Bash tool)
    if let Some(crate_name) = detect_crate_activation_bash(post) {
        record_activation(&mut db, session_id, &crate_name).await;
    }

    // Detect activation via MCP rust tool with ["crate", "<name>"]
    if let Some(crate_name) = detect_crate_activation_mcp(post) {
        record_activation(&mut db, session_id, &crate_name).await;
    }

    // Detect activation via file path matching an AvailableSkill
    if let Some(crate_names) = detect_path_activation(&mut db, post, cwd_str).await {
        for crate_name in crate_names {
            record_activation(&mut db, session_id, &crate_name).await;
        }
    }

    HookOutput::empty()
}

/// Detect if a Bash tool successfully ran `symposium crate <name>`.
fn detect_crate_activation_bash(post: &PostToolUsePayload) -> Option<String> {
    if post.tool_name != "Bash" {
        return None;
    }

    // Check for successful exit
    let exit_code = post.tool_response.get("exit_code")?.as_i64()?;
    if exit_code != 0 {
        return None;
    }

    // Check if command contains "symposium crate <name>"
    let command = post.tool_input.get("command")?.as_str()?;
    let trimmed = command.trim();

    // Match patterns like "symposium crate tokio" or "symposium crate tokio --version 1.0"
    let rest = trimmed
        .strip_prefix("symposium crate ")
        .or_else(|| trimmed.strip_prefix("symposium crate\t"))?;

    // First word after "symposium crate " is the crate name (skip flags)
    let crate_name = rest.split_whitespace().find(|w| !w.starts_with('-'))?;

    if crate_name.is_empty() || crate_name == "--list" {
        return None;
    }

    Some(crate_name.to_string())
}

/// Detect if an MCP rust tool was called with ["crate", "<name>"].
fn detect_crate_activation_mcp(post: &PostToolUsePayload) -> Option<String> {
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

/// Detect if Read/Bash accessed a path matching an AvailableSkill.skill_dir_path.
async fn detect_path_activation(
    db: &mut toasty::Db,
    post: &PostToolUsePayload,
    cwd: &str,
) -> Option<Vec<String>> {
    let target_path = match post.tool_name.as_str() {
        "Read" => post.tool_input.get("file_path")?.as_str()?,
        "Bash" => {
            // Check if the command accessed a file path (heuristic: look at stdout for paths)
            // This is intentionally conservative — we only check Read tool paths.
            return None;
        }
        _ => return None,
    };

    use crate::state::AvailableSkill;
    // Look up AvailableSkill rows for this cwd where the path matches
    let available: Vec<AvailableSkill> =
        match AvailableSkill::filter_by_cwd(cwd).exec(db).await {
            Ok(skills) => skills,
            Err(_) => return None,
        };

    let mut crate_names = Vec::new();
    for skill in &available {
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

/// Record a skill activation in the DB.
async fn record_activation(db: &mut toasty::Db, session_id: &str, crate_name: &str) {
    use crate::state::SkillActivation;
    let now = chrono::Utc::now().to_rfc3339();
    if let Err(e) = toasty::create!(SkillActivation {
        session_id: session_id.to_string(),
        crate_name: crate_name.to_string(),
        activated_at: now,
    })
    .exec(db)
    .await
    {
        tracing::warn!(
            session_id,
            crate_name,
            error = %e,
            "failed to record skill activation"
        );
    } else {
        tracing::info!(session_id, crate_name, "recorded skill activation");
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

    // Open DB and ensure workspace data is fresh
    let mut db = match crate::state::open_db(sym.config_dir()).await {
        Ok(db) => db,
        Err(e) => {
            tracing::warn!(error = %e, "failed to open state DB in UserPromptSubmit");
            return HookOutput::empty();
        }
    };

    if let Err(e) = crate::workspace::ensure_fresh(sym, &mut db, cwd).await {
        tracing::warn!(error = %e, "failed to refresh workspace in UserPromptSubmit");
        return HookOutput::empty();
    }

    // Increment prompt count
    let prompt_count = increment_prompt_count(&mut db, session_id).await;

    // Get available skills for this cwd
    use crate::state::AvailableSkill;
    let available: Vec<AvailableSkill> =
        match AvailableSkill::filter_by_cwd(cwd_str).exec(&mut db).await {
            Ok(skills) => skills,
            Err(_) => return HookOutput::empty(),
        };

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

    // Check activations for this session
    use crate::state::SkillActivation;
    let activations: Vec<SkillActivation> =
        match SkillActivation::filter_by_session_id(session_id)
            .exec(&mut db)
            .await
        {
            Ok(a) => a,
            Err(_) => Vec::new(),
        };
    let activated_crates: std::collections::HashSet<&str> =
        activations.iter().map(|a| a.crate_name.as_str()).collect();

    // Check nudges for this session
    use crate::state::SkillNudge;
    let nudges: Vec<SkillNudge> = match SkillNudge::filter_by_session_id(session_id)
        .exec(&mut db)
        .await
    {
        Ok(n) => n,
        Err(_) => Vec::new(),
    };

    // Build nudge messages
    let mut nudge_crates = Vec::new();

    for crate_name in &mentioned {
        // Skip if already activated
        if activated_crates.contains(crate_name.as_str()) {
            continue;
        }

        // Find the most recent nudge for this crate (highest at_prompt)
        let existing_nudge = nudges
            .iter()
            .filter(|n| n.crate_name == *crate_name)
            .max_by_key(|n| n.at_prompt);

        match existing_nudge {
            None => {
                // Never nudged — nudge now
                nudge_crates.push(crate_name.clone());
                record_nudge(&mut db, session_id, crate_name, prompt_count).await;
            }
            Some(nudge) => {
                // Check if enough prompts have elapsed for re-nudge
                if prompt_count - nudge.at_prompt >= nudge_interval {
                    nudge_crates.push(crate_name.clone());
                    // We can't easily update a single nudge, so just record the new prompt count.
                    // The nudge check uses the latest at_prompt for each crate, so inserting
                    // a new row with the current prompt count effectively "updates" the nudge.
                    record_nudge(&mut db, session_id, crate_name, prompt_count).await;
                }
            }
        }
    }

    if nudge_crates.is_empty() {
        return HookOutput::empty();
    }

    // Format nudge message
    let mut context = String::new();
    for crate_name in &nudge_crates {
        context.push_str(&format!(
            "The `{crate_name}` crate has specialized guidance available.\n\
             To load it, run: `symposium crate {crate_name}`\n\n"
        ));
    }

    HookOutput::with_context("UserPromptSubmit", context.trim_end().to_string())
}

/// Increment the session prompt count, returning the new count.
async fn increment_prompt_count(db: &mut toasty::Db, session_id: &str) -> i64 {
    use crate::state::SessionState;

    match SessionState::get_by_session_id(db, session_id).await {
        Ok(mut state) => {
            state.prompt_count += 1;
            let count = state.prompt_count;
            let _ = state.update().exec(db).await;
            count
        }
        Err(_) => {
            // First prompt in this session
            let _ = toasty::create!(SessionState {
                session_id: session_id.to_string(),
                prompt_count: 1,
            })
            .exec(db)
            .await;
            1
        }
    }
}

/// Record a nudge in the DB.
async fn record_nudge(db: &mut toasty::Db, session_id: &str, crate_name: &str, at_prompt: i64) {
    use crate::state::SkillNudge;
    let _ = toasty::create!(SkillNudge {
        session_id: session_id.to_string(),
        crate_name: crate_name.to_string(),
        at_prompt: at_prompt,
    })
    .exec(db)
    .await;
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

/// Dispatch plugin hooks (spawn subprocesses).
fn dispatch_plugin_hooks(sym: &Symposium, payload: &HookPayload) {
    let plugins = crate::plugins::load_all_plugins(sym);
    let hooks = hooks_for_payload(&plugins, payload);

    for (plugin_name, hook) in hooks {
        tracing::info!(?plugin_name, hook = %hook.name, cmd = %hook.command, "running plugin hook");
        let spawn_res = Command::new("sh")
            .arg("-c")
            .arg(&hook.command)
            .stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn();

        match spawn_res {
            Ok(mut child) => {
                if let Some(mut stdin) = child.stdin.take() {
                    if let Err(e) =
                        stdin.write_all(serde_json::to_string(&payload).unwrap().as_bytes())
                    {
                        tracing::warn!(error = %e, "failed to write hook stdin");
                    }
                }

                match child.wait() {
                    Ok(status) => tracing::info!(?status, "hook finished"),
                    Err(e) => tracing::warn!(error = %e, "failed waiting for hook process"),
                }
            }
            Err(e) => tracing::warn!(error = %e, "failed to spawn hook command"),
        }
    }
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

        // Run the hook event via dispatch_plugin_hooks.
        let payload = HookPayload {
            sub_payload: HookSubPayload::PreToolUse(PreToolUsePayload {
                tool_name: "Bash".to_string(),
            }),
            rest: serde_json::Map::new(),
        };
        dispatch_plugin_hooks(&sym, &payload);

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
        let output = HookOutput::with_context("UserPromptSubmit", "Load tokio guidance".to_string());
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
            tool_input: serde_json::json!({"command": "symposium crate tokio"}),
            tool_response: serde_json::json!({"exit_code": 0, "stdout": "..."}),
            session_id: Some("s1".to_string()),
            cwd: Some("/tmp".to_string()),
        };
        assert_eq!(detect_crate_activation_bash(&post), Some("tokio".to_string()));
    }

    #[test]
    fn detect_bash_crate_activation_with_version() {
        let post = PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({"command": "symposium crate serde --version 1.0"}),
            tool_response: serde_json::json!({"exit_code": 0}),
            session_id: Some("s1".to_string()),
            cwd: Some("/tmp".to_string()),
        };
        assert_eq!(detect_crate_activation_bash(&post), Some("serde".to_string()));
    }

    #[test]
    fn detect_bash_crate_list_not_activation() {
        let post = PostToolUsePayload {
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
        let post = PostToolUsePayload {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({"command": "symposium crate tokio"}),
            tool_response: serde_json::json!({"exit_code": 1}),
            session_id: Some("s1".to_string()),
            cwd: Some("/tmp".to_string()),
        };
        assert_eq!(detect_crate_activation_bash(&post), None);
    }

    #[test]
    fn detect_mcp_crate_activation() {
        let post = PostToolUsePayload {
            tool_name: "mcp__symposium__rust".to_string(),
            tool_input: serde_json::json!({"args": ["crate", "tokio"]}),
            tool_response: serde_json::json!({"output": "..."}),
            session_id: Some("s1".to_string()),
            cwd: Some("/tmp".to_string()),
        };
        assert_eq!(detect_crate_activation_mcp(&post), Some("tokio".to_string()));
    }

    #[test]
    fn detect_mcp_crate_list_not_activation() {
        let post = PostToolUsePayload {
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
        let post = PostToolUsePayload {
            tool_name: "mcp__symposium__rust".to_string(),
            tool_input: serde_json::json!({"args": ["start"]}),
            tool_response: serde_json::json!({"output": "..."}),
            session_id: Some("s1".to_string()),
            cwd: Some("/tmp".to_string()),
        };
        assert_eq!(detect_crate_activation_mcp(&post), None);
    }
}
