//! MCP server registration and unregistration for all supported agents.
//!
//! Each agent has its own config format and file location. This module
//! provides per-agent `register_*` and `unregister_*` functions that
//! are called from the `Agent` methods in the parent module.
//!
//! Registration is idempotent: existing entries with correct values are
//! left untouched, while stale entries are updated in place.

use std::fs;
use std::path::Path;

use anyhow::Result;
use indoc::formatdoc;
use sacp::schema::McpServer;
use serde_json::json;

use crate::output::{Output, display_path};

use super::{load_json_or_empty, save_json};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the name from any McpServer variant.
fn server_name(server: &McpServer) -> &str {
    match server {
        McpServer::Stdio(s) => &s.name,
        McpServer::Http(s) => &s.name,
        McpServer::Sse(s) => &s.name,
        _ => panic!("unsupported McpServer variant"),
    }
}

/// Convert an McpServer to the JSON value agents expect in their config.
///
/// Stdio: `{"command": "...", "args": [...], "env": [...]}`
/// Http/Sse: `{"url": "...", "headers": [...]}`
///
/// `env` and `headers` are omitted when empty.
fn server_to_json(server: &McpServer) -> serde_json::Value {
    match server {
        McpServer::Stdio(s) => {
            let mut v = json!({
                "command": s.command.to_string_lossy(),
                "args": s.args,
            });
            if !s.env.is_empty() {
                v["env"] = serde_json::to_value(&s.env).unwrap();
            }
            v
        }
        McpServer::Http(s) => {
            let mut v = json!({ "url": s.url });
            if !s.headers.is_empty() {
                v["headers"] = serde_json::to_value(&s.headers).unwrap();
            }
            v
        }
        McpServer::Sse(s) => {
            let mut v = json!({ "url": s.url });
            if !s.headers.is_empty() {
                v["headers"] = serde_json::to_value(&s.headers).unwrap();
            }
            v
        }
        _ => panic!("unsupported McpServer variant"),
    }
}

/// Result of upserting an MCP server entry.
enum UpsertResult {
    AlreadyCorrect,
    Inserted,
    Updated,
}

/// Upsert a single MCP server entry into a JSON object container.
fn upsert_json_mcp_entry(
    container: &mut serde_json::Value,
    name: &str,
    expected: &serde_json::Value,
) -> UpsertResult {
    if let Some(existing) = container.get(name) {
        if existing == expected {
            return UpsertResult::AlreadyCorrect;
        }
        container[name] = expected.clone();
        UpsertResult::Updated
    } else {
        container[name] = expected.clone();
        UpsertResult::Inserted
    }
}

// ---------------------------------------------------------------------------
// JSON-based registration (Claude, Copilot, Gemini, Kiro, OpenCode)
// ---------------------------------------------------------------------------

/// Register MCP servers into a JSON config file under a given container key.
///
/// If `container_key` is `Some("mcpServers")`, entries go under `config["mcpServers"][name]`.
/// If `None`, entries go at the top level `config[name]`.
fn register_json_mcp_servers(
    config_path: &Path,
    servers: &[McpServer],
    container_key: Option<&str>,
    out: &Output,
) -> Result<()> {
    let display = display_path(config_path);
    let mut config = load_json_or_empty(config_path)?;

    if !config.is_object() {
        config = json!({});
    }

    let container = if let Some(key) = container_key {
        if !config.get(key).is_some_and(|v| v.is_object()) {
            config[key] = json!({});
        }
        &mut config[key]
    } else {
        &mut config
    };

    let mut changed = false;
    for server in servers {
        let name = server_name(server);
        let expected = server_to_json(server);
        match upsert_json_mcp_entry(container, name, &expected) {
            UpsertResult::AlreadyCorrect => {
                out.already_ok(format!("{display}: {name} MCP server already configured"));
            }
            UpsertResult::Inserted => {
                out.done(format!("{display}: added {name} MCP server"));
                changed = true;
            }
            UpsertResult::Updated => {
                out.done(format!("{display}: updated {name} MCP server"));
                changed = true;
            }
        }
    }

    if changed {
        save_json(config_path, &config)?;
    }
    Ok(())
}

/// Remove MCP server entries from a JSON config file.
fn unregister_json_mcp_servers(
    config_path: &Path,
    names: &[&str],
    container_key: Option<&str>,
    out: &Output,
) -> Result<()> {
    let display = display_path(config_path);
    if !config_path.exists() {
        return Ok(());
    }

    let mut config = load_json_or_empty(config_path)?;

    let container = if let Some(key) = container_key {
        config.get_mut(key).and_then(|v| v.as_object_mut())
    } else {
        config.as_object_mut()
    };

    let Some(obj) = container else {
        return Ok(());
    };

    let mut changed = false;
    for name in names {
        if obj.remove(*name).is_some() {
            out.removed(format!("{display}: removed {name} MCP server"));
            changed = true;
        }
    }

    if changed {
        save_json(config_path, &config)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Per-agent registration functions
// ---------------------------------------------------------------------------

/// Claude Code: `mcpServers.<name>` in settings.json
pub(super) fn register_claude_mcp_servers(
    path: &Path,
    servers: &[McpServer],
    out: &Output,
) -> Result<()> {
    register_json_mcp_servers(path, servers, Some("mcpServers"), out)
}

pub(super) fn unregister_claude_mcp_servers(
    path: &Path,
    names: &[&str],
    out: &Output,
) -> Result<()> {
    unregister_json_mcp_servers(path, names, Some("mcpServers"), out)
}

/// Codex CLI: `[mcp_servers.<name>]` in config.toml
pub(super) fn register_codex_mcp_servers(
    config_path: &Path,
    servers: &[McpServer],
    out: &Output,
) -> Result<()> {
    let display = display_path(config_path);

    let content = if config_path.exists() {
        fs::read_to_string(config_path)?
    } else {
        String::new()
    };

    let mut doc: toml_edit::DocumentMut = content
        .parse()
        .unwrap_or_else(|_| toml_edit::DocumentMut::new());

    if !doc.contains_key("mcp_servers") {
        doc["mcp_servers"] = toml_edit::Item::Table(toml_edit::Table::new());
    }

    let mut changed = false;
    for server in servers {
        let name = server_name(server);
        let McpServer::Stdio(stdio) = server else {
            out.info(format!(
                "{display}: skipping non-stdio MCP server {name} (Codex only supports stdio)"
            ));
            continue;
        };

        let cmd = stdio.command.to_string_lossy().to_string();
        let needs_update = if let Some(existing) = doc["mcp_servers"].get(name) {
            let cmd_ok = existing.get("command").and_then(|v| v.as_str()) == Some(&cmd);
            let args_ok = existing
                .get("args")
                .and_then(|v| v.as_array())
                .is_some_and(|a| {
                    a.iter()
                        .map(|v| v.as_str().unwrap_or(""))
                        .collect::<Vec<_>>()
                        == stdio.args.iter().map(|s| s.as_str()).collect::<Vec<_>>()
                });
            if cmd_ok && args_ok {
                out.already_ok(format!("{display}: {name} MCP server already configured"));
                false
            } else {
                true
            }
        } else {
            true
        };

        if needs_update {
            let mut server_table = toml_edit::Table::new();
            server_table["command"] = toml_edit::value(&cmd);
            let mut args = toml_edit::Array::new();
            for arg in &stdio.args {
                args.push(arg.as_str());
            }
            server_table["args"] = toml_edit::value(args);
            let is_new = doc["mcp_servers"].get(name).is_none();
            doc["mcp_servers"][name] = toml_edit::Item::Table(server_table);
            let verb = if is_new { "added" } else { "updated" };
            out.done(format!("{display}: {verb} {name} MCP server"));
            changed = true;
        }
    }

    if changed {
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(config_path, doc.to_string())?;
    }
    Ok(())
}

pub(super) fn unregister_codex_mcp_servers(
    config_path: &Path,
    names: &[&str],
    out: &Output,
) -> Result<()> {
    let display = display_path(config_path);
    if !config_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(config_path)?;
    let mut doc: toml_edit::DocumentMut = content.parse()?;

    let Some(mcp_servers) = doc.get_mut("mcp_servers").and_then(|v| v.as_table_mut()) else {
        return Ok(());
    };

    let mut changed = false;
    for name in names {
        if mcp_servers.remove(*name).is_some() {
            out.removed(format!("{display}: removed {name} MCP server"));
            changed = true;
        }
    }

    if changed {
        fs::write(config_path, doc.to_string())?;
    }
    Ok(())
}

/// Copilot: top-level `<name>` in mcp.json
pub(super) fn register_copilot_mcp_servers(
    path: &Path,
    servers: &[McpServer],
    out: &Output,
) -> Result<()> {
    register_json_mcp_servers(path, servers, None, out)
}

pub(super) fn unregister_copilot_mcp_servers(
    path: &Path,
    names: &[&str],
    out: &Output,
) -> Result<()> {
    unregister_json_mcp_servers(path, names, None, out)
}

/// Gemini CLI: same format as Claude (`mcpServers.<name>`)
pub(super) fn register_gemini_mcp_servers(
    path: &Path,
    servers: &[McpServer],
    out: &Output,
) -> Result<()> {
    register_claude_mcp_servers(path, servers, out)
}

pub(super) fn unregister_gemini_mcp_servers(
    path: &Path,
    names: &[&str],
    out: &Output,
) -> Result<()> {
    unregister_claude_mcp_servers(path, names, out)
}

/// Kiro: `mcpServers.<name>` in mcp.json
pub(super) fn register_kiro_mcp_servers(
    path: &Path,
    servers: &[McpServer],
    out: &Output,
) -> Result<()> {
    register_claude_mcp_servers(path, servers, out)
}

pub(super) fn unregister_kiro_mcp_servers(path: &Path, names: &[&str], out: &Output) -> Result<()> {
    unregister_claude_mcp_servers(path, names, out)
}

/// Goose: `extensions.<name>` in config.yaml (string manipulation to preserve comments)
pub(super) fn register_goose_mcp_servers(
    config_path: &Path,
    servers: &[McpServer],
    out: &Output,
) -> Result<()> {
    let display = display_path(config_path);

    let mut content = if config_path.exists() {
        fs::read_to_string(config_path)?
    } else {
        String::new()
    };

    let mut changed = false;
    for server in servers {
        let name = server_name(server);
        let McpServer::Stdio(stdio) = server else {
            out.info(format!(
                "{display}: skipping non-stdio MCP server {name} (Goose extensions use stdio)"
            ));
            continue;
        };

        let cmd = stdio.command.to_string_lossy();
        let quoted_args: Vec<_> = stdio
            .args
            .iter()
            .map(|a| format!("\"{}\"", a.replace('"', "\\\"")))
            .collect();
        let args_yaml = format!("[{}]", quoted_args.join(", "));

        let snippet = formatdoc! {"
            {name}:
                provider: mcp
                config:
                  command: \"{cmd}\"
                  args: {args_yaml}
        "};

        let needle = format!("{name}:");
        let already_exists = content.contains(&needle);

        if already_exists {
            // Check if the existing entry matches — parse out the section
            // and compare command/args. If it matches, skip; otherwise
            // remove the old section so we can re-insert below.
            let lines: Vec<&str> = content.lines().collect();
            let mut new_lines = Vec::new();
            let mut in_section = false;
            let mut section_indent = 0;
            let mut old_section = String::new();

            for line in &lines {
                let trimmed = line.trim();
                if !in_section && !trimmed.is_empty() && trimmed.starts_with(&needle) {
                    section_indent = line.len() - trimmed.len();
                    in_section = true;
                    old_section.push_str(trimmed);
                    old_section.push('\n');
                    continue;
                }
                if in_section && !trimmed.is_empty() {
                    let line_indent = line.len() - trimmed.len();
                    if line_indent <= section_indent {
                        in_section = false;
                    }
                }
                if in_section {
                    old_section.push_str(trimmed);
                    old_section.push('\n');
                } else {
                    new_lines.push(*line);
                }
            }

            // Rebuild expected snippet for comparison (trimmed, no leading indent)
            let expected_trimmed = snippet.trim();
            if old_section.trim() == expected_trimmed {
                out.already_ok(format!("{display}: {name} MCP server already configured"));
                continue;
            }

            // Stale — remove old section, fall through to insert
            content = new_lines.join("\n");
        }

        content = if content.trim().is_empty() {
            format!("extensions:\n  {}", snippet.trim())
        } else if content.contains("extensions:") {
            content.replace("extensions:", &format!("extensions:\n  {}", snippet.trim()))
        } else {
            format!("{}\nextensions:\n  {}", content.trim(), snippet.trim())
        };

        let verb = if already_exists { "updated" } else { "added" };
        out.done(format!("{display}: {verb} {name} MCP server"));
        changed = true;
    }

    if changed {
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(config_path, content)?;
    }
    Ok(())
}

pub(super) fn unregister_goose_mcp_servers(
    config_path: &Path,
    names: &[&str],
    out: &Output,
) -> Result<()> {
    let display = display_path(config_path);
    if !config_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(config_path)?;
    let lines: Vec<&str> = content.lines().collect();
    let mut new_lines = Vec::new();
    let mut in_section = false;
    let mut section_indent = 0;
    let mut changed = false;

    for line in lines {
        let trimmed = line.trim();
        if !trimmed.is_empty() && names.iter().any(|n| trimmed.starts_with(&format!("{n}:"))) {
            section_indent = line.len() - trimmed.len();
            in_section = true;
            changed = true;
            let name = trimmed.split(':').next().unwrap_or("?");
            out.removed(format!("{display}: removed {name} MCP server"));
            continue;
        }
        if in_section && !trimmed.is_empty() {
            let line_indent = line.len() - trimmed.len();
            if line_indent <= section_indent {
                in_section = false;
            }
        }
        if !in_section {
            new_lines.push(line);
        }
    }

    if changed {
        fs::write(config_path, new_lines.join("\n"))?;
    }
    Ok(())
}

/// OpenCode: `mcp.<name>` in opencode.json
pub(super) fn register_opencode_mcp_servers(
    path: &Path,
    servers: &[McpServer],
    out: &Output,
) -> Result<()> {
    register_json_mcp_servers(path, servers, Some("mcp"), out)
}

pub(super) fn unregister_opencode_mcp_servers(
    path: &Path,
    names: &[&str],
    out: &Output,
) -> Result<()> {
    unregister_json_mcp_servers(path, names, Some("mcp"), out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sacp::schema::McpServerStdio;

    fn test_servers() -> Vec<McpServer> {
        vec![McpServer::Stdio(
            McpServerStdio::new("symposium", "/usr/local/bin/symposium").args(vec!["mcp".into()]),
        )]
    }

    fn test_server_names() -> Vec<&'static str> {
        vec!["symposium"]
    }

    // -- Claude MCP (also covers Gemini and Kiro via delegation) --

    #[test]
    fn register_claude_creates_config() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.json");
        register_claude_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();

        let settings: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            settings["mcpServers"]["symposium"]["command"],
            "/usr/local/bin/symposium"
        );
        assert_eq!(settings["mcpServers"]["symposium"]["args"][0], "mcp");
    }

    #[test]
    fn register_claude_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.json");
        register_claude_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();
        register_claude_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();

        let settings: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(settings["mcpServers"].as_object().unwrap().len(), 1);
    }

    #[test]
    fn register_claude_updates_stale() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.json");
        let stale = json!({"mcpServers": {"symposium": {"command": "/old/path", "args": ["mcp"]}}});
        save_json(&path, &stale).unwrap();

        register_claude_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();

        let settings: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            settings["mcpServers"]["symposium"]["command"],
            "/usr/local/bin/symposium"
        );
    }

    #[test]
    fn register_claude_recovers_non_object_container() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.json");
        // mcpServers is a string instead of an object
        save_json(&path, &json!({"mcpServers": "corrupted"})).unwrap();

        register_claude_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();

        let settings: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            settings["mcpServers"]["symposium"]["command"],
            "/usr/local/bin/symposium"
        );
    }

    #[test]
    fn unregister_claude_removes_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.json");
        register_claude_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();
        unregister_claude_mcp_servers(&path, &test_server_names(), &Output::quiet()).unwrap();

        let settings: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(settings["mcpServers"].get("symposium").is_none());
    }

    // -- Codex MCP (TOML) --

    #[test]
    fn register_codex_creates_config() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        register_codex_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let doc: toml::Value = content.parse().unwrap();
        assert_eq!(
            doc["mcp_servers"]["symposium"]["command"].as_str().unwrap(),
            "/usr/local/bin/symposium"
        );
        assert_eq!(
            doc["mcp_servers"]["symposium"]["args"].as_array().unwrap()[0]
                .as_str()
                .unwrap(),
            "mcp"
        );
    }

    #[test]
    fn register_codex_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        register_codex_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();
        register_codex_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let doc: toml::Value = content.parse().unwrap();
        assert_eq!(doc["mcp_servers"].as_table().unwrap().len(), 1);
    }

    #[test]
    fn register_codex_updates_stale() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        fs::write(
            &path,
            "[mcp_servers.symposium]\ncommand = \"/old/path\"\nargs = [\"old-arg\"]\n",
        )
        .unwrap();

        register_codex_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let doc: toml::Value = content.parse().unwrap();
        let entry = &doc["mcp_servers"]["symposium"];
        assert_eq!(
            entry["command"].as_str().unwrap(),
            "/usr/local/bin/symposium"
        );
        assert_eq!(
            entry["args"].as_array().unwrap()[0].as_str().unwrap(),
            "mcp"
        );
        // Ensure no duplicate — still exactly one server entry
        assert_eq!(doc["mcp_servers"].as_table().unwrap().len(), 1);
    }

    #[test]
    fn unregister_codex_removes_section() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        register_codex_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();
        unregister_codex_mcp_servers(&path, &test_server_names(), &Output::quiet()).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let doc: toml::Value = content.parse().unwrap();
        assert!(
            doc.get("mcp_servers")
                .and_then(|s| s.get("symposium"))
                .is_none()
        );
    }

    // -- Copilot MCP --

    #[test]
    fn register_copilot_creates_config() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("mcp.json");
        register_copilot_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();

        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(config["symposium"]["command"], "/usr/local/bin/symposium");
        assert_eq!(config["symposium"]["args"][0], "mcp");
    }

    #[test]
    fn register_copilot_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("mcp.json");
        register_copilot_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();
        register_copilot_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();

        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(config.as_object().unwrap().len(), 1);
    }

    #[test]
    fn register_copilot_updates_stale() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("mcp.json");
        let stale = json!({"symposium": {"command": "/old/path", "args": ["mcp"]}});
        save_json(&path, &stale).unwrap();

        register_copilot_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();

        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(config["symposium"]["command"], "/usr/local/bin/symposium");
    }

    #[test]
    fn unregister_copilot_removes_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("mcp.json");
        register_copilot_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();
        unregister_copilot_mcp_servers(&path, &test_server_names(), &Output::quiet()).unwrap();

        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(config.get("symposium").is_none());
    }

    // -- Goose MCP (YAML) --

    #[test]
    fn register_goose_creates_config() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.yaml");
        register_goose_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let doc: serde_yaml_ng::Value = serde_yaml_ng::from_str(&content).unwrap();
        let ext = &doc["extensions"]["symposium"];
        assert_eq!(ext["provider"].as_str().unwrap(), "mcp");
        assert_eq!(
            ext["config"]["command"].as_str().unwrap(),
            "/usr/local/bin/symposium"
        );
    }

    #[test]
    fn register_goose_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.yaml");
        register_goose_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();
        register_goose_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let doc: serde_yaml_ng::Value = serde_yaml_ng::from_str(&content).unwrap();
        assert_eq!(doc["extensions"].as_mapping().unwrap().len(), 1);
    }

    #[test]
    fn unregister_goose_removes_section() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.yaml");
        register_goose_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();
        unregister_goose_mcp_servers(&path, &test_server_names(), &Output::quiet()).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        if !content.trim().is_empty() {
            let doc: serde_yaml_ng::Value = serde_yaml_ng::from_str(&content).unwrap();
            assert!(
                doc.get("extensions")
                    .and_then(|e| e.get("symposium"))
                    .is_none()
            );
        }
    }

    #[test]
    fn register_goose_updates_stale() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.yaml");
        // Write a config with an old binary path
        fs::write(&path, "extensions:\n  symposium:\n    provider: mcp\n    config:\n      command: \"/old/path\"\n      args: [\"mcp\"]\n").unwrap();

        register_goose_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let doc: serde_yaml_ng::Value = serde_yaml_ng::from_str(&content).unwrap();
        assert_eq!(
            doc["extensions"]["symposium"]["config"]["command"]
                .as_str()
                .unwrap(),
            "/usr/local/bin/symposium",
        );
        // Still exactly one extension
        assert_eq!(doc["extensions"].as_mapping().unwrap().len(), 1);
    }

    #[test]
    fn register_goose_quotes_special_chars() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.yaml");
        let servers = vec![McpServer::Stdio(
            McpServerStdio::new("test-server", "/path with spaces/symposium")
                .args(vec!["--flag:value".into()]),
        )];
        register_goose_mcp_servers(&path, &servers, &Output::quiet()).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        // Must be valid YAML
        let doc: serde_yaml_ng::Value = serde_yaml_ng::from_str(&content).unwrap();
        assert_eq!(
            doc["extensions"]["test-server"]["config"]["command"]
                .as_str()
                .unwrap(),
            "/path with spaces/symposium",
        );
    }

    // -- OpenCode MCP --

    #[test]
    fn register_opencode_creates_config() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("opencode.json");
        register_opencode_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();

        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            config["mcp"]["symposium"]["command"],
            "/usr/local/bin/symposium"
        );
        assert_eq!(config["mcp"]["symposium"]["args"][0], "mcp");
    }

    #[test]
    fn register_opencode_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("opencode.json");
        register_opencode_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();
        register_opencode_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();

        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(config["mcp"].as_object().unwrap().len(), 1);
    }

    #[test]
    fn register_opencode_updates_stale() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("opencode.json");
        let stale = json!({"mcp": {"symposium": {"command": "/old/path", "args": ["mcp"]}}});
        save_json(&path, &stale).unwrap();

        register_opencode_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();

        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            config["mcp"]["symposium"]["command"],
            "/usr/local/bin/symposium"
        );
    }

    #[test]
    fn unregister_opencode_removes_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("opencode.json");
        register_opencode_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();
        unregister_opencode_mcp_servers(&path, &test_server_names(), &Output::quiet()).unwrap();

        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(config["mcp"].get("symposium").is_none());
    }
}
