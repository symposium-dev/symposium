use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::pm::layout::{AGENT_PLUGIN_FILE, MANIFEST_FILE, SKILL_FILE};

const SCHEMA: &str = "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json";
const CLAUDE_MANIFEST: &str = ".claude-plugin/plugin.json";
const CLAUDE_ROOT_VAR: &str = "CLAUDE_PLUGIN_ROOT";
const MAX_NAME_LEN: usize = 64;
const SKILLS: &str = "skills";
const COMMANDS: &str = "commands";
const HOOKS_FILE: &str = "hooks/hooks.json";
const MCP_FILE: &str = ".mcp.json";
const NOT_COPIED: &[&str] = &[
    ".claude-plugin",
    ".git",
    COMMANDS,
    "agents",
    MCP_FILE,
    ".lsp.json",
];
const HOOK_EVENTS: &[&str] = &[
    "PreToolUse",
    "PostToolUse",
    "UserPromptSubmit",
    "SessionStart",
    "Stop",
];
const CARRIED_FIELDS: &[&str] = &[
    "name",
    "version",
    "description",
    "author",
    "homepage",
    "repository",
    "license",
    "keywords",
];

#[derive(Serialize)]
struct PackageJson {
    #[serde(rename = "$schema")]
    schema: &'static str,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    author: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    homepage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    repository: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    keywords: Option<Vec<String>>,
}

pub fn import_claude_plugin(source: &Path, out: &Path) -> Result<PathBuf> {
    let manifest = read_claude_manifest(source)?;
    let dir_name = fs::canonicalize(source)?
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string();
    let original = match manifest.get("name") {
        Some(Value::String(name)) => name.clone(),
        _ => dir_name,
    };
    let name = normalize_name(&original, true);
    if name.is_empty() {
        bail!("cannot derive a package name from `{original}`");
    }
    if name != original {
        report(format!("name `{original}` normalized to `{name}`"));
    }

    let dest = out.join(&name);
    if fs::symlink_metadata(&dest).is_ok() {
        bail!("`{}` already exists", dest.display());
    }
    fs::create_dir_all(&dest).with_context(|| format!("create {}", dest.display()))?;

    let package = package_json(name, &manifest);
    report_unimported(source, &manifest);
    copy_contents(source, &dest)?;
    import_commands(source, &dest)?;
    let hooks = import_hooks(source)?;
    let mcp_servers = import_mcp_servers(source)?;
    report_root_references(&dest);

    fs::write(
        dest.join(AGENT_PLUGIN_FILE),
        serde_json::to_string_pretty(&package)? + "\n",
    )?;

    if !hooks.is_empty() || !mcp_servers.is_empty() {
        let mut toml = String::new();
        if dest.join(SKILLS).is_dir() {
            toml.push_str("[[skills]]\nsource.path = \"skills\"\n");
        }
        for section in hooks.iter().chain(&mcp_servers) {
            if !toml.is_empty() {
                toml.push('\n');
            }
            toml.push_str(section);
        }
        fs::write(dest.join(MANIFEST_FILE), toml)?;
    }
    Ok(dest)
}

fn report(message: String) {
    tracing::warn!(report = %crate::report::ReportEvent::Warning { message });
}

fn read_claude_manifest(source: &Path) -> Result<Map<String, Value>> {
    let path = source.join(CLAUDE_MANIFEST);
    if !path.exists() {
        if !source.is_dir() {
            bail!("`{}` is not a directory", source.display());
        }
        return Ok(Map::new());
    }
    let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    match serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))? {
        Value::Object(fields) => Ok(fields),
        _ => bail!("`{}` must hold a JSON object", path.display()),
    }
}

fn normalize_name(name: &str, allow_dot: bool) -> String {
    let mut normalized = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        let c = match c {
            'a'..='z' | '0'..='9' => c,
            '.' if allow_dot => '.',
            _ => '-',
        };
        if (c == '-' || c == '.') && normalized.ends_with(c) {
            continue;
        }
        normalized.push(c);
    }
    normalized.truncate(MAX_NAME_LEN);
    normalized
        .trim_matches(|c: char| !c.is_ascii_alphanumeric())
        .to_string()
}

fn package_json(name: String, manifest: &Map<String, Value>) -> PackageJson {
    let string = |key: &str| match manifest.get(key) {
        Some(Value::String(value)) => Some(value.clone()),
        Some(_) => {
            report(format!("`{key}` not imported: it is not a string"));
            None
        }
        None => None,
    };
    let author = match manifest.get("author") {
        Some(Value::Object(fields)) => Some(
            fields
                .iter()
                .filter(|(key, value)| {
                    let kept =
                        matches!(key.as_str(), "name" | "email" | "url") && value.is_string();
                    if !kept {
                        report(format!("`author.{key}` not imported"));
                    }
                    kept
                })
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        ),
        Some(_) => {
            report("`author` not imported: it is not an object".to_string());
            None
        }
        None => None,
    };
    let keywords = match manifest.get("keywords") {
        Some(Value::Array(items)) if items.iter().all(Value::is_string) => Some(
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect(),
        ),
        Some(_) => {
            report("`keywords` not imported: it is not an array of strings".to_string());
            None
        }
        None => None,
    };
    PackageJson {
        schema: SCHEMA,
        name,
        version: string("version"),
        description: string("description"),
        author,
        homepage: string("homepage"),
        repository: string("repository"),
        license: string("license"),
        keywords,
    }
}

fn copy_contents(source: &Path, dest: &Path) -> Result<()> {
    let dest_resolved = fs::canonicalize(dest)?;
    let mut entries: Vec<_> = fs::read_dir(source)?.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let file_name = entry.file_name();
        if NOT_COPIED.iter().any(|skipped| file_name == *skipped)
            || fs::canonicalize(entry.path()).is_ok_and(|path| path == dest_resolved)
        {
            continue;
        }
        let from = entry.path();
        let to = dest.join(&file_name);
        if from.is_dir() {
            crate::sync::copy_dir_recursive(&from, &to, None)?;
        } else {
            fs::copy(&from, &to).with_context(|| format!("copy {}", from.display()))?;
        }
    }
    let hooks_json = dest.join(HOOKS_FILE);
    if hooks_json.exists() {
        fs::remove_file(&hooks_json)?;
        if let Some(hooks_dir) = hooks_json.parent() {
            fs::remove_dir(hooks_dir).ok();
        }
    }
    Ok(())
}

fn import_commands(source: &Path, dest: &Path) -> Result<()> {
    let dir = source.join(COMMANDS);
    if !dir.is_dir() {
        return Ok(());
    }
    let mut entries: Vec<_> = fs::read_dir(&dir)?.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        let file_name = path.file_name().unwrap_or_default().to_string_lossy();
        let Some(stem) = file_name.strip_suffix(".md").filter(|_| path.is_file()) else {
            report(format!(
                "`{COMMANDS}/{file_name}` not imported: only `{COMMANDS}/*.md` become skills"
            ));
            continue;
        };
        let name = normalize_name(stem, false);
        let skill_dir = dest.join(SKILLS).join(&name);
        if name.is_empty() || skill_dir.exists() {
            report(format!(
                "command `{stem}` not imported: a skill named `{name}` already exists"
            ));
            continue;
        }
        let content = fs::read_to_string(&path)?;
        let Some(skill) = command_skill(&name, &content) else {
            report(format!(
                "command `{stem}` not imported: its frontmatter has no `description`"
            ));
            continue;
        };
        if name != stem {
            report(format!("command `{stem}` imported as skill `{name}`"));
        }
        fs::create_dir_all(&skill_dir)?;
        fs::write(skill_dir.join(SKILL_FILE), skill)?;
    }
    Ok(())
}

fn command_skill(name: &str, content: &str) -> Option<String> {
    let rest = content.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    let (frontmatter, body) = rest.split_at(end);
    let fields: Vec<&str> = frontmatter
        .lines()
        .filter(|line| !line.starts_with("name:"))
        .collect();
    if !fields.iter().any(|line| line.starts_with("description:")) {
        return None;
    }
    Some(format!("---\nname: {name}\n{}{body}", fields.join("\n")))
}

fn import_hooks(source: &Path) -> Result<Vec<String>> {
    let path = source.join(HOOKS_FILE);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let json: Value = serde_json::from_str(&fs::read_to_string(&path)?)
        .with_context(|| format!("parse {}", path.display()))?;
    let Some(Value::Object(events)) = json.get("hooks") else {
        bail!("`{HOOKS_FILE}` has no `hooks` object");
    };
    let mut sections = Vec::new();
    for (event, groups) in events {
        if !HOOK_EVENTS.contains(&event.as_str()) {
            report(format!(
                "`{event}` hooks not imported: Symposium has no such event"
            ));
            continue;
        }
        for group in groups.as_array().into_iter().flatten() {
            let matcher = group.get("matcher").and_then(Value::as_str);
            for hook in group
                .get("hooks")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(section) = import_hook(event, matcher, hook, sections.len() + 1) {
                    sections.push(section);
                }
            }
        }
    }
    Ok(sections)
}

fn import_hook(event: &str, matcher: Option<&str>, hook: &Value, index: usize) -> Option<String> {
    let command = hook.get("command").and_then(Value::as_str);
    let kind = hook.get("type").and_then(Value::as_str);
    let (Some("command"), Some(command)) = (kind, command) else {
        report(format!(
            "`{event}` hook not imported: only `type: command` hooks are supported"
        ));
        return None;
    };
    let unsupported: Vec<&str> = hook
        .as_object()?
        .keys()
        .map(String::as_str)
        .filter(|key| !matches!(*key, "type" | "command" | "timeout"))
        .collect();
    if !unsupported.is_empty() {
        report(format!(
            "`{event}` hook `{command}` not imported: Symposium does not support `{}`",
            unsupported.join("`, `")
        ));
        return None;
    }
    if hook.get("timeout").is_some() {
        report(format!("`{event}` hook `{command}`: `timeout` dropped"));
    }
    let mut section = format!(
        "[[hooks]]\nname = {}\nevent = {}\nformat = \"claude\"\n",
        quote(&format!("{}-{index}", kebab_case(event))),
        quote(event),
    );
    if let Some(matcher) = matcher.filter(|m| !m.is_empty()) {
        section.push_str(&format!("matcher = {}\n", quote(matcher)));
    }
    section.push_str(&format!(
        "command = {{ executable = \"sh\" }}\nargs = [\"-c\", {}]\n",
        quote(command)
    ));
    Some(section)
}

fn import_mcp_servers(source: &Path) -> Result<Vec<String>> {
    let path = source.join(MCP_FILE);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let json: Value = serde_json::from_str(&fs::read_to_string(&path)?)
        .with_context(|| format!("parse {}", path.display()))?;
    let servers = json.get("mcpServers").unwrap_or(&json);
    let Value::Object(servers) = servers else {
        bail!("`{MCP_FILE}` must hold an object of servers");
    };
    Ok(servers
        .iter()
        .filter_map(|(name, server)| match mcp_server(name, server) {
            Ok(section) => Some(section),
            Err(reason) => {
                report(format!("MCP server `{name}` not imported: {reason}"));
                None
            }
        })
        .collect())
}

fn mcp_server(name: &str, server: &Value) -> Result<String, String> {
    let Value::Object(fields) = server else {
        return Err("it is not an object".to_string());
    };
    if server.to_string().contains(CLAUDE_ROOT_VAR) {
        return Err(format!(
            "it refers to `${{{CLAUDE_ROOT_VAR}}}`, which agents do not expand outside Claude Code plugins"
        ));
    }
    let kind = fields
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("stdio");
    let allowed: &[&str] = match kind {
        "stdio" => &["type", "command", "args", "env"],
        "http" | "sse" => &["type", "url", "headers"],
        other => return Err(format!("unsupported type `{other}`")),
    };
    if let Some(key) = fields.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(format!("Symposium does not support `{key}`"));
    }
    let string = |key: &str| {
        fields
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("`{key}` must be a string"))
    };
    let mut section = String::from("[[mcp_servers]]\n");
    if kind == "stdio" {
        let args = match fields.get("args") {
            None => Vec::new(),
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| item.as_str().ok_or("`args` must hold strings"))
                .collect::<Result<_, _>>()?,
            Some(_) => return Err("`args` must be an array".to_string()),
        };
        section.push_str(&format!(
            "name = {}\ncommand = {}\nargs = [{}]\nenv = {}\n",
            quote(name),
            quote(string("command")?),
            args.into_iter().map(quote).collect::<Vec<_>>().join(", "),
            pairs(fields.get("env"), "env")?,
        ));
    } else {
        section.push_str(&format!(
            "type = {}\nname = {}\nurl = {}\nheaders = {}\n",
            quote(kind),
            quote(name),
            quote(string("url")?),
            pairs(fields.get("headers"), "headers")?,
        ));
    }
    Ok(section)
}

fn pairs(value: Option<&Value>, key: &str) -> Result<String, String> {
    let Some(value) = value else {
        return Ok("[]".to_string());
    };
    let Value::Object(map) = value else {
        return Err(format!("`{key}` must be an object"));
    };
    let items = map
        .iter()
        .map(|(name, value)| {
            let value = value
                .as_str()
                .ok_or_else(|| format!("`{key}.{name}` must be a string"))?;
            Ok(format!(
                "{{ name = {}, value = {} }}",
                quote(name),
                quote(value)
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(format!("[{}]", items.join(", ")))
}

fn kebab_case(name: &str) -> String {
    let mut kebab = String::new();
    for c in name.chars() {
        if c.is_ascii_uppercase() && !kebab.is_empty() {
            kebab.push('-');
        }
        kebab.push(c.to_ascii_lowercase());
    }
    kebab
}

fn quote(value: &str) -> String {
    toml::Value::String(value.to_string()).to_string()
}

fn report_unimported(source: &Path, manifest: &Map<String, Value>) {
    for key in manifest.keys() {
        if !CARRIED_FIELDS.contains(&key.as_str()) {
            report(format!("`{key}` in `{CLAUDE_MANIFEST}` not imported"));
        }
    }
    for (entry, reason) in [
        ("agents", "Agent Plugins has no agents"),
        (".lsp.json", "Agent Plugins has no LSP servers"),
    ] {
        if source.join(entry).exists() {
            report(format!("`{entry}` not imported: {reason}"));
        }
    }
}

fn report_root_references(dest: &Path) {
    let Ok(entries) = fs::read_dir(dest.join(SKILLS)) else {
        return;
    };
    let mut names: Vec<_> = entries
        .flatten()
        .filter(|entry| {
            fs::read_to_string(entry.path().join(SKILL_FILE))
                .is_ok_and(|content| content.contains(CLAUDE_ROOT_VAR))
        })
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    for name in names {
        report(format!(
            "skill `{name}` refers to `${{{CLAUDE_ROOT_VAR}}}`, which only Claude Code fills in for its own plugins"
        ));
    }
}
