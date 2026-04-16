//! Agent abstraction: hook registration and extension installation paths.
//!
//! Each supported agent has different conventions for where hooks are
//! configured and where skill files are placed. This module centralizes
//! that knowledge.

mod mcp_server_registration;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use serde_json::json;

use crate::config::Symposium;
use crate::output::{Output, display_path};

/// Supported AI agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    Claude,
    Codex,
    Copilot,
    Gemini,
    Goose,
    Kiro,
    OpenCode,
}

impl Agent {
    /// Parse an agent name from a config string.
    pub fn from_config_name(name: &str) -> Result<Self> {
        match name {
            "claude" => Ok(Agent::Claude),
            "codex" => Ok(Agent::Codex),
            "copilot" => Ok(Agent::Copilot),
            "gemini" => Ok(Agent::Gemini),
            "goose" => Ok(Agent::Goose),
            "kiro" => Ok(Agent::Kiro),
            "opencode" => Ok(Agent::OpenCode),
            other => bail!(
                "unknown agent: {other} (expected claude, codex, copilot, gemini, goose, kiro, or opencode)"
            ),
        }
    }

    /// Config name as stored in TOML.
    pub fn config_name(&self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
            Agent::Copilot => "copilot",
            Agent::Gemini => "gemini",
            Agent::Goose => "goose",
            Agent::Kiro => "kiro",
            Agent::OpenCode => "opencode",
        }
    }

    /// Human-readable display name.
    pub fn display_name(&self) -> &'static str {
        match self {
            Agent::Claude => "Claude Code",
            Agent::Codex => "Codex CLI",
            Agent::Copilot => "GitHub Copilot",
            Agent::Gemini => "Gemini CLI",
            Agent::Goose => "Goose",
            Agent::Kiro => "Kiro",
            Agent::OpenCode => "OpenCode",
        }
    }

    /// All supported agents for interactive prompts.
    pub fn all() -> &'static [Agent] {
        &[
            Agent::Claude,
            Agent::Codex,
            Agent::Copilot,
            Agent::Gemini,
            Agent::Goose,
            Agent::Kiro,
            Agent::OpenCode,
        ]
    }

    // -----------------------------------------------------------------------
    // Skill installation paths
    // -----------------------------------------------------------------------

    /// Project-level skill directory for a given skill name.
    ///
    /// Claude Code requires `.claude/skills/`, while Copilot and Gemini
    /// support the vendor-neutral `.agents/skills/` path.
    pub fn project_skill_dir(&self, project_root: &Path, skill_name: &str) -> PathBuf {
        match self {
            Agent::Claude => project_root.join(".claude").join("skills").join(skill_name),
            Agent::Codex | Agent::Copilot | Agent::Gemini => {
                project_root.join(".agents").join("skills").join(skill_name)
            }
            Agent::Goose => project_root.join(".agents").join("skills").join(skill_name),
            Agent::Kiro => project_root.join(".kiro").join("skills").join(skill_name),
            Agent::OpenCode => project_root.join(".agents").join("skills").join(skill_name),
        }
    }

    /// Global skill directory for a given skill name, if supported.
    pub fn global_skill_dir(&self, home: &Path, skill_name: &str) -> Option<PathBuf> {
        match self {
            Agent::Claude => Some(home.join(".claude").join("skills").join(skill_name)),
            Agent::Codex => Some(home.join(".agents").join("skills").join(skill_name)),
            Agent::Copilot => None, // no global skills path
            Agent::Gemini => Some(home.join(".gemini").join("skills").join(skill_name)),
            Agent::Goose => Some(home.join(".agents").join("skills").join(skill_name)),
            Agent::Kiro => Some(home.join(".kiro").join("skills").join(skill_name)),
            Agent::OpenCode => Some(home.join(".agents").join("skills").join(skill_name)),
        }
    }

    // -----------------------------------------------------------------------
    // Hook registration
    // -----------------------------------------------------------------------

    /// Register hooks in the project-level agent config.
    pub fn register_project_hooks(
        &self,
        project_root: &Path,
        _sym: &Symposium,
        out: &Output,
    ) -> Result<()> {
        match self {
            Agent::Claude => {
                register_claude_hooks(&project_root.join(".claude").join("settings.json"), out)
            }
            Agent::Codex => {
                register_codex_hooks(&project_root.join(".codex").join("hooks.json"), out)
            }
            Agent::Copilot => {
                register_copilot_hooks(&project_root.join(".github").join("hooks"), out)
            }
            Agent::Gemini => {
                register_gemini_hooks(&project_root.join(".gemini").join("settings.json"), out)
            }
            Agent::Kiro => register_kiro_hooks(&project_root.join(".kiro").join("agents"), out),
            Agent::Goose => {
                out.info(
                    "Goose uses MCP extensions for hooks; skipping hook registration (skills only)",
                );
                Ok(())
            }
            Agent::OpenCode => {
                out.info("OpenCode uses JS/TS plugins for hooks; skipping hook registration (skills only)");
                Ok(())
            }
        }?;

        Ok(())
    }

    /// Register hooks in the global agent config.
    pub fn register_global_hooks(&self, home: &Path, _sym: &Symposium, out: &Output) -> Result<()> {
        // Register hooks
        match self {
            Agent::Claude => {
                register_claude_hooks(&home.join(".claude").join("settings.json"), out)
            }
            Agent::Codex => register_codex_hooks(&home.join(".codex").join("hooks.json"), out),
            Agent::Copilot => {
                register_copilot_hooks_global(&home.join(".copilot").join("config.json"), out)
            }
            Agent::Gemini => {
                register_gemini_hooks(&home.join(".gemini").join("settings.json"), out)
            }
            Agent::Kiro => register_kiro_hooks(&home.join(".kiro").join("agents"), out),
            Agent::Goose => {
                out.info(
                    "Goose uses MCP extensions for hooks; skipping hook registration (skills only)",
                );
                Ok(())
            }
            Agent::OpenCode => {
                out.info("OpenCode uses JS/TS plugins for hooks; skipping hook registration (skills only)");
                Ok(())
            }
        }?;

        Ok(())
    }

    // -----------------------------------------------------------------------
    // MCP server registration
    // -----------------------------------------------------------------------

    /// Register MCP servers in the project-level agent config.
    pub fn register_project_mcp_servers(
        &self,
        project_root: &Path,
        servers: &[sacp::schema::McpServer],
        out: &Output,
    ) -> Result<()> {
        match self {
            Agent::Claude => mcp_server_registration::register_claude_mcp_servers(
                &project_root.join(".claude").join("settings.json"),
                servers,
                out,
            ),
            Agent::Codex => mcp_server_registration::register_codex_mcp_servers(
                &project_root.join(".codex").join("config.toml"),
                servers,
                out,
            ),
            Agent::Copilot => mcp_server_registration::register_copilot_mcp_servers(
                &project_root.join(".vscode").join("mcp.json"),
                servers,
                out,
            ),
            Agent::Gemini => mcp_server_registration::register_gemini_mcp_servers(
                &project_root.join(".gemini").join("settings.json"),
                servers,
                out,
            ),
            Agent::Kiro => mcp_server_registration::register_kiro_mcp_servers(
                &project_root.join(".kiro").join("settings").join("mcp.json"),
                servers,
                out,
            ),
            Agent::Goose => mcp_server_registration::register_goose_mcp_servers(
                &project_root.join(".goose").join("config.yaml"),
                servers,
                out,
            ),
            Agent::OpenCode => mcp_server_registration::register_opencode_mcp_servers(
                &project_root.join("opencode.json"),
                servers,
                out,
            ),
        }
    }

    /// Register MCP servers in the global agent config.
    pub fn register_global_mcp_servers(
        &self,
        home: &Path,
        servers: &[sacp::schema::McpServer],
        out: &Output,
    ) -> Result<()> {
        match self {
            Agent::Claude => mcp_server_registration::register_claude_mcp_servers(
                &home.join(".claude").join("settings.json"),
                servers,
                out,
            ),
            Agent::Codex => mcp_server_registration::register_codex_mcp_servers(
                &home.join(".codex").join("config.toml"),
                servers,
                out,
            ),
            Agent::Copilot => mcp_server_registration::register_copilot_mcp_servers(
                &home.join(".copilot").join("mcp-config.json"),
                servers,
                out,
            ),
            Agent::Gemini => mcp_server_registration::register_gemini_mcp_servers(
                &home.join(".gemini").join("settings.json"),
                servers,
                out,
            ),
            Agent::Kiro => mcp_server_registration::register_kiro_mcp_servers(
                &home.join(".kiro").join("settings").join("mcp.json"),
                servers,
                out,
            ),
            Agent::Goose => mcp_server_registration::register_goose_mcp_servers(
                &home.join(".config").join("goose").join("config.yaml"),
                servers,
                out,
            ),
            Agent::OpenCode => mcp_server_registration::register_opencode_mcp_servers(
                &home.join(".config").join("opencode").join("opencode.json"),
                servers,
                out,
            ),
        }
    }

    /// Remove MCP servers from the project-level agent config.
    pub fn unregister_project_mcp_servers(
        &self,
        project_root: &Path,
        names: &[&str],
        out: &Output,
    ) -> Result<()> {
        match self {
            Agent::Claude => mcp_server_registration::unregister_claude_mcp_servers(
                &project_root.join(".claude").join("settings.json"),
                names,
                out,
            ),
            Agent::Codex => mcp_server_registration::unregister_codex_mcp_servers(
                &project_root.join(".codex").join("config.toml"),
                names,
                out,
            ),
            Agent::Copilot => mcp_server_registration::unregister_copilot_mcp_servers(
                &project_root.join(".vscode").join("mcp.json"),
                names,
                out,
            ),
            Agent::Gemini => mcp_server_registration::unregister_gemini_mcp_servers(
                &project_root.join(".gemini").join("settings.json"),
                names,
                out,
            ),
            Agent::Kiro => mcp_server_registration::unregister_kiro_mcp_servers(
                &project_root.join(".kiro").join("settings").join("mcp.json"),
                names,
                out,
            ),
            Agent::Goose => mcp_server_registration::unregister_goose_mcp_servers(
                &project_root.join(".goose").join("config.yaml"),
                names,
                out,
            ),
            Agent::OpenCode => mcp_server_registration::unregister_opencode_mcp_servers(
                &project_root.join("opencode.json"),
                names,
                out,
            ),
        }
    }

    /// Remove MCP servers from the global agent config.
    pub fn unregister_global_mcp_servers(
        &self,
        home: &Path,
        names: &[&str],
        out: &Output,
    ) -> Result<()> {
        match self {
            Agent::Claude => mcp_server_registration::unregister_claude_mcp_servers(
                &home.join(".claude").join("settings.json"),
                names,
                out,
            ),
            Agent::Codex => mcp_server_registration::unregister_codex_mcp_servers(
                &home.join(".codex").join("config.toml"),
                names,
                out,
            ),
            Agent::Copilot => mcp_server_registration::unregister_copilot_mcp_servers(
                &home.join(".copilot").join("mcp-config.json"),
                names,
                out,
            ),
            Agent::Gemini => mcp_server_registration::unregister_gemini_mcp_servers(
                &home.join(".gemini").join("settings.json"),
                names,
                out,
            ),
            Agent::Kiro => mcp_server_registration::unregister_kiro_mcp_servers(
                &home.join(".kiro").join("settings").join("mcp.json"),
                names,
                out,
            ),
            Agent::Goose => mcp_server_registration::unregister_goose_mcp_servers(
                &home.join(".config").join("goose").join("config.yaml"),
                names,
                out,
            ),
            Agent::OpenCode => mcp_server_registration::unregister_opencode_mcp_servers(
                &home.join(".config").join("opencode").join("opencode.json"),
                names,
                out,
            ),
        }
    }

    /// Remove hooks from the project-level agent config.
    pub fn unregister_project_hooks(&self, project_root: &Path, _sym: &Symposium, out: &Output) {
        match self {
            Agent::Claude => {
                unregister_claude_hooks(&project_root.join(".claude").join("settings.json"), out)
            }
            Agent::Codex => {
                unregister_codex_hooks(&project_root.join(".codex").join("hooks.json"), out)
            }
            Agent::Copilot => {
                unregister_copilot_hooks(&project_root.join(".github").join("hooks"), out)
            }
            Agent::Gemini => {
                unregister_gemini_hooks(&project_root.join(".gemini").join("settings.json"), out)
            }
            Agent::Kiro => unregister_kiro_hooks(&project_root.join(".kiro").join("agents"), out),
            Agent::Goose => {}    // no hooks to unregister
            Agent::OpenCode => {} // no hooks to unregister
        }
    }

    /// Remove hooks from the global agent config.
    pub fn unregister_global_hooks(&self, home: &Path, _sym: &Symposium, out: &Output) {
        match self {
            Agent::Claude => {
                unregister_claude_hooks(&home.join(".claude").join("settings.json"), out)
            }
            Agent::Codex => unregister_codex_hooks(&home.join(".codex").join("hooks.json"), out),
            Agent::Copilot => {
                unregister_copilot_hooks_global(&home.join(".copilot").join("config.json"), out)
            }
            Agent::Gemini => {
                unregister_gemini_hooks(&home.join(".gemini").join("settings.json"), out)
            }
            Agent::Kiro => unregister_kiro_hooks(&home.join(".kiro").join("agents"), out),
            Agent::Goose => {}    // no hooks to unregister
            Agent::OpenCode => {} // no hooks to unregister
        }
    }

    /// Install a single skill file into the agent's expected location.
    pub fn install_skill(&self, skill_source: &Path, dest_dir: &Path) -> Result<()> {
        fs::create_dir_all(dest_dir)?;
        let dest_file = dest_dir.join("SKILL.md");
        fs::copy(skill_source, &dest_file)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Claude Code hook registration
// ---------------------------------------------------------------------------

fn register_claude_hooks(settings_path: &Path, out: &Output) -> Result<()> {
    let mut settings = load_json_or_empty(settings_path)?;
    let display = display_path(settings_path);

    let hooks = settings
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| json!({}));

    let hooks_obj = hooks.as_object_mut().unwrap();

    let mut added = Vec::new();

    for event in [
        "PreToolUse",
        "PostToolUse",
        "UserPromptSubmit",
        "SessionStart",
    ] {
        let command = format!("symposium hook claude {}", event_to_cli_arg(event));
        if ensure_claude_hook_entry(hooks_obj, event, &command) {
            added.push(event);
        }
    }

    if added.is_empty() {
        out.already_ok(format!("{display}: hooks already registered"));
    } else {
        save_json(settings_path, &settings)?;
        out.done(format!("{display}: added hooks ({})", added.join(", ")));
    }

    Ok(())
}

/// Returns `true` if a new entry was added, `false` if already registered.
fn ensure_claude_hook_entry(
    hooks: &mut serde_json::Map<String, serde_json::Value>,
    event: &str,
    command: &str,
) -> bool {
    let event_hooks = hooks.entry(event).or_insert_with(|| json!([]));

    let arr = match event_hooks.as_array_mut() {
        Some(a) => a,
        None => return false,
    };

    let already_registered = arr.iter().any(|group| {
        group
            .get("hooks")
            .and_then(|h| h.as_array())
            .map_or(false, |hooks| {
                hooks.iter().any(|h| {
                    h.get("command")
                        .and_then(|c| c.as_str())
                        .is_some_and(|c| c.starts_with("symposium hook"))
                })
            })
    });

    if already_registered {
        return false;
    }

    arr.push(json!({
        "matcher": "*",
        "hooks": [{
            "type": "command",
            "command": command
        }]
    }));
    true
}

// ---------------------------------------------------------------------------
// Codex CLI hook registration
// ---------------------------------------------------------------------------

fn register_codex_hooks(hooks_path: &Path, out: &Output) -> Result<()> {
    let mut settings = load_json_or_empty(hooks_path)?;
    let display = display_path(hooks_path);

    let hooks = settings
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| json!({}));

    let hooks_obj = hooks.as_object_mut().unwrap();

    let mut added = Vec::new();

    for event in [
        "PreToolUse",
        "PostToolUse",
        "UserPromptSubmit",
        "SessionStart",
    ] {
        let command = format!("symposium hook codex {}", event_to_cli_arg(event));
        if ensure_codex_hook_entry(hooks_obj, event, &command) {
            added.push(event);
        }
    }

    if added.is_empty() {
        out.already_ok(format!("{display}: hooks already registered"));
    } else {
        save_json(hooks_path, &settings)?;
        out.done(format!("{display}: added hooks ({})", added.join(", ")));
    }

    Ok(())
}

/// Returns `true` if a new entry was added, `false` if already registered.
fn ensure_codex_hook_entry(
    hooks: &mut serde_json::Map<String, serde_json::Value>,
    event: &str,
    command: &str,
) -> bool {
    let event_hooks = hooks.entry(event).or_insert_with(|| json!([]));

    let arr = match event_hooks.as_array_mut() {
        Some(a) => a,
        None => return false,
    };

    let already_registered = arr.iter().any(|group| {
        group
            .get("hooks")
            .and_then(|h| h.as_array())
            .map_or(false, |hooks| {
                hooks.iter().any(|h| {
                    h.get("command")
                        .and_then(|c| c.as_str())
                        .is_some_and(|c| c.starts_with("symposium hook"))
                })
            })
    });

    if already_registered {
        return false;
    }

    arr.push(json!({
        "matcher": "",
        "hooks": [{
            "type": "command",
            "command": command,
            "timeout": 10
        }]
    }));
    true
}

fn unregister_codex_hooks(hooks_path: &Path, out: &Output) {
    unregister_settings_hooks(hooks_path, "symposium hook", out);
}

// ---------------------------------------------------------------------------
// GitHub Copilot hook registration
// ---------------------------------------------------------------------------

/// Register hooks in the global Copilot config file (`~/.copilot/config.json`).
fn register_copilot_hooks_global(config_path: &Path, out: &Output) -> Result<()> {
    let display = display_path(config_path);
    let mut config = load_json_or_empty(config_path)?;

    let hooks = config
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| json!({}));

    let hooks_obj = hooks.as_object_mut().unwrap();

    // Check if already registered
    let already = hooks_obj.values().any(|arr| {
        arr.as_array().map_or(false, |a| {
            a.iter().any(|h| {
                h.get("bash")
                    .and_then(|c| c.as_str())
                    .is_some_and(|c| c.starts_with("symposium hook"))
            })
        })
    });

    if already {
        out.already_ok(format!("{display}: hooks already registered"));
        return Ok(());
    }

    let copilot_hooks = copilot_hook_entries();
    for (event, entry) in copilot_hooks {
        let arr = hooks_obj.entry(event).or_insert_with(|| json!([]));
        if let Some(a) = arr.as_array_mut() {
            a.push(entry);
        }
    }

    save_json(config_path, &config)?;
    out.done(format!("{display}: added hooks"));
    Ok(())
}

/// Register hooks in a project-level Copilot hooks directory (`.github/hooks/`).
fn register_copilot_hooks(hooks_dir: &Path, out: &Output) -> Result<()> {
    fs::create_dir_all(hooks_dir)?;
    let hook_file = hooks_dir.join("symposium.json");
    let display = display_path(&hook_file);

    if hook_file.exists() {
        let existing = fs::read_to_string(&hook_file)?;
        if existing.contains("symposium hook") {
            out.already_ok(format!("{display}: hooks already registered"));
            return Ok(());
        }
    }

    let mut hooks_obj = serde_json::Map::new();
    for (event, entry) in copilot_hook_entries() {
        hooks_obj.insert(event.to_string(), json!([entry]));
    }

    let hooks = json!({
        "version": 1,
        "hooks": hooks_obj
    });

    save_json(&hook_file, &hooks)?;
    out.done(format!("{display}: added hooks"));
    Ok(())
}

/// Copilot hook entries shared by global and project registration.
fn copilot_hook_entries() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        (
            "preToolUse",
            json!({
                "type": "command",
                "bash": "symposium hook copilot pre-tool-use",
                "timeoutSec": 10
            }),
        ),
        (
            "postToolUse",
            json!({
                "type": "command",
                "bash": "symposium hook copilot post-tool-use",
                "timeoutSec": 10
            }),
        ),
        (
            "userPromptSubmitted",
            json!({
                "type": "command",
                "bash": "symposium hook copilot user-prompt-submit",
                "timeoutSec": 10
            }),
        ),
        (
            "sessionStart",
            json!({
                "type": "command",
                "bash": "symposium hook copilot session-start",
                "timeoutSec": 10
            }),
        ),
    ]
}

// ---------------------------------------------------------------------------
// Gemini CLI hook registration
// ---------------------------------------------------------------------------

fn register_gemini_hooks(settings_path: &Path, out: &Output) -> Result<()> {
    let mut settings = load_json_or_empty(settings_path)?;
    let display = display_path(settings_path);

    let hooks = settings
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| json!({}));

    let hooks_obj = hooks.as_object_mut().unwrap();

    let mut added = Vec::new();

    let events = [
        ("BeforeTool", "pre-tool-use"),
        ("AfterTool", "post-tool-use"),
        ("SessionStart", "session-start"),
    ];

    for (gemini_event, cli_arg) in events {
        let command = format!("symposium hook gemini {cli_arg}");
        if ensure_gemini_hook_entry(hooks_obj, gemini_event, &command) {
            added.push(gemini_event);
        }
    }

    if added.is_empty() {
        out.already_ok(format!("{display}: hooks already registered"));
    } else {
        save_json(settings_path, &settings)?;
        out.done(format!("{display}: added hooks ({})", added.join(", ")));
    }

    Ok(())
}

/// Returns `true` if a new entry was added, `false` if already registered.
fn ensure_gemini_hook_entry(
    hooks: &mut serde_json::Map<String, serde_json::Value>,
    event: &str,
    command: &str,
) -> bool {
    let event_hooks = hooks.entry(event).or_insert_with(|| json!([]));

    let arr = match event_hooks.as_array_mut() {
        Some(a) => a,
        None => return false,
    };

    let already_registered = arr.iter().any(|group| {
        group
            .get("hooks")
            .and_then(|h| h.as_array())
            .map_or(false, |hooks| {
                hooks.iter().any(|h| {
                    h.get("command")
                        .and_then(|c| c.as_str())
                        .is_some_and(|c| c.starts_with("symposium hook"))
                })
            })
    });

    if already_registered {
        return false;
    }

    arr.push(json!({
        "matcher": ".*",
        "hooks": [{
            "name": "symposium",
            "type": "command",
            "command": command,
            "timeout": 10000
        }]
    }));
    true
}

// ---------------------------------------------------------------------------
// Kiro hook registration
// ---------------------------------------------------------------------------

/// Merge Kiro hook entries into a JSON config, returning the list of newly added events.
///
/// Also ensures the required `name` field is present (Kiro validates it on load).
/// Returns `(changed, added_events)` — `changed` is true if any field was
/// inserted (not just hooks), so the caller knows to save the file.
fn merge_kiro_hooks(
    config: &mut serde_json::Value,
    default_name: &str,
) -> (bool, Vec<&'static str>) {
    let obj = config.as_object_mut().unwrap();
    let mut changed = false;

    // Use a helper to track insertions.
    let mut ensure = |key: &str, value: serde_json::Value| {
        if !obj.contains_key(key) {
            obj.insert(key.to_string(), value);
            changed = true;
        }
    };

    ensure("name", json!(default_name));

    // Without `tools`, the agent has zero tools available.
    ensure("tools", json!(["*"]));

    // Auto-discover skills from the standard locations.
    ensure("resources", json!(["skill://.kiro/skills/**/SKILL.md",]));

    let hooks = obj.entry("hooks").or_insert_with(|| {
        changed = true;
        json!({})
    });

    let hooks_obj = hooks.as_object_mut().unwrap();

    let mut added = Vec::new();
    for (event, entry) in kiro_hook_entries() {
        if ensure_kiro_hook_entry(hooks_obj, event, &entry) {
            added.push(event);
            changed = true;
        }
    }
    (changed, added)
}

/// Register hooks by creating a Kiro agent file (`.kiro/agents/symposium.json`).
fn register_kiro_hooks(agents_dir: &Path, out: &Output) -> Result<()> {
    fs::create_dir_all(agents_dir)?;
    let hook_file = agents_dir.join("symposium.json");
    let display = display_path(&hook_file);

    let mut config = load_json_or_empty(&hook_file)?;
    let (changed, added) = merge_kiro_hooks(&mut config, "symposium");

    if !changed {
        out.already_ok(format!("{display}: hooks already registered"));
    } else {
        save_json(&hook_file, &config)?;
        if added.is_empty() {
            out.done(format!("{display}: updated agent config"));
        } else {
            out.done(format!("{display}: added hooks ({})", added.join(", ")));
        }
    }

    Ok(())
}

/// Returns `true` if a new entry was added, `false` if already registered.
fn ensure_kiro_hook_entry(
    hooks: &mut serde_json::Map<String, serde_json::Value>,
    event: &str,
    entry: &serde_json::Value,
) -> bool {
    let event_hooks = hooks.entry(event).or_insert_with(|| json!([]));

    let arr = match event_hooks.as_array_mut() {
        Some(a) => a,
        None => return false,
    };

    // Kiro uses a flat structure: each entry has `command` directly (no nested `hooks` array)
    let already_registered = arr.iter().any(|e| {
        e.get("command")
            .and_then(|c| c.as_str())
            .is_some_and(|c| c.starts_with("symposium hook"))
    });

    if already_registered {
        return false;
    }

    arr.push(entry.clone());
    true
}

/// Kiro hook entries for all supported events.
fn kiro_hook_entries() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        (
            "preToolUse",
            json!({
                "matcher": "*",
                "command": "symposium hook kiro pre-tool-use"
            }),
        ),
        (
            "postToolUse",
            json!({
                "matcher": "*",
                "command": "symposium hook kiro post-tool-use"
            }),
        ),
        (
            "userPromptSubmit",
            json!({
                "command": "symposium hook kiro user-prompt-submit"
            }),
        ),
        (
            "agentSpawn",
            json!({
                "command": "symposium hook kiro session-start"
            }),
        ),
    ]
}

/// Remove the symposium agent file from a Kiro agents directory.
fn unregister_kiro_hooks(agents_dir: &Path, out: &Output) {
    let hook_file = agents_dir.join("symposium.json");
    if hook_file.exists() {
        let display = display_path(&hook_file);
        if fs::remove_file(&hook_file).is_ok() {
            out.removed(format!("{display}: removed hooks"));
        }
    }
}

// ---------------------------------------------------------------------------
// Hook unregistration
// ---------------------------------------------------------------------------

/// Remove symposium hooks from a Claude/Gemini settings.json file.
/// Shared by both Claude and Gemini since they use the same structure.
fn unregister_settings_hooks(settings_path: &Path, command_prefix: &str, out: &Output) {
    let display = display_path(settings_path);

    let Ok(mut settings) = load_json_or_empty(settings_path) else {
        return;
    };

    let Some(hooks) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) else {
        return;
    };

    let mut changed = false;
    for (_event, arr) in hooks.iter_mut() {
        if let Some(groups) = arr.as_array_mut() {
            let before = groups.len();
            groups.retain(|group| {
                !group
                    .get("hooks")
                    .and_then(|h| h.as_array())
                    .map_or(false, |hooks| {
                        hooks.iter().any(|h| {
                            h.get("command")
                                .and_then(|c| c.as_str())
                                .is_some_and(|c| c.starts_with(command_prefix))
                        })
                    })
            });
            if groups.len() < before {
                changed = true;
            }
        }
    }

    if changed {
        if let Ok(()) = save_json(settings_path, &settings) {
            out.removed(format!("{display}: removed hooks"));
        }
    }
}

fn unregister_claude_hooks(settings_path: &Path, out: &Output) {
    unregister_settings_hooks(settings_path, "symposium hook", out);
}

fn unregister_gemini_hooks(settings_path: &Path, out: &Output) {
    unregister_settings_hooks(settings_path, "symposium hook", out);
}

/// Remove symposium hooks from a Copilot project hooks directory.
fn unregister_copilot_hooks(hooks_dir: &Path, out: &Output) {
    let hook_file = hooks_dir.join("symposium.json");
    if hook_file.exists() {
        let display = display_path(&hook_file);
        if fs::remove_file(&hook_file).is_ok() {
            out.removed(format!("{display}: removed hooks"));
        }
    }
}

/// Remove symposium hooks from the global Copilot config.
fn unregister_copilot_hooks_global(config_path: &Path, out: &Output) {
    unregister_flat_hooks(config_path, "bash", out);
}

/// Remove symposium hooks from a JSON config where entries are flat objects
/// with the command in `command_key` (e.g., `"command"` for Kiro, `"bash"` for Copilot).
///
/// Contrasts with `unregister_settings_hooks` which handles the nested
/// `{ "hooks": [{ "command": "..." }] }` structure used by Claude/Gemini/Codex.
fn unregister_flat_hooks(config_path: &Path, command_key: &str, out: &Output) {
    let display = display_path(config_path);

    let Ok(mut config) = load_json_or_empty(config_path) else {
        return;
    };

    let Some(hooks) = config.get_mut("hooks").and_then(|h| h.as_object_mut()) else {
        return;
    };

    let mut changed = false;
    for (_event, arr) in hooks.iter_mut() {
        if let Some(entries) = arr.as_array_mut() {
            let before = entries.len();
            entries.retain(|entry| {
                !entry
                    .get(command_key)
                    .and_then(|c| c.as_str())
                    .is_some_and(|c| c.starts_with("symposium hook"))
            });
            if entries.len() < before {
                changed = true;
            }
        }
    }

    if changed {
        if let Ok(()) = save_json(config_path, &config) {
            out.removed(format!("{display}: removed hooks"));
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn event_to_cli_arg(event: &str) -> &str {
    match event {
        "PreToolUse" | "preToolUse" => "pre-tool-use",
        "PostToolUse" | "postToolUse" => "post-tool-use",
        "UserPromptSubmit" | "userPromptSubmit" => "user-prompt-submit",
        "SessionStart" | "sessionStart" | "agentSpawn" => "session-start",
        other => other,
    }
}

fn load_json_or_empty(path: &Path) -> Result<serde_json::Value> {
    if path.exists() {
        let contents = fs::read_to_string(path)?;
        if contents.trim().is_empty() {
            return Ok(json!({}));
        }
        Ok(serde_json::from_str(&contents)?)
    } else {
        Ok(json!({}))
    }
}

fn save_json(path: &Path, value: &serde_json::Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let contents = serde_json::to_string_pretty(value)?;
    fs::write(path, contents)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_from_config_name() {
        assert_eq!(Agent::from_config_name("claude").unwrap(), Agent::Claude);
        assert_eq!(Agent::from_config_name("codex").unwrap(), Agent::Codex);
        assert_eq!(Agent::from_config_name("copilot").unwrap(), Agent::Copilot);
        assert_eq!(Agent::from_config_name("gemini").unwrap(), Agent::Gemini);
        assert!(Agent::from_config_name("unknown").is_err());
    }

    #[test]
    fn claude_project_skill_dir() {
        let root = Path::new("/project");
        assert_eq!(
            Agent::Claude.project_skill_dir(root, "tokio"),
            PathBuf::from("/project/.claude/skills/tokio")
        );
    }

    #[test]
    fn copilot_project_skill_dir_uses_vendor_neutral() {
        let root = Path::new("/project");
        assert_eq!(
            Agent::Copilot.project_skill_dir(root, "tokio"),
            PathBuf::from("/project/.agents/skills/tokio")
        );
    }

    #[test]
    fn register_claude_hooks_creates_settings() {
        let tmp = tempfile::tempdir().unwrap();
        let settings_path = tmp.path().join("settings.json");
        register_claude_hooks(&settings_path, &Output::quiet()).unwrap();

        let settings: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
        let hooks = settings.get("hooks").unwrap();

        assert!(hooks.get("PreToolUse").is_some());
        assert!(hooks.get("PostToolUse").is_some());
        assert!(hooks.get("UserPromptSubmit").is_some());
    }

    #[test]
    fn register_claude_hooks_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let settings_path = tmp.path().join("settings.json");
        register_claude_hooks(&settings_path, &Output::quiet()).unwrap();
        register_claude_hooks(&settings_path, &Output::quiet()).unwrap();

        let settings: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
        let pre_tool = settings["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre_tool.len(), 1);
    }

    #[test]
    fn codex_project_skill_dir_uses_vendor_neutral() {
        let root = Path::new("/project");
        assert_eq!(
            Agent::Codex.project_skill_dir(root, "tokio"),
            PathBuf::from("/project/.agents/skills/tokio")
        );
    }

    #[test]
    fn register_codex_hooks_creates_settings() {
        let tmp = tempfile::tempdir().unwrap();
        let hooks_path = tmp.path().join("hooks.json");
        register_codex_hooks(&hooks_path, &Output::quiet()).unwrap();

        let settings: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&hooks_path).unwrap()).unwrap();
        let hooks = settings.get("hooks").unwrap();

        assert!(hooks.get("PreToolUse").is_some());
        assert!(hooks.get("PostToolUse").is_some());
        assert!(hooks.get("UserPromptSubmit").is_some());
        assert!(hooks.get("SessionStart").is_some());

        // Verify the structure uses empty matcher
        let pre_tool = hooks["PreToolUse"].as_array().unwrap();
        assert_eq!(pre_tool[0]["matcher"], "");
    }

    #[test]
    fn register_codex_hooks_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let hooks_path = tmp.path().join("hooks.json");
        register_codex_hooks(&hooks_path, &Output::quiet()).unwrap();
        register_codex_hooks(&hooks_path, &Output::quiet()).unwrap();

        let settings: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&hooks_path).unwrap()).unwrap();
        let pre_tool = settings["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre_tool.len(), 1);
    }

    #[test]
    fn register_copilot_hooks_creates_file() {
        let tmp = tempfile::tempdir().unwrap();
        let hooks_dir = tmp.path().join("hooks");
        register_copilot_hooks(&hooks_dir, &Output::quiet()).unwrap();

        let hook_file = hooks_dir.join("symposium.json");
        assert!(hook_file.exists());
        let content: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&hook_file).unwrap()).unwrap();
        assert_eq!(content["version"], 1);
        assert!(content["hooks"]["preToolUse"].is_array());
    }

    #[test]
    fn agent_from_config_name_kiro() {
        assert_eq!(Agent::from_config_name("kiro").unwrap(), Agent::Kiro);
    }

    #[test]
    fn kiro_project_skill_dir() {
        let root = Path::new("/project");
        assert_eq!(
            Agent::Kiro.project_skill_dir(root, "tokio"),
            PathBuf::from("/project/.kiro/skills/tokio")
        );
    }

    #[test]
    fn kiro_global_skill_dir() {
        let home = Path::new("/home/user");
        assert_eq!(
            Agent::Kiro.global_skill_dir(home, "tokio"),
            Some(PathBuf::from("/home/user/.kiro/skills/tokio"))
        );
    }

    #[test]
    fn register_kiro_hooks_creates_file() {
        let tmp = tempfile::tempdir().unwrap();
        let agents_dir = tmp.path().join("agents");
        register_kiro_hooks(&agents_dir, &Output::quiet()).unwrap();

        let hook_file = agents_dir.join("symposium.json");
        assert!(hook_file.exists());
        let content: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&hook_file).unwrap()).unwrap();
        assert_eq!(content["name"], "symposium");
        assert!(content["hooks"]["preToolUse"].is_array());
        assert!(content["hooks"]["postToolUse"].is_array());
        assert!(content["hooks"]["userPromptSubmit"].is_array());
        assert!(content["hooks"]["agentSpawn"].is_array());

        // Verify flat format (command directly on entry, no nested hooks array)
        let pre_tool = &content["hooks"]["preToolUse"][0];
        assert_eq!(pre_tool["command"], "symposium hook kiro pre-tool-use");
        assert_eq!(pre_tool["matcher"], "*");
        assert!(pre_tool.get("hooks").is_none());
    }

    #[test]
    fn register_kiro_hooks_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let agents_dir = tmp.path().join("agents");
        register_kiro_hooks(&agents_dir, &Output::quiet()).unwrap();
        register_kiro_hooks(&agents_dir, &Output::quiet()).unwrap();

        let hook_file = agents_dir.join("symposium.json");
        let content: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&hook_file).unwrap()).unwrap();
        let pre_tool = content["hooks"]["preToolUse"].as_array().unwrap();
        assert_eq!(pre_tool.len(), 1);
    }

    #[test]
    fn unregister_kiro_hooks_removes_file() {
        let tmp = tempfile::tempdir().unwrap();
        let agents_dir = tmp.path().join("agents");
        register_kiro_hooks(&agents_dir, &Output::quiet()).unwrap();

        let hook_file = agents_dir.join("symposium.json");
        assert!(hook_file.exists());

        unregister_kiro_hooks(&agents_dir, &Output::quiet());
        assert!(!hook_file.exists());
    }

    #[test]
    fn agent_from_config_name_opencode() {
        assert_eq!(
            Agent::from_config_name("opencode").unwrap(),
            Agent::OpenCode
        );
    }

    #[test]
    fn opencode_project_skill_dir_uses_vendor_neutral() {
        let root = Path::new("/project");
        assert_eq!(
            Agent::OpenCode.project_skill_dir(root, "tokio"),
            PathBuf::from("/project/.agents/skills/tokio")
        );
    }

    #[test]
    fn opencode_global_skill_dir() {
        let home = Path::new("/home/user");
        assert_eq!(
            Agent::OpenCode.global_skill_dir(home, "tokio"),
            Some(PathBuf::from("/home/user/.agents/skills/tokio"))
        );
    }

    #[test]
    fn agent_from_config_name_goose() {
        assert_eq!(Agent::from_config_name("goose").unwrap(), Agent::Goose);
    }

    #[test]
    fn goose_project_skill_dir_uses_vendor_neutral() {
        let root = Path::new("/project");
        assert_eq!(
            Agent::Goose.project_skill_dir(root, "tokio"),
            PathBuf::from("/project/.agents/skills/tokio")
        );
    }

    #[test]
    fn goose_global_skill_dir() {
        let home = Path::new("/home/user");
        assert_eq!(
            Agent::Goose.global_skill_dir(home, "tokio"),
            Some(PathBuf::from("/home/user/.agents/skills/tokio"))
        );
    }

    #[test]
    fn register_gemini_hooks_creates_settings() {
        let tmp = tempfile::tempdir().unwrap();
        let settings_path = tmp.path().join("settings.json");
        register_gemini_hooks(&settings_path, &Output::quiet()).unwrap();

        let settings: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert!(settings["hooks"]["BeforeTool"].is_array());
        assert!(settings["hooks"]["AfterTool"].is_array());
    }
}
