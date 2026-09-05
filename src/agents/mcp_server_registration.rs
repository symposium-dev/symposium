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
/// Stdio: `{"command": "...", "args": [...], "env": {...}}`
/// Http/Sse: `{"url": "...", "headers": {...}}`
///
/// `env`/`headers` are objects, not ACP's `[{name, value}]` list: a list is
/// skipped or rejected, silently for an entry that only differs by carrying env.
fn server_to_json(server: &McpServer) -> serde_json::Value {
    match server {
        McpServer::Stdio(s) => {
            let mut v = json!({
                "command": s.command.to_string_lossy(),
                "args": s.args,
            });
            if !s.env.is_empty() {
                v["env"] = pairs_to_object(s.env.iter().map(|e| (&e.name, &e.value)));
            }
            v
        }
        McpServer::Http(s) => {
            let mut v = json!({ "url": s.url });
            if !s.headers.is_empty() {
                v["headers"] = pairs_to_object(s.headers.iter().map(|h| (&h.name, &h.value)));
            }
            v
        }
        McpServer::Sse(s) => {
            let mut v = json!({ "url": s.url });
            if !s.headers.is_empty() {
                v["headers"] = pairs_to_object(s.headers.iter().map(|h| (&h.name, &h.value)));
            }
            v
        }
        _ => panic!("unsupported McpServer variant"),
    }
}

/// [`server_to_json`] plus the explicit `type` its own `mcp add` writes.
/// Without it a remote entry is not picked up at all.
fn copilot_server_to_json(server: &McpServer) -> serde_json::Value {
    let mut v = server_to_json(server);
    v["type"] = json!(match server {
        McpServer::Stdio(_) => "local",
        McpServer::Sse(_) => "sse",
        _ => "http",
    });
    v
}

fn pairs_to_object<'a>(pairs: impl Iterator<Item = (&'a String, &'a String)>) -> serde_json::Value {
    serde_json::Value::Object(
        pairs
            .map(|(name, value)| (name.clone(), serde_json::Value::String(value.clone())))
            .collect(),
    )
}

/// OpenCode rejects the whole config file given the common `{command, args}`
/// shape: `type` is required, the command is one array of binary plus args, and
/// env goes under `environment` (an `env` key parses but never reaches the child).
fn opencode_server_to_json(server: &McpServer) -> serde_json::Value {
    match server {
        McpServer::Stdio(s) => {
            let mut command = vec![s.command.to_string_lossy().into_owned()];
            command.extend(s.args.iter().cloned());
            let mut v = json!({
                "type": "local",
                "command": command,
                "enabled": true,
            });
            if !s.env.is_empty() {
                v["environment"] = pairs_to_object(s.env.iter().map(|e| (&e.name, &e.value)));
            }
            v
        }
        McpServer::Http(s) => {
            let mut v = json!({ "type": "remote", "url": s.url, "enabled": true });
            if !s.headers.is_empty() {
                v["headers"] = pairs_to_object(s.headers.iter().map(|h| (&h.name, &h.value)));
            }
            v
        }
        McpServer::Sse(s) => {
            let mut v = json!({ "type": "remote", "url": s.url, "enabled": true });
            if !s.headers.is_empty() {
                v["headers"] = pairs_to_object(s.headers.iter().map(|h| (&h.name, &h.value)));
            }
            v
        }
        _ => panic!("unsupported McpServer variant"),
    }
}

/// Render a table as it would appear in a file, sub-tables included, unlike
/// `Table::to_string()` which renders only the table's own values.
fn render_toml_entry(table: &toml_edit::Table) -> String {
    let mut doc = toml_edit::DocumentMut::new();
    doc["entry"] = toml_edit::Item::Table(table.clone());
    doc.to_string()
}

/// Result of upserting an MCP server entry.
enum UpsertResult {
    AlreadyCorrect,
    Inserted,
    Updated,
}

/// Upsert a single MCP server entry into a JSON object container.
///
/// An existing `"enabled": false` is preserved: it is how a user turns one
/// server off, and sync runs per hook event.
fn upsert_json_mcp_entry(
    container: &mut serde_json::Value,
    name: &str,
    expected: &serde_json::Value,
) -> UpsertResult {
    if let Some(existing) = container.get(name) {
        let mut expected = expected.clone();
        if existing.get("enabled") == Some(&serde_json::Value::Bool(false))
            && expected.get("enabled").is_some()
        {
            expected["enabled"] = serde_json::Value::Bool(false);
        }
        if *existing == expected {
            return UpsertResult::AlreadyCorrect;
        }
        container[name] = expected;
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
    register_json_mcp_servers_with(config_path, servers, container_key, server_to_json, out)
}

/// As [`register_json_mcp_servers`], for an agent whose entry shape differs
/// from the common `{command, args}` one.
fn register_json_mcp_servers_with(
    config_path: &Path,
    servers: &[McpServer],
    container_key: Option<&str>,
    to_json: fn(&McpServer) -> serde_json::Value,
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
        let expected = to_json(server);
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

        // Built before the comparison: an entry can differ by `env` or `url`,
        // not just command/args.
        let mut server_table = toml_edit::Table::new();
        match server {
            McpServer::Stdio(stdio) => {
                server_table["command"] =
                    toml_edit::value(stdio.command.to_string_lossy().to_string());
                let mut args = toml_edit::Array::new();
                for arg in &stdio.args {
                    args.push(arg.as_str());
                }
                server_table["args"] = toml_edit::value(args);
                if !stdio.env.is_empty() {
                    let mut env = toml_edit::Table::new();
                    for var in &stdio.env {
                        env[var.name.as_str()] = toml_edit::value(var.value.as_str());
                    }
                    server_table["env"] = toml_edit::Item::Table(env);
                }
            }
            // A bare `url` means streamable HTTP, per `codex mcp add --url`.
            McpServer::Http(http) => {
                server_table["url"] = toml_edit::value(http.url.as_str());
                if !http.headers.is_empty() {
                    // Reported, not dropped: an absent auth header surfaces later
                    // as an opaque connect failure.
                    out.info(format!(
                        "{display}: {name} headers not registered (Codex config has no header field); \
                         the server may fail to authenticate"
                    ));
                }
            }
            // Only one remote form exists, so SSE would be spoken to as
            // streamable HTTP. Skipped rather than written wrong.
            McpServer::Sse(_) => {
                out.info(format!(
                    "{display}: skipping SSE MCP server {name} (Codex supports streamable HTTP only)"
                ));
                continue;
            }
            _ => {
                out.info(format!("{display}: skipping unsupported MCP server {name}"));
                continue;
            }
        }

        let rendered = render_toml_entry(&server_table);
        let already_correct = doc["mcp_servers"]
            .get(name)
            .and_then(|existing| existing.as_table())
            .is_some_and(|existing| render_toml_entry(existing) == rendered);
        if already_correct {
            out.already_ok(format!("{display}: {name} MCP server already configured"));
            continue;
        }

        let is_new = doc["mcp_servers"].get(name).is_none();
        doc["mcp_servers"][name] = toml_edit::Item::Table(server_table);
        let verb = if is_new { "added" } else { "updated" };
        out.done(format!("{display}: {verb} {name} MCP server"));
        changed = true;
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
        if mcp_servers.remove(name).is_some() {
            out.removed(format!("{display}: removed {name} MCP server"));
            changed = true;
        }
    }

    if changed {
        fs::write(config_path, doc.to_string())?;
    }
    Ok(())
}

/// Copilot: `mcpServers.<name>` in mcp-config.json.
///
/// The wrapper is not optional: bare at the top level, the CLI refuses the
/// whole file with `mcpServers: Required`, taking the user's own servers with it.
pub(super) fn register_copilot_mcp_servers(
    path: &Path,
    servers: &[McpServer],
    out: &Output,
) -> Result<()> {
    register_json_mcp_servers_with(
        path,
        servers,
        Some("mcpServers"),
        copilot_server_to_json,
        out,
    )
}

pub(super) fn unregister_copilot_mcp_servers(
    path: &Path,
    names: &[&str],
    out: &Output,
) -> Result<()> {
    unregister_json_mcp_servers(path, names, Some("mcpServers"), out)
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

/// One Goose `extensions.<name>` block.
///
/// Goose's schema, per `goose recipe validate`: a `type` discriminant (`stdio` /
/// `streamable_http`, no `sse`), the binary under `cmd` not `command`, `envs` as
/// a map, and the name repeated inside the entry.
fn goose_extension_yaml(server: &McpServer) -> Option<String> {
    let quote = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    let yaml_list = |items: &[String]| {
        let quoted: Vec<_> = items.iter().map(|a| quote(a)).collect();
        format!("[{}]", quoted.join(", "))
    };
    let map_block = |pairs: Vec<(&String, &String)>, indent: &str| {
        pairs
            .iter()
            .map(|(k, v)| format!("\n{indent}{}: {}", k, quote(v)))
            .collect::<String>()
    };

    match server {
        McpServer::Stdio(stdio) => {
            let name = &stdio.name;
            let cmd = quote(&stdio.command.to_string_lossy());
            let args = yaml_list(&stdio.args);
            let envs = if stdio.env.is_empty() {
                String::new()
            } else {
                format!(
                    "\n    envs:{}",
                    map_block(
                        stdio.env.iter().map(|e| (&e.name, &e.value)).collect(),
                        "      "
                    )
                )
            };
            Some(formatdoc! {"
                {name}:
                    name: {name}
                    type: stdio
                    cmd: {cmd}
                    args: {args}
                    enabled: true{envs}
            "})
        }
        // Goose calls remote MCP `streamable_http`, and the endpoint is `uri`.
        McpServer::Http(http) => Some(goose_remote_yaml(
            &http.name,
            &http.url,
            http.headers.iter().map(|h| (&h.name, &h.value)).collect(),
        )),
        // No `sse` variant exists, and writing one as streamable HTTP yields an
        // entry that cannot connect. Caller reports it as unsupported.
        McpServer::Sse(_) => None,
        _ => None,
    }
}

/// Does `content` carry this extension with `enabled: false`? Scans the entry's
/// own block, so another extension's `enabled: false` is not mistaken for it.
fn goose_extension_disabled(content: &str, name: &str) -> bool {
    let needle = format!("{name}:");
    let mut indent = 0;
    let mut in_section = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let line_indent = line.len() - trimmed.len();
        if in_section && line_indent <= indent {
            break;
        }
        if in_section && trimmed == "enabled: false" {
            return true;
        }
        if !in_section && trimmed.starts_with(&needle) {
            indent = line_indent;
            in_section = true;
        }
    }
    false
}

fn goose_remote_yaml(name: &str, uri: &str, headers: Vec<(&String, &String)>) -> String {
    let quote = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    let uri = quote(uri);
    let headers_block = if headers.is_empty() {
        String::new()
    } else {
        let pairs: String = headers
            .iter()
            .map(|(k, v)| format!("\n      {}: {}", k, quote(v)))
            .collect();
        format!("\n    headers:{pairs}")
    };
    formatdoc! {"
        {name}:
            name: {name}
            type: streamable_http
            uri: {uri}
            enabled: true{headers_block}
    "}
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
        let snippet = match goose_extension_yaml(server) {
            Some(snippet) => snippet,
            None => {
                out.info(format!("{display}: skipping unsupported MCP server {name}"));
                continue;
            }
        };

        // Keep a user's `enabled: false`; sync runs per hook event.
        let snippet = if goose_extension_disabled(&content, name) {
            snippet.replace("enabled: true", "enabled: false")
        } else {
            snippet
        };

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
    register_json_mcp_servers_with(path, servers, Some("mcp"), opencode_server_to_json, out)
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
            McpServerStdio::new("symposium", "/usr/local/bin/cargo-agents")
                .args(vec!["mcp".into()]),
        )]
    }

    fn test_server_names() -> Vec<&'static str> {
        vec!["symposium"]
    }

    /// The two shapes that were dropped or mis-serialized: env, and headers.
    fn env_and_remote_servers() -> Vec<McpServer> {
        use sacp::schema::{EnvVariable, HttpHeader, McpServerHttp};
        vec![
            McpServer::Stdio(
                McpServerStdio::new("withenv", "/bin/server")
                    .env(vec![EnvVariable::new("TOKEN", "abc")]),
            ),
            McpServer::Http(
                McpServerHttp::new("remote", "http://localhost:8080/mcp")
                    .headers(vec![HttpHeader::new("Authorization", "Bearer t")]),
            ),
        ]
    }

    /// A pair-list `env` is skipped or rejected by every client, silently when
    /// the entry differs from a working one only by carrying env.
    #[test]
    fn env_and_headers_are_objects_not_pair_lists() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.json");
        register_claude_mcp_servers(&path, &env_and_remote_servers(), &Output::quiet()).unwrap();

        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(config["mcpServers"]["withenv"]["env"]["TOKEN"], "abc");
        assert_eq!(
            config["mcpServers"]["remote"]["headers"]["Authorization"],
            "Bearer t"
        );
    }

    /// The Copilot CLI needs an explicit `type`; without it a remote entry is
    /// not picked up at all.
    #[test]
    fn copilot_entries_carry_a_type() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("mcp-config.json");
        register_copilot_mcp_servers(&path, &env_and_remote_servers(), &Output::quiet()).unwrap();

        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(config["mcpServers"]["withenv"]["type"], "local");
        assert_eq!(config["mcpServers"]["remote"]["type"], "http");
        assert_eq!(config["mcpServers"]["withenv"]["env"]["TOKEN"], "abc");
    }

    /// Codex takes env as a TOML table and a remote server as a bare `url`,
    /// matching `codex mcp add --env` / `--url`.
    #[test]
    fn codex_writes_env_table_and_remote_url() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        register_codex_mcp_servers(&path, &env_and_remote_servers(), &Output::quiet()).unwrap();

        let written = fs::read_to_string(&path).unwrap();
        let doc: toml::Table = toml::from_str(&written).unwrap();
        let servers = doc["mcp_servers"].as_table().unwrap();
        assert_eq!(
            servers["withenv"]["env"]["TOKEN"].as_str(),
            Some("abc"),
            "got: {written}"
        );
        assert_eq!(
            servers["remote"]["url"].as_str(),
            Some("http://localhost:8080/mcp"),
            "got: {written}"
        );
    }

    /// The comparison must see the `env` sub-table, or a rotated token stays
    /// stale forever.
    #[test]
    fn codex_updates_a_changed_env_value() {
        use sacp::schema::EnvVariable;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        let with = |value: &str| {
            vec![McpServer::Stdio(
                McpServerStdio::new("s", "/bin/server").env(vec![EnvVariable::new("TOKEN", value)]),
            )]
        };
        register_codex_mcp_servers(&path, &with("old"), &Output::quiet()).unwrap();
        register_codex_mcp_servers(&path, &with("new"), &Output::quiet()).unwrap();

        let written = fs::read_to_string(&path).unwrap();
        let doc: toml::Table = toml::from_str(&written).unwrap();
        assert_eq!(
            doc["mcp_servers"]["s"]["env"]["TOKEN"].as_str(),
            Some("new"),
            "got: {written}"
        );
    }

    /// `enabled: false` is how a user turns one server off for the agents whose
    /// schema has the field. Sync runs per hook event, so re-asserting `true`
    /// would make the choice impossible to keep.
    #[test]
    fn a_user_disabled_server_stays_disabled() {
        let tmp = tempfile::tempdir().unwrap();

        let opencode = tmp.path().join("opencode.json");
        save_json(
            &opencode,
            &json!({"mcp": {"symposium": {"type": "local", "command": ["/old"], "enabled": false}}}),
        )
        .unwrap();
        register_opencode_mcp_servers(&opencode, &test_servers(), &Output::quiet()).unwrap();
        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&opencode).unwrap()).unwrap();
        assert_eq!(
            config["mcp"]["symposium"]["enabled"], false,
            "got: {config:#}"
        );
        // The rest of the entry is still brought up to date.
        assert_eq!(
            config["mcp"]["symposium"]["command"][0],
            "/usr/local/bin/cargo-agents"
        );

        let goose = tmp.path().join("config.yaml");
        fs::write(
            &goose,
            "extensions:\n  symposium:\n    name: symposium\n    type: stdio\n    cmd: \"/old\"\n    args: []\n    enabled: false\n",
        )
        .unwrap();
        register_goose_mcp_servers(&goose, &test_servers(), &Output::quiet()).unwrap();
        let content = fs::read_to_string(&goose).unwrap();
        let doc: serde_yaml_ng::Value = serde_yaml_ng::from_str(&content).unwrap();
        assert_eq!(
            doc["extensions"]["symposium"]["enabled"].as_bool(),
            Some(false),
            "got: {content}"
        );
        assert_eq!(
            doc["extensions"]["symposium"]["cmd"].as_str(),
            Some("/usr/local/bin/cargo-agents"),
            "got: {content}"
        );
    }

    /// An SSE endpoint written as streamable HTTP could never connect, so it is
    /// reported as unsupported instead of registered wrong.
    #[test]
    fn sse_servers_are_skipped_where_the_transport_does_not_exist() {
        use sacp::schema::McpServerSse;
        let sse = vec![McpServer::Sse(McpServerSse::new(
            "streamy",
            "http://localhost:8080/sse",
        ))];
        let tmp = tempfile::tempdir().unwrap();

        let goose = tmp.path().join("config.yaml");
        register_goose_mcp_servers(&goose, &sse, &Output::quiet()).unwrap();
        let goose_content = fs::read_to_string(&goose).unwrap_or_default();
        assert!(
            !goose_content.contains("streamable_http"),
            "got: {goose_content}"
        );

        let codex = tmp.path().join("config.toml");
        register_codex_mcp_servers(&codex, &sse, &Output::quiet()).unwrap();
        let codex_content = fs::read_to_string(&codex).unwrap_or_default();
        assert!(!codex_content.contains("streamy"), "got: {codex_content}");
    }

    /// Registering the same Goose entry twice must leave the file byte-identical:
    /// hook auto-sync runs this per event, and the update path rewrites the
    /// user's whole config.yaml.
    #[test]
    fn goose_registration_leaves_the_file_untouched_when_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.yaml");
        register_goose_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();
        let first = fs::read_to_string(&path).unwrap();
        register_goose_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();
        assert_eq!(first, fs::read_to_string(&path).unwrap());
    }

    /// Re-registering an unchanged entry must not rewrite the file, including
    /// for the fields the comparison used to ignore.
    #[test]
    fn codex_registration_is_idempotent_for_env_and_url() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        register_codex_mcp_servers(&path, &env_and_remote_servers(), &Output::quiet()).unwrap();
        let first = fs::read_to_string(&path).unwrap();
        register_codex_mcp_servers(&path, &env_and_remote_servers(), &Output::quiet()).unwrap();
        assert_eq!(first, fs::read_to_string(&path).unwrap());
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
            "/usr/local/bin/cargo-agents"
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
            "/usr/local/bin/cargo-agents"
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
            "/usr/local/bin/cargo-agents"
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
            "/usr/local/bin/cargo-agents"
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
            "/usr/local/bin/cargo-agents"
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
        // The `mcpServers` wrapper is required: bare entries make the CLI
        // reject the entire file.
        assert_eq!(
            config["mcpServers"]["symposium"]["command"],
            "/usr/local/bin/cargo-agents"
        );
        assert_eq!(config["mcpServers"]["symposium"]["args"][0], "mcp");
    }

    #[test]
    fn register_copilot_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("mcp.json");
        register_copilot_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();
        register_copilot_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();

        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(config["mcpServers"].as_object().unwrap().len(), 1);
    }

    #[test]
    fn register_copilot_updates_stale() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("mcp.json");
        let stale = json!({"mcpServers": {"symposium": {"command": "/old/path", "args": ["mcp"]}}});
        save_json(&path, &stale).unwrap();

        register_copilot_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();

        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            config["mcpServers"]["symposium"]["command"],
            "/usr/local/bin/cargo-agents"
        );
    }

    #[test]
    fn unregister_copilot_removes_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("mcp.json");
        register_copilot_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();
        unregister_copilot_mcp_servers(&path, &test_server_names(), &Output::quiet()).unwrap();

        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(config["mcpServers"].get("symposium").is_none());
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
        // Goose's schema: a `type` discriminant and `cmd`, not a nested
        // `provider`/`config` pair - which it rejects with `missing field type`.
        assert_eq!(ext["type"].as_str().unwrap(), "stdio");
        assert_eq!(ext["name"].as_str().unwrap(), "symposium");
        assert_eq!(ext["cmd"].as_str().unwrap(), "/usr/local/bin/cargo-agents");
        assert_eq!(ext["args"][0].as_str().unwrap(), "mcp");
        assert_eq!(ext["enabled"].as_bool().unwrap(), true);
    }

    /// Env vars were dropped entirely before, so a server needing them got
    /// none - silently.
    #[test]
    fn register_goose_writes_envs_and_remote() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.yaml");
        register_goose_mcp_servers(&path, &env_and_remote_servers(), &Output::quiet()).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let doc: serde_yaml_ng::Value = serde_yaml_ng::from_str(&content).unwrap();
        assert_eq!(
            doc["extensions"]["withenv"]["envs"]["TOKEN"]
                .as_str()
                .unwrap(),
            "abc",
            "got: {content}"
        );
        // Goose calls remote MCP `streamable_http` and the endpoint `uri`;
        // there is no `sse` variant.
        let remote = &doc["extensions"]["remote"];
        assert_eq!(remote["type"].as_str().unwrap(), "streamable_http");
        assert_eq!(
            remote["uri"].as_str().unwrap(),
            "http://localhost:8080/mcp",
            "got: {content}"
        );
        assert_eq!(
            remote["headers"]["Authorization"].as_str().unwrap(),
            "Bearer t"
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
        // The pre-fix shape, which is what an upgrading user has on disk: it
        // has to be replaced, not merged with.
        fs::write(&path, "extensions:\n  symposium:\n    provider: mcp\n    config:\n      command: \"/old/path\"\n      args: [\"mcp\"]\n").unwrap();

        register_goose_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let doc: serde_yaml_ng::Value = serde_yaml_ng::from_str(&content).unwrap();
        assert_eq!(
            doc["extensions"]["symposium"]["cmd"].as_str().unwrap(),
            "/usr/local/bin/cargo-agents",
        );
        assert!(
            doc["extensions"]["symposium"]["provider"].is_null(),
            "the rejected shape must not survive: {content}"
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
            doc["extensions"]["test-server"]["cmd"].as_str().unwrap(),
            "/path with spaces/symposium",
        );
        assert_eq!(
            doc["extensions"]["test-server"]["args"][0].as_str().unwrap(),
            "--flag:value",
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
        // OpenCode's own shape: a `type`, and the command as one array rather
        // than a command plus separate `args`. Anything else and it rejects
        // the whole config file.
        let entry = &config["mcp"]["symposium"];
        assert_eq!(entry["type"], "local");
        assert_eq!(entry["command"][0], "/usr/local/bin/cargo-agents");
        assert_eq!(entry["command"][1], "mcp");
        assert_eq!(entry["enabled"], true);
        assert!(entry.get("args").is_none(), "got: {entry}");
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
        // The stale entry is in the pre-fix shape, which is exactly what an
        // upgrading user has on disk: it must be rewritten, not left alone.
        let stale = json!({"mcp": {"symposium": {"command": "/old/path", "args": ["mcp"]}}});
        save_json(&path, &stale).unwrap();

        register_opencode_mcp_servers(&path, &test_servers(), &Output::quiet()).unwrap();

        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            config["mcp"]["symposium"]["command"][0],
            "/usr/local/bin/cargo-agents"
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
