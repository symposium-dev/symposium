//! Agent abstraction: hook registration and extension installation paths.
//!
//! Each supported agent has different conventions for where hooks are
//! configured and where skill files are placed. This module centralizes
//! that knowledge.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use serde_json::json;

use crate::output::{Output, display_path};

/// Supported AI agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    Claude,
    Copilot,
    Gemini,
}

impl Agent {
    /// Parse an agent name from a config string.
    pub fn from_config_name(name: &str) -> Result<Self> {
        match name {
            "claude" => Ok(Agent::Claude),
            "copilot" => Ok(Agent::Copilot),
            "gemini" => Ok(Agent::Gemini),
            other => bail!("unknown agent: {other} (expected claude, copilot, or gemini)"),
        }
    }

    /// Config name as stored in TOML.
    pub fn config_name(&self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Copilot => "copilot",
            Agent::Gemini => "gemini",
        }
    }

    /// Human-readable display name.
    pub fn display_name(&self) -> &'static str {
        match self {
            Agent::Claude => "Claude Code",
            Agent::Copilot => "GitHub Copilot",
            Agent::Gemini => "Gemini CLI",
        }
    }

    /// All supported agents for interactive prompts.
    pub fn all() -> &'static [Agent] {
        &[Agent::Claude, Agent::Copilot, Agent::Gemini]
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
            Agent::Claude => project_root
                .join(".claude")
                .join("skills")
                .join(skill_name),
            Agent::Copilot | Agent::Gemini => project_root
                .join(".agents")
                .join("skills")
                .join(skill_name),
        }
    }

    /// Global skill directory for a given skill name, if supported.
    pub fn global_skill_dir(&self, skill_name: &str) -> Option<PathBuf> {
        let home = dirs::home_dir()?;
        match self {
            Agent::Claude => Some(home.join(".claude").join("skills").join(skill_name)),
            Agent::Copilot => None, // no global skills path
            Agent::Gemini => Some(home.join(".gemini").join("skills").join(skill_name)),
        }
    }

    // -----------------------------------------------------------------------
    // Hook registration
    // -----------------------------------------------------------------------

    /// Register hooks in the project-level agent config.
    pub fn register_project_hooks(&self, project_root: &Path, out: &Output) -> Result<()> {
        match self {
            Agent::Claude => register_claude_hooks(
                &project_root.join(".claude").join("settings.json"),
                out,
            ),
            Agent::Copilot => register_copilot_hooks(
                &project_root.join(".github").join("hooks"),
                out,
            ),
            Agent::Gemini => register_gemini_hooks(
                &project_root.join(".gemini").join("settings.json"),
                out,
            ),
        }
    }

    /// Register hooks in the global agent config.
    pub fn register_global_hooks(&self, out: &Output) -> Result<()> {
        let home = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("could not determine home directory"))?;
        match self {
            Agent::Claude => {
                register_claude_hooks(&home.join(".claude").join("settings.json"), out)
            }
            Agent::Copilot => {
                register_copilot_hooks(&home.join(".copilot").join("config.json"), out)
            }
            Agent::Gemini => {
                register_gemini_hooks(&home.join(".gemini").join("settings.json"), out)
            }
        }
    }

    /// Install a single skill file into the agent's expected location.
    pub fn install_skill(
        &self,
        skill_source: &Path,
        dest_dir: &Path,
    ) -> Result<()> {
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

    for event in ["PreToolUse", "PostToolUse", "UserPromptSubmit"] {
        let command = format!("cargo agents hook claude {}", event_to_cli_arg(event));
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
    let event_hooks = hooks
        .entry(event)
        .or_insert_with(|| json!([]));

    let arr = match event_hooks.as_array_mut() {
        Some(a) => a,
        None => return false,
    };

    let already_registered = arr.iter().any(|group| {
        group.get("hooks").and_then(|h| h.as_array()).map_or(false, |hooks| {
            hooks.iter().any(|h| {
                h.get("command")
                    .and_then(|c| c.as_str())
                    .is_some_and(|c| c.starts_with("cargo agents hook"))
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
// GitHub Copilot hook registration
// ---------------------------------------------------------------------------

fn register_copilot_hooks(hooks_dir: &Path, out: &Output) -> Result<()> {
    fs::create_dir_all(hooks_dir)?;
    let hook_file = hooks_dir.join("cargo-agents.json");
    let display = display_path(&hook_file);

    if hook_file.exists() {
        let existing = fs::read_to_string(&hook_file)?;
        if existing.contains("cargo agents hook") {
            out.already_ok(format!("{display}: hooks already registered"));
            return Ok(());
        }
    }

    let hooks = json!({
        "version": 1,
        "hooks": {
            "preToolUse": [{
                "type": "command",
                "bash": "cargo agents hook copilot pre-tool-use",
                "timeoutSec": 10
            }],
            "postToolUse": [{
                "type": "command",
                "bash": "cargo agents hook copilot post-tool-use",
                "timeoutSec": 10
            }],
            "userPromptSubmitted": [{
                "type": "command",
                "bash": "cargo agents hook copilot user-prompt-submit",
                "timeoutSec": 10
            }]
        }
    });

    save_json(&hook_file, &hooks)?;
    out.done(format!("{display}: added hooks (preToolUse, postToolUse, userPromptSubmitted)"));
    Ok(())
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
    ];

    for (gemini_event, cli_arg) in events {
        let command = format!("cargo agents hook gemini {cli_arg}");
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
    let event_hooks = hooks
        .entry(event)
        .or_insert_with(|| json!([]));

    let arr = match event_hooks.as_array_mut() {
        Some(a) => a,
        None => return false,
    };

    let already_registered = arr.iter().any(|group| {
        group.get("hooks").and_then(|h| h.as_array()).map_or(false, |hooks| {
            hooks.iter().any(|h| {
                h.get("command")
                    .and_then(|c| c.as_str())
                    .is_some_and(|c| c.starts_with("cargo agents hook"))
            })
        })
    });

    if already_registered {
        return false;
    }

    arr.push(json!({
        "matcher": ".*",
        "hooks": [{
            "name": "cargo-agents",
            "type": "command",
            "command": command,
            "timeout": 10000
        }]
    }));
    true
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn event_to_cli_arg(event: &str) -> &str {
    match event {
        "PreToolUse" => "pre-tool-use",
        "PostToolUse" => "post-tool-use",
        "UserPromptSubmit" => "user-prompt-submit",
        other => other,
    }
}

fn load_json_or_empty(path: &Path) -> Result<serde_json::Value> {
    if path.exists() {
        let contents = fs::read_to_string(path)?;
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
    fn register_copilot_hooks_creates_file() {
        let tmp = tempfile::tempdir().unwrap();
        let hooks_dir = tmp.path().join("hooks");
        register_copilot_hooks(&hooks_dir, &Output::quiet()).unwrap();

        let hook_file = hooks_dir.join("cargo-agents.json");
        assert!(hook_file.exists());
        let content: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&hook_file).unwrap()).unwrap();
        assert_eq!(content["version"], 1);
        assert!(content["hooks"]["preToolUse"].is_array());
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
