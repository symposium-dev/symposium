//! Agent abstraction: hook registration and extension installation paths.
//!
//! Each supported agent has different conventions for where hooks are
//! configured and where skill files are placed. This module centralizes
//! that knowledge.

mod mcp_server_registration;
mod plugin_registration;

pub(crate) use plugin_registration::sync_marketplace_index;

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::json;

use crate::config::Symposium;
use crate::output::{Output, display_path};

#[derive(Debug, PartialEq)]
pub(crate) struct CompiledPlugin {
    pub name: String,
    pub dir: PathBuf,
}

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
    Antigravity,
    Claude,
    Codex,
    Copilot,
    Goose,
    Kiro,
    OpenCode,
}

impl Agent {
    /// Parse an agent name from a config string.
    pub fn from_config_name(name: &str) -> Result<Self> {
        match name {
            "antigravity" => Ok(Agent::Antigravity),
            "claude" => Ok(Agent::Claude),
            "codex" => Ok(Agent::Codex),
            "copilot" => Ok(Agent::Copilot),
            "goose" => Ok(Agent::Goose),
            "kiro" => Ok(Agent::Kiro),
            "opencode" => Ok(Agent::OpenCode),
            other => bail!(
                "unknown agent: {other} (expected antigravity, claude, codex, copilot, goose, kiro, or opencode)"
            ),
        }
    }

    /// Config name as stored in TOML.
    pub fn config_name(&self) -> &'static str {
        match self {
            Agent::Antigravity => "antigravity",
            Agent::Claude => "claude",
            Agent::Codex => "codex",
            Agent::Copilot => "copilot",
            Agent::Goose => "goose",
            Agent::Kiro => "kiro",
            Agent::OpenCode => "opencode",
        }
    }

    /// Human-readable display name.
    pub fn display_name(&self) -> &'static str {
        match self {
            Agent::Antigravity => "Antigravity CLI",
            Agent::Claude => "Claude Code",
            Agent::Codex => "Codex CLI",
            Agent::Copilot => "GitHub Copilot",
            Agent::Goose => "Goose",
            Agent::Kiro => "Kiro",
            Agent::OpenCode => "OpenCode",
        }
    }

    /// All supported agents for interactive prompts.
    pub fn all() -> &'static [Agent] {
        &[
            Agent::Antigravity,
            Agent::Claude,
            Agent::Codex,
            Agent::Copilot,
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
    /// Claude Code requires `.claude/skills/`, while Antigravity, Copilot and
    /// Antigravity, Codex and Copilot support the vendor-neutral
    /// `.agents/skills/` path.
    pub fn project_skill_dir(&self, project_root: &Path, skill_name: &str) -> PathBuf {
        assert!(
            crate::skills::is_safe_dir_name(skill_name),
            "unsafe skill directory name {skill_name:?}"
        );
        match self {
            Agent::Claude => project_root.join(".claude").join("skills").join(skill_name),
            Agent::Antigravity | Agent::Codex | Agent::Copilot => {
                project_root.join(".agents").join("skills").join(skill_name)
            }
            Agent::Goose => project_root.join(".agents").join("skills").join(skill_name),
            Agent::Kiro => project_root.join(".kiro").join("skills").join(skill_name),
            Agent::OpenCode => project_root
                .join(".opencode")
                .join("skills")
                .join(skill_name),
        }
    }

    /// Global skill directory for a given skill name, if supported.
    pub fn global_skill_dir(&self, home: &Path, skill_name: &str) -> Option<PathBuf> {
        assert!(
            crate::skills::is_safe_dir_name(skill_name),
            "unsafe skill directory name {skill_name:?}"
        );
        match self {
            // Antigravity's shared config root, the one location all three of its
            // surfaces (CLI, IDE, web) read.
            Agent::Antigravity => Some(
                home.join(".gemini")
                    .join("config")
                    .join("skills")
                    .join(skill_name),
            ),
            Agent::Claude => Some(home.join(".claude").join("skills").join(skill_name)),
            Agent::Codex => Some(home.join(".agents").join("skills").join(skill_name)),
            Agent::Copilot => None, // no global skills path
            Agent::Goose => Some(home.join(".agents").join("skills").join(skill_name)),
            Agent::Kiro => Some(home.join(".kiro").join("skills").join(skill_name)),
            Agent::OpenCode => Some(home.join(".agents").join("skills").join(skill_name)),
        }
    }

    pub(crate) fn reads_vendor_neutral_skills(&self) -> bool {
        match self {
            Agent::Antigravity | Agent::Codex | Agent::Copilot | Agent::Goose | Agent::OpenCode => {
                true
            }
            Agent::Claude | Agent::Kiro => false,
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
            Agent::Antigravity => {
                register_antigravity_hooks(&project_root.join(".agents").join("hooks.json"), out)
            }
            Agent::Claude => {
                register_claude_hooks(&project_root.join(".claude").join("settings.json"), out)
            }
            Agent::Codex => {
                register_codex_hooks(&project_root.join(".codex").join("hooks.json"), out)
            }
            Agent::Copilot => {
                register_copilot_hooks(&project_root.join(".github").join("hooks"), out)
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
    pub fn register_hooks(&self, home: &Path, sym: &Symposium, out: &Output) -> Result<()> {
        tracing::debug!(agent = %self.config_name(), "registering hooks");
        // Register hooks
        match self {
            Agent::Antigravity => register_antigravity_hooks(
                &home.join(".gemini").join("config").join("hooks.json"),
                out,
            ),
            Agent::Claude => {
                register_claude_hooks(&claude_config_dir(sym).join("settings.json"), out)
            }
            Agent::Codex => register_codex_hooks(&codex_dir(sym).join("hooks.json"), out),
            Agent::Copilot => {
                register_copilot_hooks_global(&copilot_config_dir(sym).join("settings.json"), out)
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
    pub fn mcp_config_path(
        &self,
        scope: McpScope,
        project_root: &Path,
        sym: &Symposium,
    ) -> PathBuf {
        let home = sym.home_dir();
        let xdg_config = || {
            sym.env_dir("XDG_CONFIG_HOME")
                .unwrap_or_else(|| home.join(".config"))
        };

        match (self, scope) {
            // Project MCP is `.mcp.json`; the user-level file is `.claude.json`.
            // Neither is `settings.json`, which holds hooks.
            (Agent::Claude, McpScope::Project) => project_root.join(".mcp.json"),
            (Agent::Claude, McpScope::User) => sym
                .env_dir("CLAUDE_CONFIG_DIR")
                .unwrap_or_else(|| home.to_path_buf())
                .join(".claude.json"),

            // Antigravity keeps MCP out of its hooks file, unlike Gemini CLI:
            // `mcp_config.json`, not `hooks.json`.
            (Agent::Antigravity, McpScope::Project) => {
                project_root.join(".agents").join("mcp_config.json")
            }
            (Agent::Antigravity, McpScope::User) => {
                home.join(".gemini").join("config").join("mcp_config.json")
            }

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
            (Agent::Codex, _) => codex_dir(sym).join("config.toml"),
            (Agent::Copilot, _) => copilot_config_dir(sym).join("mcp-config.json"),
            (Agent::Goose, _) => goose_path_root(sym)
                .map(|root| root.join("config"))
                .unwrap_or_else(|| xdg_config().join("goose"))
                .join("config.yaml"),
        }
    }

    /// Whether this agent honors project-scoped MCP registration at all.
    ///
    /// `false` means [`Self::mcp_config_path`] ignores the requested scope and
    /// answers with the user-level file, which callers may want to report.
    pub fn supports_project_mcp_scope(&self) -> bool {
        matches!(
            self,
            Agent::Antigravity | Agent::Claude | Agent::OpenCode | Agent::Kiro
        )
    }

    /// Register MCP servers in the agent's config for `scope`, returning the
    /// names of the servers now present there (skipped ones left out).
    pub fn register_mcp_servers(
        &self,
        scope: McpScope,
        project_root: &Path,
        sym: &Symposium,
        servers: &[sacp::schema::McpServer],
        out: &Output,
    ) -> Result<Vec<String>> {
        tracing::debug!(agent = %self.config_name(), count = servers.len(), ?scope, "registering MCP servers");
        let path = self.mcp_config_path(scope, project_root, sym);
        match self {
            Agent::Claude => {
                mcp_server_registration::register_claude_mcp_servers(&path, servers, out)
            }
            Agent::Codex => {
                mcp_server_registration::register_codex_mcp_servers(&path, servers, out)
            }
            Agent::Copilot => {
                mcp_server_registration::register_copilot_mcp_servers(&path, servers, out)
            }
            Agent::Antigravity => {
                mcp_server_registration::register_antigravity_mcp_servers(&path, servers, out)
            }
            Agent::Kiro => mcp_server_registration::register_kiro_mcp_servers(&path, servers, out),
            Agent::Goose => {
                mcp_server_registration::register_goose_mcp_servers(&path, servers, out)
            }
            Agent::OpenCode => {
                mcp_server_registration::register_opencode_mcp_servers(&path, servers, out)
            }
        }
    }

    /// Remove MCP servers from `path`, an MCP config file of this agent.
    pub fn unregister_mcp_servers(&self, path: &Path, names: &[&str], out: &Output) -> Result<()> {
        match self {
            Agent::Claude => {
                mcp_server_registration::unregister_claude_mcp_servers(path, names, out)
            }
            Agent::Codex => mcp_server_registration::unregister_codex_mcp_servers(path, names, out),
            Agent::Copilot => {
                mcp_server_registration::unregister_copilot_mcp_servers(path, names, out)
            }
            Agent::Antigravity => {
                mcp_server_registration::unregister_antigravity_mcp_servers(path, names, out)
            }
            Agent::Kiro => mcp_server_registration::unregister_kiro_mcp_servers(path, names, out),
            Agent::Goose => mcp_server_registration::unregister_goose_mcp_servers(path, names, out),
            Agent::OpenCode => {
                mcp_server_registration::unregister_opencode_mcp_servers(path, names, out)
            }
        }
    }

    pub(crate) fn deliver_user_plugins(
        &self,
        sym: &Symposium,
        plugins: &[CompiledPlugin],
    ) -> Option<Vec<String>> {
        let plugins_dir = match self {
            Agent::Antigravity => sym
                .home_dir()
                .join(".gemini")
                .join("config")
                .join("plugins"),
            Agent::Claude => claude_config_dir(sym).join("skills"),
            Agent::Codex => return plugin_registration::codex::deliver_user_plugins(sym, plugins),
            Agent::Copilot => {
                return plugin_registration::copilot::deliver_user_plugins(sym, plugins);
            }
            Agent::Goose => goose_path_root(sym)
                .unwrap_or_else(|| sym.home_dir().to_path_buf())
                .join(".agents")
                .join("plugins"),
            Agent::Kiro | Agent::OpenCode => return None,
        };
        Some(plugin_registration::deliver_copies(
            sym,
            *self,
            &plugins_dir,
            plugins,
        ))
    }

    pub(crate) fn deliver_project_plugins(
        &self,
        sym: &Symposium,
        project_root: &Path,
        plugins: &[CompiledPlugin],
    ) -> Option<Vec<String>> {
        let Agent::Claude = self else {
            return None;
        };
        let skills_dir = project_root.join(".claude").join("skills");
        let updated = plugin_registration::deliver_copies(sym, *self, &skills_dir, plugins);
        for plugin in plugins {
            let dir = skills_dir.join(&plugin.name);
            if !crate::sync::has_symposium_marker(&dir) {
                continue;
            }
            if let Err(e) = crate::sync::ignore_in_git(&dir) {
                tracing::info!(
                    report = %crate::report::ReportEvent::Warning { message: format!("{e:#}") },
                );
            }
        }
        Some(updated)
    }

    /// Remove hooks from the project-level agent config.
    pub fn unregister_project_hooks(&self, project_root: &Path, _sym: &Symposium, out: &Output) {
        match self {
            Agent::Antigravity => {
                unregister_antigravity_hooks(&project_root.join(".agents").join("hooks.json"), out)
            }
            Agent::Claude => {
                unregister_claude_hooks(&project_root.join(".claude").join("settings.json"), out)
            }
            Agent::Codex => {
                unregister_codex_hooks(&project_root.join(".codex").join("hooks.json"), out)
            }
            Agent::Copilot => {
                unregister_copilot_hooks(&project_root.join(".github").join("hooks"), out)
            }
            Agent::Kiro => unregister_kiro_hooks(&project_root.join(".kiro").join("agents"), out),
            Agent::Goose => {}    // no hooks to unregister
            Agent::OpenCode => {} // no hooks to unregister
        }
    }

    /// Remove hooks from the global agent config.
    pub fn unregister_hooks(&self, home: &Path, sym: &Symposium, out: &Output) {
        match self {
            Agent::Antigravity => unregister_antigravity_hooks(
                &home.join(".gemini").join("config").join("hooks.json"),
                out,
            ),
            Agent::Claude => {
                unregister_claude_hooks(&claude_config_dir(sym).join("settings.json"), out)
            }
            Agent::Codex => unregister_codex_hooks(&codex_dir(sym).join("hooks.json"), out),
            Agent::Copilot => {
                unregister_copilot_hooks_global(&copilot_config_dir(sym).join("settings.json"), out)
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

fn claude_config_dir(sym: &Symposium) -> PathBuf {
    sym.env_dir("CLAUDE_CONFIG_DIR")
        .unwrap_or_else(|| sym.home_dir().join(".claude"))
}

fn register_claude_hooks(settings_path: &Path, out: &Output) -> Result<()> {
    let mut settings = load_json_or_empty(settings_path)?;
    let display = display_path(settings_path);
    let hooks_obj = hooks_object(&mut settings, &display)?;

    let mut added = Vec::new();

    for event in [
        "PreToolUse",
        "PostToolUse",
        "UserPromptSubmit",
        "SessionStart",
        "Stop",
    ] {
        let command = format!("cargo-agents hook claude {}", event_to_cli_arg(event));
        if ensure_claude_hook_entry(event_hooks(hooks_obj, event, &display)?, &command) {
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
fn ensure_claude_hook_entry(arr: &mut Vec<serde_json::Value>, command: &str) -> bool {
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

fn codex_dir(sym: &Symposium) -> PathBuf {
    sym.env_dir("CODEX_HOME")
        .unwrap_or_else(|| sym.home_dir().join(".codex"))
}

fn register_codex_hooks(hooks_path: &Path, out: &Output) -> Result<()> {
    let mut settings = load_json_or_empty(hooks_path)?;
    let display = display_path(hooks_path);
    let hooks_obj = hooks_object(&mut settings, &display)?;

    let mut added = Vec::new();

    for event in [
        "PreToolUse",
        "PostToolUse",
        "UserPromptSubmit",
        "SessionStart",
    ] {
        let command = format!("cargo-agents hook codex {}", event_to_cli_arg(event));
        if ensure_codex_hook_entry(event_hooks(hooks_obj, event, &display)?, &command) {
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
fn ensure_codex_hook_entry(arr: &mut Vec<serde_json::Value>, command: &str) -> bool {
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

fn copilot_config_dir(sym: &Symposium) -> PathBuf {
    sym.env_dir("COPILOT_HOME")
        .unwrap_or_else(|| sym.home_dir().join(".copilot"))
}

/// Register hooks in the global Copilot user settings file (`~/.copilot/settings.json`).
fn register_copilot_hooks_global(config_path: &Path, out: &Output) -> Result<()> {
    let display = display_path(config_path);
    let mut config = load_json_or_empty(config_path)?;
    let hooks_obj = hooks_object(&mut config, &display)?;

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
        event_hooks(hooks_obj, event, &display)?.push(entry);
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
// Antigravity CLI hook registration
// ---------------------------------------------------------------------------

/// The key symposium owns in Antigravity's `hooks.json`. Registrations there are
/// keyed by a hook *name*, so several tools can share one file; symposium only
/// ever writes and reaps this one entry.
const ANTIGRAVITY_HOOK_NAME: &str = "symposium";

/// Antigravity's events, paired with the symposium event each maps to.
///
/// `PreInvocation` stands in for `user-prompt-submit`: Antigravity has no
/// prompt event, and `PreInvocation` fires before every model call, so only the
/// first invocation of a turn dispatches. `SessionStart` is undocumented but
/// real; it fires with the first turn of a new conversation and never on resume.
const ANTIGRAVITY_EVENTS: &[(&str, &str)] = &[
    ("PreToolUse", "pre-tool-use"),
    ("PostToolUse", "post-tool-use"),
    ("PreInvocation", "user-prompt-submit"),
    ("SessionStart", "session-start"),
    ("Stop", "stop"),
];

/// Tool events wrap their handlers in a `matcher` group; lifecycle events take a
/// flat list of handlers. Getting this wrong is silent — Antigravity accepts an
/// unrecognised shape and simply never fires it.
fn antigravity_is_tool_event(event: &str) -> bool {
    matches!(event, "PreToolUse" | "PostToolUse")
}

fn antigravity_handler(cli_arg: &str) -> serde_json::Value {
    json!({
        "type": "command",
        "command": format!("cargo-agents hook antigravity {cli_arg}"),
        "timeout": 30,
    })
}

fn register_antigravity_hooks(hooks_path: &Path, out: &Output) -> Result<()> {
    let mut config = load_json_or_empty(hooks_path)?;
    let display = display_path(hooks_path);

    let mut entry = serde_json::Map::new();
    for (event, cli_arg) in ANTIGRAVITY_EVENTS {
        let handler = antigravity_handler(cli_arg);
        let value = if antigravity_is_tool_event(event) {
            json!([{ "matcher": "*", "hooks": [handler] }])
        } else {
            json!([handler])
        };
        entry.insert((*event).to_string(), value);
    }
    let entry = serde_json::Value::Object(entry);

    let obj = config
        .as_object_mut()
        .with_context(|| format!("{display} is not a JSON object"))?;
    if obj.get(ANTIGRAVITY_HOOK_NAME) == Some(&entry) {
        out.already_ok(format!("{display}: hooks already registered"));
        return Ok(());
    }

    obj.insert(ANTIGRAVITY_HOOK_NAME.to_string(), entry);
    save_json(hooks_path, &config)?;
    let events: Vec<&str> = ANTIGRAVITY_EVENTS.iter().map(|(e, _)| *e).collect();
    out.done(format!("{display}: added hooks ({})", events.join(", ")));
    Ok(())
}

/// Remove only symposium's own named entry, leaving hooks other tools or the
/// user registered in the same file untouched.
fn unregister_antigravity_hooks(hooks_path: &Path, out: &Output) {
    let display = display_path(hooks_path);

    let Ok(mut config) = load_json_or_empty(hooks_path) else {
        return;
    };
    let Some(obj) = config.as_object_mut() else {
        return;
    };
    if obj.shift_remove(ANTIGRAVITY_HOOK_NAME).is_some()
        && let Ok(()) = save_json(hooks_path, &config)
    {
        out.removed(format!("{display}: removed hooks"));
    }
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
    display: &str,
) -> Result<(bool, Vec<&'static str>)> {
    let obj = config
        .as_object_mut()
        .with_context(|| format!("{display} is not a JSON object"))?;
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

    let hooks_obj = hooks
        .as_object_mut()
        .with_context(|| format!("`hooks` in {display} is not a JSON object"))?;

    let mut added = Vec::new();
    for (event, entry) in kiro_hook_entries() {
        if ensure_kiro_hook_entry(event_hooks(hooks_obj, event, display)?, &entry) {
            added.push(event);
            changed = true;
        }
    }
    Ok((changed, added))
}

/// Register hooks by creating a Kiro agent file (`.kiro/agents/symposium.json`).
fn register_kiro_hooks(agents_dir: &Path, out: &Output) -> Result<()> {
    fs::create_dir_all(agents_dir)?;
    let hook_file = agents_dir.join("symposium.json");
    let display = display_path(&hook_file);

    let mut config = load_json_or_empty(&hook_file)?;
    let (changed, added) = merge_kiro_hooks(&mut config, "symposium", &display)?;

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
fn ensure_kiro_hook_entry(arr: &mut Vec<serde_json::Value>, entry: &serde_json::Value) -> bool {
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

/// Remove symposium hooks from a Claude-style settings.json file.
fn unregister_settings_hooks(settings_path: &Path, command_prefix: &str, out: &Output) {
    remove_hook_entries(settings_path, out, |group| {
        let Some(handlers) = group.get_mut("hooks").and_then(|h| h.as_array_mut()) else {
            return false;
        };
        let before = handlers.len();
        handlers.retain(|h| {
            !h.get("command")
                .and_then(|c| c.as_str())
                .is_some_and(|c| c.starts_with(command_prefix))
        });
        handlers.len() < before && handlers.is_empty()
    });
}

fn remove_hook_entries(
    config_path: &Path,
    out: &Output,
    drop_entry: impl Fn(&mut serde_json::Value) -> bool,
) {
    let display = display_path(config_path);

    let Ok(mut config) = load_json_or_empty(config_path) else {
        return;
    };
    let Some(root) = config.as_object_mut() else {
        return;
    };
    let Some(hooks) = root.get_mut("hooks").and_then(|h| h.as_object_mut()) else {
        return;
    };

    let mut emptied = Vec::new();
    let mut changed = false;
    for (event, arr) in hooks.iter_mut() {
        if let Some(entries) = arr.as_array_mut() {
            let before = entries.clone();
            entries.retain_mut(|entry| !drop_entry(entry));
            if *entries != before {
                changed = true;
                if entries.is_empty() {
                    emptied.push(event.clone());
                }
            }
        }
    }
    for event in &emptied {
        hooks.shift_remove(event);
    }
    if changed && hooks.is_empty() {
        root.shift_remove("hooks");
    }

    if changed && let Ok(()) = save_json(config_path, &config) {
        out.removed(format!("{display}: removed hooks"));
    }
}

fn unregister_claude_hooks(settings_path: &Path, out: &Output) {
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
/// `{ "hooks": [{ "command": "..." }] }` structure used by Claude/Codex.
fn unregister_flat_hooks(config_path: &Path, command_key: &str, out: &Output) {
    remove_hook_entries(config_path, out, |entry| {
        entry
            .get(command_key)
            .and_then(|c| c.as_str())
            .is_some_and(|c| c.starts_with("cargo-agents hook"))
    });
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn goose_path_root(sym: &Symposium) -> Option<PathBuf> {
    sym.env_dir("GOOSE_PATH_ROOT")
}

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
        serde_json::from_str(&contents).with_context(|| format!("parse {}", display_path(path)))
    } else {
        Ok(json!({}))
    }
}

fn hooks_object<'a>(
    config: &'a mut serde_json::Value,
    display: &str,
) -> Result<&'a mut serde_json::Map<String, serde_json::Value>> {
    config
        .as_object_mut()
        .with_context(|| format!("{display} is not a JSON object"))?
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .with_context(|| format!("`hooks` in {display} is not a JSON object"))
}

fn event_hooks<'a>(
    hooks: &'a mut serde_json::Map<String, serde_json::Value>,
    event: &str,
    display: &str,
) -> Result<&'a mut Vec<serde_json::Value>> {
    hooks
        .entry(event)
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .with_context(|| format!("`hooks.{event}` in {display} is not a JSON array"))
}

/// Write JSON config, replacing the file atomically.
///
/// Temp file plus rename, because some of these are live agent state (Claude
/// Code rewrites `~/.claude.json` throughout a session) and a truncating write
/// that loses a race leaves a document the agent cannot parse.
///
/// The rename replaces whatever `path` names, so a symlink is resolved first
/// and its target replaced instead, and the temp file takes the target's mode
/// before any content is written: these files can hold tokens, and dotfile
/// setups link them. The new inode still drops any hard links.
///
/// Bounds torn reads, not lost updates. What keeps that window from mattering is
/// that registration writes only when an entry actually differs.
fn save_json(path: &Path, value: &serde_json::Value) -> Result<()> {
    let target = if fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        fs::canonicalize(path)
            .with_context(|| format!("cannot resolve symlink {}", display_path(path)))?
    } else {
        path.to_path_buf()
    };
    let parent = target
        .parent()
        .with_context(|| format!("{} has no parent directory", display_path(&target)))?;
    fs::create_dir_all(parent)?;

    let existing = fs::read_to_string(&target).unwrap_or_default();
    let formatter =
        serde_json::ser::PrettyFormatter::with_indent(json_indent(&existing).as_bytes());
    let mut contents = Vec::new();
    serde::Serialize::serialize(
        value,
        &mut serde_json::Serializer::with_formatter(&mut contents, formatter),
    )?;
    contents.push(b'\n');

    #[cfg(unix)]
    let permissions = match fs::metadata(&target) {
        Ok(meta) => Some(meta.permissions()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let mut builder = tempfile::Builder::new();
    builder.prefix(".symposium-tmp-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        // Keep private configs private from the moment the temp file is created.
        builder.permissions(
            permissions
                .clone()
                .unwrap_or_else(|| fs::Permissions::from_mode(0o666)),
        );
    }
    let mut temp = builder.tempfile_in(parent)?;
    #[cfg(unix)]
    if let Some(permissions) = permissions {
        // Restore bits that the umask may have removed during creation.
        temp.as_file().set_permissions(permissions)?;
    }
    temp.write_all(&contents)?;
    temp.persist(&target).map_err(|e| e.error)?;
    Ok(())
}

fn json_indent(text: &str) -> &str {
    text.lines()
        .find_map(|line| {
            let rest = line.trim_start_matches([' ', '\t']);
            (rest.starts_with('"') && rest.len() < line.len())
                .then(|| &line[..line.len() - rest.len()])
        })
        .unwrap_or("  ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_from_config_name() {
        assert_eq!(Agent::from_config_name("claude").unwrap(), Agent::Claude);
        assert_eq!(Agent::from_config_name("codex").unwrap(), Agent::Codex);
        assert_eq!(Agent::from_config_name("copilot").unwrap(), Agent::Copilot);
        assert!(Agent::from_config_name("unknown").is_err());
    }

    /// Each path was confirmed by asking the tool itself, not read off docs: a
    /// wrong one fails silently, since symposium reports success for writing a
    /// file the agent never reads.
    #[test]
    fn mcp_config_paths_match_what_each_agent_reads() {
        let home = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(home.path());
        let project = Path::new("/project");
        let cases = [
            (Agent::Claude, "/project/.mcp.json", ".claude.json"),
            (
                Agent::Antigravity,
                "/project/.agents/mcp_config.json",
                ".gemini/config/mcp_config.json",
            ),
            (
                Agent::OpenCode,
                "/project/opencode.json",
                ".config/opencode/opencode.json",
            ),
            (
                Agent::Kiro,
                "/project/.kiro/settings/mcp.json",
                ".kiro/settings/mcp.json",
            ),
        ];
        for (agent, project_path, user_path) in cases {
            assert_eq!(
                agent.mcp_config_path(McpScope::Project, project, &sym),
                PathBuf::from(project_path),
                "{agent:?} project scope"
            );
            assert_eq!(
                agent.mcp_config_path(McpScope::User, project, &sym),
                home.path().join(user_path),
                "{agent:?} user scope"
            );
            assert!(agent.supports_project_mcp_scope(), "{agent:?}");
        }
    }

    #[test]
    fn user_files_follow_each_agents_relocation_variable() {
        use sacp::schema::{McpServer, McpServerStdio};

        let servers = [McpServer::Stdio(McpServerStdio::new(
            "symposium",
            "/usr/local/bin/cargo-agents",
        ))];
        let out = Output::quiet();
        let project = Path::new("/project");
        let cases: &[(Agent, &[(&str, &str)], Option<&str>, &str)] = &[
            (
                Agent::Claude,
                &[("CLAUDE_CONFIG_DIR", "moved")],
                Some("moved/settings.json"),
                "moved/.claude.json",
            ),
            (
                Agent::Codex,
                &[("CODEX_HOME", "moved")],
                Some("moved/hooks.json"),
                "moved/config.toml",
            ),
            (
                Agent::Copilot,
                &[("COPILOT_HOME", "moved")],
                Some("moved/settings.json"),
                "moved/mcp-config.json",
            ),
            (
                Agent::Goose,
                &[("GOOSE_PATH_ROOT", "moved"), ("XDG_CONFIG_HOME", "xdg")],
                None,
                "moved/config/config.yaml",
            ),
            (
                Agent::Goose,
                &[("GOOSE_PATH_ROOT", ""), ("XDG_CONFIG_HOME", "xdg")],
                None,
                "xdg/goose/config.yaml",
            ),
        ];
        for &(agent, env, hooks, mcp) in cases {
            let tmp = tempfile::tempdir().unwrap();
            let vars: Vec<(String, std::ffi::OsString)> = env
                .iter()
                .map(|&(name, dir)| match dir {
                    "" => (name.to_string(), "".into()),
                    dir => (name.to_string(), tmp.path().join(dir).into_os_string()),
                })
                .collect();
            let mut sym = Symposium::from_dir(&tmp.path().join("home"));
            sym.set_env(move |name| {
                vars.iter()
                    .find(|(var, _)| var == name)
                    .map(|(_, value)| value.clone())
            });
            let read = |file: &str| fs::read_to_string(tmp.path().join(file)).unwrap();

            agent.register_hooks(sym.home_dir(), &sym, &out).unwrap();
            agent
                .register_mcp_servers(McpScope::User, project, &sym, &servers, &out)
                .unwrap();
            if let Some(hooks) = hooks {
                assert!(
                    read(hooks).contains("cargo-agents hook"),
                    "{agent:?} {env:?}"
                );
            }
            assert!(read(mcp).contains("symposium"), "{agent:?} {env:?}");

            agent.unregister_hooks(sym.home_dir(), &sym, &out);
            agent
                .unregister_mcp_servers(
                    &agent.mcp_config_path(McpScope::User, project, &sym),
                    &["symposium"],
                    &out,
                )
                .unwrap();
            if let Some(hooks) = hooks {
                assert!(
                    !read(hooks).contains("cargo-agents hook"),
                    "{agent:?} {env:?}"
                );
            }
            assert!(!read(mcp).contains("symposium"), "{agent:?} {env:?}");
        }
    }

    #[test]
    fn config_files_symposium_cannot_parse_are_an_error_and_stay_untouched() {
        use sacp::schema::{McpServer, McpServerStdio};

        let servers = [McpServer::Stdio(McpServerStdio::new(
            "symposium",
            "/usr/local/bin/cargo-agents",
        ))];
        let out = Output::quiet();
        let json_hooks: &[&str] = &[
            "{",
            "[]",
            r#"{"hooks": []}"#,
            r#"{"hooks": {"SessionStart": {}, "sessionStart": {}, "agentSpawn": {}}}"#,
        ];
        let cases: [(&str, &str, &dyn Fn(&Path) -> Result<()>, &[&str]); 6] = [
            (
                "claude",
                "settings.json",
                &|path: &Path| register_claude_hooks(path, &out),
                json_hooks,
            ),
            (
                "codex",
                "hooks.json",
                &|path: &Path| register_codex_hooks(path, &out),
                json_hooks,
            ),
            (
                "copilot",
                "settings.json",
                &|path: &Path| register_copilot_hooks_global(path, &out),
                json_hooks,
            ),
            (
                "antigravity",
                "hooks.json",
                &|path: &Path| register_antigravity_hooks(path, &out),
                &["{", "[]"],
            ),
            (
                "kiro",
                "symposium.json",
                &|path: &Path| register_kiro_hooks(path.parent().unwrap(), &out),
                json_hooks,
            ),
            (
                "codex mcp",
                "config.toml",
                &|path: &Path| {
                    mcp_server_registration::register_codex_mcp_servers(path, &servers, &out)
                        .map(drop)
                },
                &["[[[", "mcp_servers = 3"],
            ),
        ];
        for (agent, file, register, contents) in cases {
            for &contents in contents {
                let tmp = tempfile::tempdir().unwrap();
                let path = tmp.path().join(file);
                fs::write(&path, contents).unwrap();
                let error = format!("{:#}", register(&path).unwrap_err());
                assert!(
                    error.contains(&display_path(&path)),
                    "{agent} {contents:?}: {error}"
                );
                assert_eq!(fs::read_to_string(&path).unwrap(), contents, "{agent}");
            }
        }
    }

    /// These CLIs read only their user-level file, so project scope resolves
    /// there rather than to a project file they would ignore.
    #[test]
    fn agents_without_project_mcp_scope_fall_back_to_the_user_file() {
        let home = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(home.path());
        let project = Path::new("/project");
        for (agent, expected) in [
            (Agent::Codex, ".codex/config.toml"),
            (Agent::Copilot, ".copilot/mcp-config.json"),
            (Agent::Goose, ".config/goose/config.yaml"),
        ] {
            assert!(!agent.supports_project_mcp_scope(), "{agent:?}");
            for scope in [McpScope::Project, McpScope::User] {
                assert_eq!(
                    agent.mcp_config_path(scope, project, &sym),
                    home.path().join(expected),
                    "{agent:?} {scope:?}"
                );
            }
        }
    }

    /// Compared per agent against *its own* hook file: a global "never
    /// `.claude/settings.json`" check would pass while pointing Codex at
    /// `.codex/hooks.json`.
    ///
    #[test]
    fn mcp_config_is_never_the_agents_own_hooks_file() {
        let tmp = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(tmp.path());
        let project = Path::new("/project");
        let home = tmp.path();
        for &agent in Agent::all() {
            for (scope, root) in [(McpScope::Project, project), (McpScope::User, home)] {
                let mcp = agent.mcp_config_path(scope, project, &sym);
                for hooks in hook_paths_for(agent, root) {
                    assert_ne!(
                        mcp, hooks,
                        "{agent:?} {scope:?} writes MCP into its hooks file"
                    );
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
            Agent::Antigravity => vec![
                root.join(".agents").join("hooks.json"),
                root.join(".gemini").join("config").join("hooks.json"),
            ],
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
    fn unregistering_hooks_leaves_the_file_as_the_user_had_it() {
        type Register = fn(&Path, &Output) -> Result<()>;
        type Unregister = fn(&Path, &Output);
        let agents: [(Register, Unregister); 3] = [
            (register_claude_hooks, unregister_claude_hooks),
            (register_codex_hooks, unregister_codex_hooks),
            (
                register_copilot_hooks_global,
                unregister_copilot_hooks_global,
            ),
        ];
        let originals = [
            json!({ "model": "opus" }),
            json!({
                "model": "opus",
                "hooks": { "Notification": [], "Custom": [{ "keep": true }] }
            }),
        ];
        for (register, unregister) in agents {
            for original in &originals {
                let tmp = tempfile::tempdir().unwrap();
                let path = tmp.path().join("settings.json");
                let text = format!("{}\n", serde_json::to_string_pretty(original).unwrap());
                fs::write(&path, &text).unwrap();

                register(&path, &Output::quiet()).unwrap();
                unregister(&path, &Output::quiet());

                assert_eq!(fs::read_to_string(&path).unwrap(), text);
            }
        }
    }

    #[test]
    fn unregistering_keeps_user_handlers_in_symposiums_group() {
        type Register = fn(&Path, &Output) -> Result<()>;
        type Unregister = fn(&Path, &Output);
        let agents: [(Register, Unregister); 2] = [
            (register_claude_hooks, unregister_claude_hooks),
            (register_codex_hooks, unregister_codex_hooks),
        ];
        let user_hook = json!({ "type": "command", "command": "user-hook" });
        for (register, unregister) in agents {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("settings.json");
            register(&path, &Output::quiet()).unwrap();

            let mut settings: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
            let group = &mut settings["hooks"]["PreToolUse"][0];
            let matcher = group["matcher"].clone();
            group["hooks"]
                .as_array_mut()
                .unwrap()
                .push(user_hook.clone());
            fs::write(&path, settings.to_string()).unwrap();

            unregister(&path, &Output::quiet());

            let content = fs::read_to_string(&path).unwrap();
            let after: serde_json::Value = serde_json::from_str(&content).unwrap();
            assert_eq!(
                after["hooks"]["PreToolUse"],
                json!([{ "matcher": matcher, "hooks": [user_hook] }])
            );
            assert!(!content.contains("cargo-agents hook"));
        }
    }

    #[test]
    fn hook_registration_keeps_the_file_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.json");
        fs::write(
            &path,
            "{\n    \"model\": \"opus\",\n    \"permissions\": {}\n}\n",
        )
        .unwrap();

        register_claude_hooks(&path, &Output::quiet()).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(
            text.starts_with(
                "{\n    \"model\": \"opus\",\n    \"permissions\": {},\n    \"hooks\": {"
            ),
            "{text}"
        );
        assert!(text.ends_with("}\n"), "{text}");
    }

    #[test]
    fn unregistering_hooks_keeps_the_order_of_the_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.json");
        let text = "{\n  \"hooks\": {\n    \"PreToolUse\": [\n      {\n        \"hooks\": [\n          {\n            \"command\": \"cargo-agents hook claude pre-tool-use\",\n            \"type\": \"command\"\n          }\n        ]\n      }\n    ],\n    \"A\": [],\n    \"B\": []\n  }\n}\n";
        fs::write(&path, text).unwrap();
        unregister_claude_hooks(&path, &Output::quiet());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "{\n  \"hooks\": {\n    \"A\": [],\n    \"B\": []\n  }\n}\n"
        );
    }

    #[test]
    fn agent_from_config_name_opencode() {
        assert_eq!(
            Agent::from_config_name("opencode").unwrap(),
            Agent::OpenCode
        );
    }

    #[test]
    fn opencode_project_skill_dir() {
        let root = Path::new("/project");
        assert_eq!(
            Agent::OpenCode.project_skill_dir(root, "tokio"),
            PathBuf::from("/project/.opencode/skills/tokio")
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
    // ── Antigravity ──────────────────────────────────────────────────────

    #[test]
    fn antigravity_hooks_use_the_right_shape_per_event() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("hooks.json");
        register_antigravity_hooks(&path, &Output::quiet()).unwrap();

        let cfg: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let entry = &cfg["symposium"];

        // Tool events wrap handlers in a matcher group.
        let pre_tool = &entry["PreToolUse"][0];
        assert_eq!(pre_tool["matcher"], "*");
        assert_eq!(
            pre_tool["hooks"][0]["command"],
            "cargo-agents hook antigravity pre-tool-use"
        );

        // Lifecycle events are flat handler lists — no matcher wrapper.
        let pre_invocation = &entry["PreInvocation"][0];
        assert!(pre_invocation.get("matcher").is_none());
        assert_eq!(
            pre_invocation["command"],
            "cargo-agents hook antigravity user-prompt-submit"
        );

        // SessionStart is undocumented but real; it fires with the first turn
        // of a new conversation and never on resume.
        assert_eq!(
            entry["SessionStart"][0]["command"],
            "cargo-agents hook antigravity session-start"
        );
        assert_eq!(
            entry["Stop"][0]["command"],
            "cargo-agents hook antigravity stop"
        );

        // Timeout is in seconds for this agent, not milliseconds.
        assert_eq!(pre_invocation["timeout"], 30);
    }

    #[test]
    fn antigravity_hooks_registration_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("hooks.json");
        register_antigravity_hooks(&path, &Output::quiet()).unwrap();
        let first = fs::read_to_string(&path).unwrap();
        register_antigravity_hooks(&path, &Output::quiet()).unwrap();
        assert_eq!(first, fs::read_to_string(&path).unwrap());
    }

    /// The file is shared with whatever else the user registered, so reaping
    /// must take symposium's named entry and nothing else.
    #[test]
    fn antigravity_unregister_leaves_other_named_hooks_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("hooks.json");
        fs::write(
            &path,
            serde_json::to_string_pretty(&json!({
                "someone-elses-linter": { "PostToolUse": [{ "matcher": "*", "hooks": [] }] }
            }))
            .unwrap(),
        )
        .unwrap();

        register_antigravity_hooks(&path, &Output::quiet()).unwrap();
        unregister_antigravity_hooks(&path, &Output::quiet());

        let cfg: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(cfg.get("symposium").is_none(), "symposium entry removed");
        assert!(
            cfg.get("someone-elses-linter").is_some(),
            "unrelated hook preserved"
        );
    }

    /// Antigravity keeps MCP in its own file rather than sharing one with
    /// hooks, and the project and global locations are different shapes — so
    /// the scope dispatch is worth pinning down.
    #[test]
    fn antigravity_mcp_lands_in_its_own_file_at_both_scopes() {
        use sacp::schema::{McpServer, McpServerStdio};
        let servers = vec![McpServer::Stdio(McpServerStdio::new(
            "symposium",
            "/usr/local/bin/cargo-agents",
        ))];

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let sym = Symposium::from_dir(root);

        Agent::Antigravity
            .register_mcp_servers(McpScope::Project, root, &sym, &servers, &Output::quiet())
            .unwrap();
        let project = root.join(".agents/mcp_config.json");
        assert!(
            project.is_file(),
            "project MCP goes to .agents/mcp_config.json"
        );
        let cfg: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&project).unwrap()).unwrap();
        assert!(cfg["mcpServers"]["symposium"].is_object());

        Agent::Antigravity
            .register_mcp_servers(McpScope::User, root, &sym, &servers, &Output::quiet())
            .unwrap();
        assert!(
            root.join(".gemini/config/mcp_config.json").is_file(),
            "global MCP goes to the shared config root"
        );
    }

    #[test]
    fn antigravity_skill_and_config_paths() {
        let root = Path::new("/project");
        let home = Path::new("/home/user");
        assert_eq!(
            Agent::Antigravity.project_skill_dir(root, "tokio"),
            PathBuf::from("/project/.agents/skills/tokio")
        );
        assert_eq!(
            Agent::Antigravity.global_skill_dir(home, "tokio"),
            Some(PathBuf::from("/home/user/.gemini/config/skills/tokio"))
        );
        assert_eq!(
            Agent::from_config_name("antigravity").unwrap(),
            Agent::Antigravity
        );
    }
}
