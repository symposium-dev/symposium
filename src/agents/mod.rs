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

/// Which of an agent's two MCP configuration levels to write.
///
/// Distinct from [`crate::config::HookScope`]: an agent may support one level
/// and not the other, so this is a preference, not a guarantee (see
/// [`Agent::supports_project_mcp_scope`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpScope {
    /// Applies to one workspace.
    Project,
    /// Applies to every project this user opens.
    User,
}

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
    pub fn register_hooks(&self, home: &Path, _sym: &Symposium, out: &Output) -> Result<()> {
        tracing::debug!(agent = %self.config_name(), "registering hooks");
        // Register hooks
        match self {
            Agent::Claude => {
                register_claude_hooks(&home.join(".claude").join("settings.json"), out)
            }
            Agent::Codex => register_codex_hooks(&home.join(".codex").join("hooks.json"), out),
            Agent::Copilot => {
                register_copilot_hooks_global(&home.join(".copilot").join("settings.json"), out)
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

    /// Where an agent reads MCP servers from.
    ///
    /// Deliberately not the file its *hooks* live in: several agents keep the
    /// two apart, and writing MCP entries into the hooks file means the agent
    /// never sees them.
    ///
    /// Honors each tool's relocation env var (`CLAUDE_CONFIG_DIR`,
    /// `XDG_CONFIG_HOME`), or a user who moved their config gets a file the
    /// agent never reads.
    pub fn mcp_config_path(&self, scope: McpScope, project_root: &Path, home: &Path) -> PathBuf {
        let env_dir = |name: &str| {
            std::env::var_os(name)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        let xdg_config = || {
            env_dir("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"))
        };

        match (self, scope) {
            // Project MCP is `.mcp.json`; the user-level file is `.claude.json`.
            // Neither is `settings.json`, which holds hooks.
            (Agent::Claude, McpScope::Project) => project_root.join(".mcp.json"),
            (Agent::Claude, McpScope::User) => env_dir("CLAUDE_CONFIG_DIR")
                .unwrap_or_else(|| home.to_path_buf())
                .join(".claude.json"),

            (Agent::Gemini, McpScope::Project) => project_root.join(".gemini").join("settings.json"),
            (Agent::Gemini, McpScope::User) => home.join(".gemini").join("settings.json"),

            (Agent::OpenCode, McpScope::Project) => project_root.join("opencode.json"),
            (Agent::OpenCode, McpScope::User) => {
                xdg_config().join("opencode").join("opencode.json")
            }

            (Agent::Kiro, McpScope::Project) => {
                project_root.join(".kiro").join("settings").join("mcp.json")
            }
            (Agent::Kiro, McpScope::User) => home.join(".kiro").join("settings").join("mcp.json"),

            // No project-level MCP config exists for these: the CLI reads the
            // user-level file only (Codex and Copilot verified by asking them
            // from a project holding an entry: both reported none). Project
            // scope therefore resolves to the same user-level file rather than
            // to a file nobody reads.
            (Agent::Codex, _) => home.join(".codex").join("config.toml"),
            (Agent::Copilot, _) => home.join(".copilot").join("mcp-config.json"),
            (Agent::Goose, _) => xdg_config().join("goose").join("config.yaml"),
        }
    }

    /// Whether this agent honors project-scoped MCP registration at all.
    ///
    /// `false` means [`Self::mcp_config_path`] ignores the requested scope and
    /// answers with the user-level file, which callers may want to report.
    pub fn supports_project_mcp_scope(&self) -> bool {
        matches!(
            self,
            Agent::Claude | Agent::Gemini | Agent::OpenCode | Agent::Kiro
        )
    }

    /// Register MCP servers in the agent's config for `scope`.
    pub fn register_mcp_servers(
        &self,
        scope: McpScope,
        project_root: &Path,
        home: &Path,
        servers: &[sacp::schema::McpServer],
        out: &Output,
    ) -> Result<()> {
        tracing::debug!(agent = %self.config_name(), count = servers.len(), ?scope, "registering MCP servers");
        let path = self.mcp_config_path(scope, project_root, home);
        match self {
            Agent::Claude => {
                mcp_server_registration::register_claude_mcp_servers(&path, servers, out)
            }
            Agent::Codex => mcp_server_registration::register_codex_mcp_servers(&path, servers, out),
            Agent::Copilot => {
                mcp_server_registration::register_copilot_mcp_servers(&path, servers, out)
            }
            Agent::Gemini => {
                mcp_server_registration::register_gemini_mcp_servers(&path, servers, out)
            }
            Agent::Kiro => mcp_server_registration::register_kiro_mcp_servers(&path, servers, out),
            Agent::Goose => mcp_server_registration::register_goose_mcp_servers(&path, servers, out),
            Agent::OpenCode => {
                mcp_server_registration::register_opencode_mcp_servers(&path, servers, out)
            }
        }
    }

    /// Remove MCP servers from the agent's config for `scope`.
    pub fn unregister_mcp_servers(
        &self,
        scope: McpScope,
        project_root: &Path,
        home: &Path,
        names: &[&str],
        out: &Output,
    ) -> Result<()> {
        let path = self.mcp_config_path(scope, project_root, home);
        match self {
            Agent::Claude => {
                mcp_server_registration::unregister_claude_mcp_servers(&path, names, out)
            }
            Agent::Codex => mcp_server_registration::unregister_codex_mcp_servers(&path, names, out),
            Agent::Copilot => {
                mcp_server_registration::unregister_copilot_mcp_servers(&path, names, out)
            }
            Agent::Gemini => {
                mcp_server_registration::unregister_gemini_mcp_servers(&path, names, out)
            }
            Agent::Kiro => mcp_server_registration::unregister_kiro_mcp_servers(&path, names, out),
            Agent::Goose => mcp_server_registration::unregister_goose_mcp_servers(&path, names, out),
            Agent::OpenCode => {
                mcp_server_registration::unregister_opencode_mcp_servers(&path, names, out)
            }
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
    pub fn unregister_hooks(&self, home: &Path, _sym: &Symposium, out: &Output) {
        match self {
            Agent::Claude => {
                unregister_claude_hooks(&home.join(".claude").join("settings.json"), out)
            }
            Agent::Codex => unregister_codex_hooks(&home.join(".codex").join("hooks.json"), out),
            Agent::Copilot => {
                unregister_copilot_hooks_global(&home.join(".copilot").join("settings.json"), out)
            }
            Agent::Gemini => {
                unregister_gemini_hooks(&home.join(".gemini").join("settings.json"), out)
            }
            Agent::Kiro => unregister_kiro_hooks(&home.join(".kiro").join("agents"), out),
            Agent::Goose => {}    // no hooks to unregister
            Agent::OpenCode => {} // no hooks to unregister
        }
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
        "Stop",
    ] {
        let command = format!("cargo-agents hook claude {}", event_to_cli_arg(event));
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
            .is_some_and(|hooks| {
                hooks.iter().any(|h| {
                    h.get("command")
                        .and_then(|c| c.as_str())
                        .is_some_and(|c| c.starts_with("cargo-agents hook"))
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
        let command = format!("cargo-agents hook codex {}", event_to_cli_arg(event));
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
            .is_some_and(|hooks| {
                hooks.iter().any(|h| {
                    h.get("command")
                        .and_then(|c| c.as_str())
                        .is_some_and(|c| c.starts_with("cargo-agents hook"))
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
    unregister_settings_hooks(hooks_path, "cargo-agents hook", out);
}

// ---------------------------------------------------------------------------
// GitHub Copilot hook registration
// ---------------------------------------------------------------------------

/// Register hooks in the global Copilot user settings file (`~/.copilot/settings.json`).
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
        arr.as_array().is_some_and(|a| {
            a.iter().any(|h| {
                h.get("bash")
                    .and_then(|c| c.as_str())
                    .is_some_and(|c| c.starts_with("cargo-agents hook"))
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
        if existing.contains("cargo-agents hook") {
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
                "bash": "cargo-agents hook copilot pre-tool-use",
                "timeoutSec": 10
            }),
        ),
        (
            "postToolUse",
            json!({
                "type": "command",
                "bash": "cargo-agents hook copilot post-tool-use",
                "timeoutSec": 10
            }),
        ),
        (
            "userPromptSubmitted",
            json!({
                "type": "command",
                "bash": "cargo-agents hook copilot user-prompt-submit",
                "timeoutSec": 10
            }),
        ),
        (
            "sessionStart",
            json!({
                "type": "command",
                "bash": "cargo-agents hook copilot session-start",
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
        ("BeforeAgent", "user-prompt-submit"),
        ("SessionStart", "session-start"),
    ];

    for (gemini_event, cli_arg) in events {
        let command = format!("cargo-agents hook gemini {cli_arg}");
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
            .is_some_and(|hooks| {
                hooks.iter().any(|h| {
                    h.get("command")
                        .and_then(|c| c.as_str())
                        .is_some_and(|c| c.starts_with("cargo-agents hook"))
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
            .is_some_and(|c| c.starts_with("cargo-agents hook"))
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
                "command": "cargo-agents hook kiro pre-tool-use"
            }),
        ),
        (
            "postToolUse",
            json!({
                "matcher": "*",
                "command": "cargo-agents hook kiro post-tool-use"
            }),
        ),
        (
            "userPromptSubmit",
            json!({
                "command": "cargo-agents hook kiro user-prompt-submit"
            }),
        ),
        (
            "agentSpawn",
            json!({
                "command": "cargo-agents hook kiro session-start"
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
                    .is_some_and(|hooks| {
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

    if changed && let Ok(()) = save_json(settings_path, &settings) {
        out.removed(format!("{display}: removed hooks"));
    }
}

fn unregister_claude_hooks(settings_path: &Path, out: &Output) {
    unregister_settings_hooks(settings_path, "cargo-agents hook", out);
}

fn unregister_gemini_hooks(settings_path: &Path, out: &Output) {
    unregister_settings_hooks(settings_path, "cargo-agents hook", out);
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
                    .is_some_and(|c| c.starts_with("cargo-agents hook"))
            });
            if entries.len() < before {
                changed = true;
            }
        }
    }

    if changed && let Ok(()) = save_json(config_path, &config) {
        out.removed(format!("{display}: removed hooks"));
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
        "Stop" | "stop" => "stop",
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

/// Write JSON config, replacing the file atomically.
///
/// Temp file plus rename, because some of these are live agent state (Claude
/// Code rewrites `~/.claude.json` throughout a session) and a truncating write
/// that loses a race leaves a document the agent cannot parse.
///
/// Bounds torn reads, not lost updates. What keeps that window from mattering is
/// that registration writes only when an entry actually differs.
fn save_json(path: &Path, value: &serde_json::Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let contents = serde_json::to_string_pretty(value)?;

    // Pid-suffixed so two concurrent syncs cannot share a temp path.
    let temp = path.with_extension(format!("symposium-tmp-{}", std::process::id()));
    fs::write(&temp, contents)?;
    if let Err(e) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(e.into());
    }
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

    /// Each path was confirmed by asking the tool itself, not read off docs: a
    /// wrong one fails silently, since symposium reports success for writing a
    /// file the agent never reads.
    #[test]
    fn mcp_config_paths_match_what_each_agent_reads() {
        let project = Path::new("/project");
        let home = Path::new("/home/user");
        let cases = [
            (Agent::Claude, "/project/.mcp.json", "/home/user/.claude.json"),
            (
                Agent::Gemini,
                "/project/.gemini/settings.json",
                "/home/user/.gemini/settings.json",
            ),
            (
                Agent::OpenCode,
                "/project/opencode.json",
                "/home/user/.config/opencode/opencode.json",
            ),
            (
                Agent::Kiro,
                "/project/.kiro/settings/mcp.json",
                "/home/user/.kiro/settings/mcp.json",
            ),
        ];
        for (agent, project_path, user_path) in cases {
            assert_eq!(
                agent.mcp_config_path(McpScope::Project, project, home),
                PathBuf::from(project_path),
                "{agent:?} project scope"
            );
            // Env-relocatable on purpose; mutating env here would race other tests.
            if !relocated_by_env(agent) {
                assert_eq!(
                    agent.mcp_config_path(McpScope::User, project, home),
                    PathBuf::from(user_path),
                    "{agent:?} user scope"
                );
            }
            assert!(agent.supports_project_mcp_scope(), "{agent:?}");
        }
    }

    /// Is this agent's user-scope path redirected by an env var right now?
    fn relocated_by_env(agent: Agent) -> bool {
        let set = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
        match agent {
            Agent::Claude => set("CLAUDE_CONFIG_DIR"),
            Agent::OpenCode | Agent::Goose => set("XDG_CONFIG_HOME"),
            _ => false,
        }
    }

    #[test]
    fn claude_user_path_follows_claude_config_dir() {
        let path = Agent::Claude.mcp_config_path(
            McpScope::User,
            Path::new("/project"),
            Path::new("/home/user"),
        );
        match std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()) {
            Some(dir) => assert_eq!(path, PathBuf::from(dir).join(".claude.json")),
            None => assert_eq!(path, PathBuf::from("/home/user/.claude.json")),
        }
    }

    /// These CLIs read only their user-level file, so project scope resolves
    /// there rather than to a project file they would ignore.
    #[test]
    fn agents_without_project_mcp_scope_fall_back_to_the_user_file() {
        let project = Path::new("/project");
        let home = Path::new("/home/user");
        for (agent, expected) in [
            (Agent::Codex, "/home/user/.codex/config.toml"),
            (Agent::Copilot, "/home/user/.copilot/mcp-config.json"),
            (Agent::Goose, "/home/user/.config/goose/config.yaml"),
        ] {
            assert!(!agent.supports_project_mcp_scope(), "{agent:?}");
            for scope in [McpScope::Project, McpScope::User] {
                let path = agent.mcp_config_path(scope, project, home);
                if !relocated_by_env(agent) {
                    assert_eq!(path, PathBuf::from(expected), "{agent:?} {scope:?}");
                } else {
                    // Still the point of the test: both scopes agree.
                    assert_eq!(
                        path,
                        agent.mcp_config_path(McpScope::User, project, home),
                        "{agent:?} {scope:?}"
                    );
                }
            }
        }
    }

    /// Compared per agent against *its own* hook file: a global "never
    /// `.claude/settings.json`" check would pass while pointing Codex at
    /// `.codex/hooks.json`.
    ///
    /// Gemini is the legitimate exception - `.gemini/settings.json` carries both,
    /// confirmed by `gemini mcp list` reading entries written beside the hooks.
    #[test]
    fn mcp_config_is_never_the_agents_own_hooks_file() {
        let project = Path::new("/project");
        let home = Path::new("/home/user");
        for &agent in Agent::all() {
            if agent == Agent::Gemini {
                continue;
            }
            for (scope, root) in [
                (McpScope::Project, project),
                (McpScope::User, home),
            ] {
                let mcp = agent.mcp_config_path(scope, project, home);
                for hooks in hook_paths_for(agent, root) {
                    assert_ne!(mcp, hooks, "{agent:?} {scope:?} writes MCP into its hooks file");
                }
            }
        }
    }

    /// Mirrors the targets [`Agent::register_hooks`] writes.
    fn hook_paths_for(agent: Agent, root: &Path) -> Vec<PathBuf> {
        match agent {
            Agent::Claude => vec![root.join(".claude").join("settings.json")],
            Agent::Codex => vec![root.join(".codex").join("hooks.json")],
            Agent::Copilot => vec![root.join(".github").join("hooks")],
            Agent::Gemini => vec![root.join(".gemini").join("settings.json")],
            Agent::Kiro => vec![root.join(".kiro").join("agents")],
            // Skills-only agents register no hooks at all.
            Agent::Goose | Agent::OpenCode => vec![],
        }
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
        assert_eq!(pre_tool["command"], "cargo-agents hook kiro pre-tool-use");
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
        assert!(settings["hooks"]["BeforeAgent"].is_array());
        assert!(settings["hooks"]["SessionStart"].is_array());
    }
}
