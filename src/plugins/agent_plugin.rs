use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use sacp::schema::{
    EnvVariable, HttpHeader, McpServer, McpServerHttp, McpServerSse, McpServerStdio,
};
use serde_json::{Map, Value};
use symposium_sdk::manifest::{RawPluginManifest, RawPluginMcpServer};
use symposium_sdk::predicate::{DependsOnList, PredicateSet};

use crate::pm::layout::{AGENT_PLUGIN_FILE, SKILL_FILE};

const SCHEMA_VERSIONS: &[&str] = &["1.0.0", "1.1.0"];
const PLUGIN_SCHEMA_FILE: &str = "plugin.schema.json";
const MCP_SCHEMA_FILE: &str = "mcp.schema.json";
const PLUGIN_ROOT_VAR: &str = "PLUGIN_ROOT";
const PLUGIN_DATA_VAR: &str = "PLUGIN_DATA";
const SYMPOSIUM_NAMESPACE: &str = "dev.symposium";
const SKILLS_DIR: &str = "skills";
const MCP_FILE: &str = "mcp.json";
const MAX_NAME_LEN: usize = 64;

struct AgentPluginJson {
    spec: &'static str,
    name: String,
    version: Option<String>,
    description: Option<String>,
    gate: SymposiumGate,
}

#[derive(Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SymposiumGate {
    #[serde(default, rename = "depends-on")]
    depends_on: DependsOnList,
    #[serde(default)]
    predicates: PredicateSet,
}

pub(crate) fn agent_plugin_manifest(dir: &Path) -> Result<RawPluginManifest> {
    let (manifest, notices) = read_package(dir)?;
    for notice in notices {
        report(dir, &notice);
    }
    Ok(manifest)
}

pub(crate) fn fill_from_agent_plugin(manifest: &mut RawPluginManifest, dir: &Path) {
    if let Some(notice) = fill(manifest, dir) {
        report(dir, &notice);
    }
}

fn report(dir: &Path, notice: &str) {
    tracing::warn!(
        report = %crate::report::ReportEvent::Warning {
            message: format!(
                "{}: {notice}",
                crate::output::display_path(&dir.components().collect::<PathBuf>())
            ),
        },
    );
}

fn read_package(dir: &Path) -> Result<(RawPluginManifest, Vec<String>)> {
    let root = fs::canonicalize(dir)?;
    let mut notices = Vec::new();
    let json = read_agent_plugin_json(&root, &mut notices)?;
    let mut manifest = RawPluginManifest {
        name: Some(json.name),
        version: json.version,
        description: json.description,
        depends_on: json.gate.depends_on,
        predicates: json.gate.predicates,
        ..Default::default()
    };
    for child in skill_children(&root, &mut notices) {
        manifest.push_skill_group(&format!("{SKILLS_DIR}/{child}"), PredicateSet::default());
    }
    manifest.mcp_servers = mcp_servers(&root, json.spec, &mut notices);
    if root.join(SKILL_FILE).is_file() {
        notices.push(format!(
            "`{SKILL_FILE}` at the package root is not one of its skills; move it to `{SKILLS_DIR}/<name>/{SKILL_FILE}`"
        ));
    }
    Ok((manifest, notices))
}

fn fill(manifest: &mut RawPluginManifest, dir: &Path) -> Option<String> {
    if !dir.join(AGENT_PLUGIN_FILE).is_file() {
        return None;
    }
    let read = fs::canonicalize(dir)
        .map_err(anyhow::Error::from)
        .and_then(|root| read_agent_plugin_json(&root, &mut Vec::new()));
    match read {
        Ok(json) => {
            manifest.name = manifest.name.take().or(Some(json.name));
            manifest.version = manifest.version.take().or(json.version);
            manifest.description = manifest.description.take().or(json.description);
            None
        }
        Err(e) => Some(format!(
            "`{AGENT_PLUGIN_FILE}` ignored beside the symposium manifest: {e:#}"
        )),
    }
}

fn read_agent_plugin_json(root: &Path, notices: &mut Vec<String>) -> Result<AgentPluginJson> {
    let path = root.join(AGENT_PLUGIN_FILE);
    if !resolves_inside(root, &path) {
        bail!("`{AGENT_PLUGIN_FILE}` resolves outside the package");
    }
    let text = fs::read_to_string(&path)?;
    let Value::Object(fields) = serde_json::from_str(&text)
        .with_context(|| format!("`{AGENT_PLUGIN_FILE}` is not valid JSON"))?
    else {
        bail!("`{AGENT_PLUGIN_FILE}` must hold a JSON object");
    };

    let spec = match fields.get("$schema") {
        None => bail!("missing required field `$schema`"),
        Some(Value::String(schema)) => schema_version(schema, PLUGIN_SCHEMA_FILE)?,
        Some(_) => bail!("`$schema` must be a string"),
    };

    let mut name = None;
    let mut version = None;
    let mut description = None;
    let mut gate = SymposiumGate::default();
    for (key, value) in &fields {
        match key.as_str() {
            "$schema" => {}
            "name" => name = Some(string_field(key, value)?),
            "version" => version = Some(string_field(key, value)?),
            "description" => description = Some(string_field(key, value)?),
            "homepage" | "repository" | "license" => {
                string_field(key, value)?;
            }
            "author" => check_author(value)?,
            "keywords" => check_keywords(value)?,
            "extensions" => match value {
                Value::Object(namespaces) => {
                    if let Some(extension) = namespaces.get(SYMPOSIUM_NAMESPACE) {
                        gate = serde_json::from_value(extension.clone()).with_context(|| {
                            format!("invalid `extensions.{SYMPOSIUM_NAMESPACE}`")
                        })?;
                    }
                }
                _ => notices.push("`extensions` is not an object; ignored".to_string()),
            },
            _ => notices.push(format!("unknown field `{key}` ignored")),
        }
    }
    let name = name.context("missing required field `name`")?;
    check_name(&name)?;
    Ok(AgentPluginJson {
        spec,
        name,
        version,
        description,
        gate,
    })
}

fn schema_version(schema: &str, file: &str) -> Result<&'static str> {
    SCHEMA_VERSIONS
        .iter()
        .copied()
        .find(|version| schema == format!("https://agent-plugins.org/schemas/{version}/{file}"))
        .with_context(|| {
            format!(
                "unsupported `$schema` `{schema}`; supported: Agent Plugins {}",
                SCHEMA_VERSIONS.join(" and ")
            )
        })
}

fn string_field(key: &str, value: &Value) -> Result<String> {
    value
        .as_str()
        .map(str::to_string)
        .with_context(|| format!("`{key}` must be a string"))
}

fn check_author(value: &Value) -> Result<()> {
    let Value::Object(fields) = value else {
        bail!("`author` must be an object");
    };
    for (key, value) in fields {
        if !matches!(key.as_str(), "name" | "email" | "url") {
            bail!("`author` allows only `name`, `email` and `url`, not `{key}`");
        }
        string_field(&format!("author.{key}"), value)?;
    }
    Ok(())
}

fn check_keywords(value: &Value) -> Result<()> {
    match value {
        Value::Array(items) if items.iter().all(Value::is_string) => Ok(()),
        _ => bail!("`keywords` must be an array of strings"),
    }
}

fn check_name(name: &str) -> Result<()> {
    let alphanumeric = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    let valid = (1..=MAX_NAME_LEN).contains(&name.len())
        && name
            .chars()
            .all(|c| alphanumeric(c) || c == '-' || c == '.')
        && name.starts_with(alphanumeric)
        && name.ends_with(alphanumeric)
        && !name.contains("--")
        && !name.contains("..");
    if !valid {
        bail!(
            "invalid `name` `{name}`: use 1 to {MAX_NAME_LEN} lowercase letters, digits, `-` and `.`, \
             starting and ending with a letter or digit, with no `--` or `..`"
        );
    }
    Ok(())
}

fn skill_children(root: &Path, notices: &mut Vec<String>) -> Vec<String> {
    let skills = root.join(SKILLS_DIR);
    if fs::symlink_metadata(&skills).is_err() {
        return Vec::new();
    }
    if !skills.is_dir() {
        notices.push(format!(
            "`{SKILLS_DIR}` is not a directory; no skills loaded"
        ));
        return Vec::new();
    }
    if !resolves_inside(root, &skills) {
        notices.push(format!(
            "`{SKILLS_DIR}` resolves outside the package; no skills loaded"
        ));
        return Vec::new();
    }
    let entries = match fs::read_dir(&skills) {
        Ok(entries) => entries,
        Err(e) => {
            notices.push(format!("cannot read `{SKILLS_DIR}`: {e}; no skills loaded"));
            return Vec::new();
        }
    };
    let mut children: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    children.sort();
    children.retain(|child| {
        let skill_md = skills.join(child).join(SKILL_FILE);
        if !skill_md.is_file() {
            return false;
        }
        if !resolves_inside(root, &skill_md) {
            notices.push(format!(
                "skill `{child}` skipped: its `{SKILL_FILE}` resolves outside the package"
            ));
            return false;
        }
        true
    });
    children
}

fn mcp_servers(root: &Path, spec: &str, notices: &mut Vec<String>) -> Vec<RawPluginMcpServer> {
    let path = root.join(MCP_FILE);
    if fs::symlink_metadata(&path).is_err() {
        return Vec::new();
    }
    let servers = match read_mcp_json(root, &path, spec) {
        Ok(servers) => servers,
        Err(e) => {
            notices.push(format!("no MCP servers loaded: {e:#}"));
            return Vec::new();
        }
    };
    servers
        .into_iter()
        .filter_map(|(name, value)| match mcp_server(root, &name, &value) {
            Ok(server) => Some(RawPluginMcpServer {
                depends_on: None,
                crates: None,
                predicates: PredicateSet::default(),
                server,
            }),
            Err(e) => {
                notices.push(format!("MCP server `{name}` skipped: {e:#}"));
                None
            }
        })
        .collect()
}

fn read_mcp_json(root: &Path, path: &Path, spec: &str) -> Result<Map<String, Value>> {
    if !path.is_file() {
        bail!("`{MCP_FILE}` is not a file");
    }
    if !resolves_inside(root, path) {
        bail!("`{MCP_FILE}` resolves outside the package");
    }
    let Value::Object(mut fields) = serde_json::from_str(&fs::read_to_string(path)?)
        .with_context(|| format!("`{MCP_FILE}` is not valid JSON"))?
    else {
        bail!("`{MCP_FILE}` must hold a JSON object");
    };
    let version = match fields.remove("$schema") {
        None => bail!("`{MCP_FILE}` misses required field `$schema`"),
        Some(Value::String(schema)) => schema_version(&schema, MCP_SCHEMA_FILE)?,
        Some(_) => bail!("`{MCP_FILE}` `$schema` must be a string"),
    };
    if version != spec {
        bail!(
            "`{MCP_FILE}` targets Agent Plugins {version} but `{AGENT_PLUGIN_FILE}` targets {spec}"
        );
    }
    let servers = match fields.remove("mcpServers") {
        None => bail!("`{MCP_FILE}` misses required field `mcpServers`"),
        Some(Value::Object(servers)) => servers,
        Some(_) => bail!("`mcpServers` must be an object"),
    };
    if let Some(key) = fields.keys().next() {
        bail!("`{MCP_FILE}` allows only `$schema` and `mcpServers`, not `{key}`");
    }
    Ok(servers)
}

fn mcp_server(root: &Path, name: &str, value: &Value) -> Result<McpServer> {
    let Value::Object(fields) = value else {
        bail!("must be an object");
    };
    let transport = fields
        .get("type")
        .context("missing required field `type`")
        .and_then(|value| string_field("type", value))?;
    match transport.as_str() {
        "stdio" => {
            only_fields(fields, &["type", "command", "args", "env", "cwd"])?;
            if fields.contains_key("cwd") {
                bail!(
                    "`cwd` is not supported, since servers start in the agent's working directory"
                );
            }
            let command = required_string(fields, "command")?;
            let command = match command.strip_prefix("./") {
                Some(relative) => {
                    let path = root.join(Path::new(relative).components().collect::<PathBuf>());
                    if !path.exists() {
                        bail!("`command` `{command}` does not exist in the package");
                    }
                    if !resolves_inside(root, &path) {
                        bail!("`command` `{command}` resolves outside the package");
                    }
                    dunce::simplified(&path).to_path_buf()
                }
                None if command.is_empty() || command.contains(['/', '\\']) => {
                    bail!("`command` must be a bare executable name or a path starting with `./`")
                }
                None => PathBuf::from(command),
            };
            let args = match fields.get("args") {
                None => Vec::new(),
                Some(Value::Array(items)) => items
                    .iter()
                    .map(|item| string_field("args", item))
                    .collect::<Result<_>>()?,
                Some(_) => bail!("`args` must be an array of strings"),
            };
            let env = string_map(fields, "env")?;
            if let Some(reserved) = env
                .iter()
                .map(|(key, _)| key)
                .find(|key| [PLUGIN_ROOT_VAR, PLUGIN_DATA_VAR].contains(&key.as_str()))
            {
                bail!("`env` must not set `{reserved}`");
            }
            let env = env
                .into_iter()
                .map(|(key, value)| EnvVariable::new(key, value))
                .collect();
            Ok(McpServer::Stdio(
                McpServerStdio::new(name, command).args(args).env(env),
            ))
        }
        "streamable-http" | "sse" => {
            only_fields(fields, &["type", "url", "headers"])?;
            let url = required_string(fields, "url")?;
            check_url(&url)?;
            let headers = string_map(fields, "headers")?;
            let mut seen = std::collections::BTreeSet::new();
            if let Some((duplicate, _)) = headers
                .iter()
                .find(|(key, _)| !seen.insert(key.to_ascii_lowercase()))
            {
                bail!("`headers` names `{duplicate}` more than once");
            }
            let token = |c: char| c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c);
            if let Some((invalid, _)) = headers.iter().find(|(key, value)| {
                key.is_empty()
                    || !key.chars().all(token)
                    || value.chars().any(|c| c.is_control() && c != '\t')
            }) {
                bail!("`headers` entry `{invalid}` is not a valid HTTP header");
            }
            let headers = headers
                .into_iter()
                .map(|(key, value)| HttpHeader::new(key, value))
                .collect();
            Ok(if transport == "sse" {
                McpServer::Sse(McpServerSse::new(name, url).headers(headers))
            } else {
                McpServer::Http(McpServerHttp::new(name, url).headers(headers))
            })
        }
        other => bail!("unsupported transport `{other}`"),
    }
}

fn only_fields(fields: &Map<String, Value>, allowed: &[&str]) -> Result<()> {
    match fields.keys().find(|key| !allowed.contains(&key.as_str())) {
        Some(key) => bail!("field `{key}` is not allowed for this transport"),
        None => Ok(()),
    }
}

fn required_string(fields: &Map<String, Value>, key: &str) -> Result<String> {
    let value = fields
        .get(key)
        .with_context(|| format!("missing required field `{key}`"))?;
    string_field(key, value)
}

fn string_map(fields: &Map<String, Value>, key: &str) -> Result<Vec<(String, String)>> {
    match fields.get(key) {
        None => Ok(Vec::new()),
        Some(Value::Object(entries)) => entries
            .iter()
            .map(|(name, value)| Ok((name.clone(), string_field(&format!("{key}.{name}"), value)?)))
            .collect(),
        Some(_) => bail!("`{key}` must be an object of strings"),
    }
}

fn check_url(url: &str) -> Result<()> {
    let parsed = url::Url::parse(url).with_context(|| format!("`url` `{url}` is not a URL"))?;
    let loopback = match parsed.host() {
        Some(url::Host::Domain(host)) => host == "localhost",
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    };
    let scheme_ok = match parsed.scheme() {
        "https" => true,
        "http" => loopback,
        _ => false,
    };
    if !scheme_ok {
        bail!("`url` must use https, or http on a loopback host");
    }
    if !parsed.username().is_empty() || parsed.password().is_some() || parsed.fragment().is_some() {
        bail!("`url` must not carry user information or a fragment");
    }
    Ok(())
}

pub(crate) fn with_plugin_vars(mut server: McpServer, root: &Path, data: &Path) -> McpServer {
    if let McpServer::Stdio(stdio) = &mut server {
        let root = dunce::simplified(root).to_string_lossy();
        let data = dunce::simplified(data).to_string_lossy();
        for arg in &mut stdio.args {
            *arg = expand_plugin_vars(arg, &root, &data);
        }
        for var in &mut stdio.env {
            var.value = expand_plugin_vars(&var.value, &root, &data);
        }
        stdio.env.push(EnvVariable::new(PLUGIN_ROOT_VAR, root));
        stdio.env.push(EnvVariable::new(PLUGIN_DATA_VAR, data));
    }
    server
}

fn expand_plugin_vars(text: &str, root: &str, data: &str) -> String {
    let root_placeholder = format!("${{{PLUGIN_ROOT_VAR}}}");
    let data_placeholder = format!("${{{PLUGIN_DATA_VAR}}}");
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        if let Some(after) = tail.strip_prefix(&root_placeholder) {
            out.push_str(root);
            rest = after;
        } else if let Some(after) = tail.strip_prefix(&data_placeholder) {
            out.push_str(data);
            rest = after;
        } else {
            out.push_str("${");
            rest = &tail[2..];
        }
    }
    out.push_str(rest);
    out
}

pub(crate) fn resolves_inside(root: &Path, path: &Path) -> bool {
    fs::canonicalize(path).is_ok_and(|resolved| resolved.starts_with(root))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{File, instantiate_fixture};
    use symposium_sdk::manifest::RawPluginSource;

    const SKILL: &str = "---\nname: s\ndescription: d\n---\nBody.\n";

    const SCHEMA: &str = "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json";

    fn with_schema(fields: &str) -> String {
        format!(r#"{{"$schema": "{SCHEMA}", {fields}}}"#)
    }

    fn package(plugin_json: &str, files: &[File]) -> tempfile::TempDir {
        let tmp = instantiate_fixture(files);
        fs::write(tmp.path().join(AGENT_PLUGIN_FILE), plugin_json).unwrap();
        tmp
    }

    fn translate(plugin_json: &str) -> Result<(RawPluginManifest, Vec<String>)> {
        read_package(package(plugin_json, &[]).path())
    }

    fn group_paths(manifest: &RawPluginManifest) -> Vec<String> {
        manifest
            .skills
            .iter()
            .map(|group| match &group.source {
                Some(RawPluginSource::Table(table)) => {
                    table.path.as_ref().unwrap().to_string_lossy().into_owned()
                }
                other => panic!("unexpected source {other:?}"),
            })
            .collect()
    }

    #[test]
    fn maps_identity_and_accepts_any_metadata_of_the_right_type() {
        let (manifest, notices) = translate(&with_schema(r#""name": "minimal-plugin""#)).unwrap();
        assert_eq!(manifest.name.as_deref(), Some("minimal-plugin"));
        assert!(manifest.version.is_none() && manifest.description.is_none());
        assert!(manifest.skills.is_empty() && manifest.predicates.is_empty());
        assert!(notices.is_empty(), "{notices:?}");

        let (manifest, notices) = translate(&with_schema(
            r#""name": "plugin-name", "version": "not semver", "description": "Brief",
               "author": {"name": "A", "email": "not an email", "url": "not a url"},
               "homepage": "h", "repository": "r", "license": "not spdx", "keywords": ["k"],
               "extensions": {"com.example.client": {"setting": true}, "org.example.broken": 5}"#,
        ))
        .unwrap();
        assert_eq!(manifest.name.as_deref(), Some("plugin-name"));
        assert_eq!(manifest.version.as_deref(), Some("not semver"));
        assert_eq!(manifest.description.as_deref(), Some("Brief"));
        assert!(manifest.predicates.is_empty());
        assert!(notices.is_empty(), "{notices:?}");
    }

    #[test]
    fn rejects_a_manifest_that_breaks_the_schema() {
        let cases: [(String, &str); 11] = [
            ("{".into(), "is not valid JSON"),
            ("[]".into(), "must hold a JSON object"),
            (r#"{"name": "x"}"#.into(), "missing required field `$schema`"),
            (
                r#"{"$schema": "https://agent-plugins.org/schemas/1.2.0/plugin.schema.json", "name": "x"}"#.into(),
                "supported: Agent Plugins 1.0.0 and 1.1.0",
            ),
            (
                r#"{"$schema": "https://agent-plugins.org/schemas/1.1.0/mcp.schema.json", "name": "x"}"#.into(),
                "unsupported `$schema`",
            ),
            (r#"{"$schema": 1, "name": "x"}"#.into(), "`$schema` must be a string"),
            (with_schema(r#""version": "1.0.0""#), "missing required field `name`"),
            (with_schema(r#""name": "x", "description": null"#), "`description` must be a string"),
            (with_schema(r#""name": "x", "author": "me""#), "`author` must be an object"),
            (with_schema(r#""name": "x", "author": {"name": "a", "phone": "1"}"#), "not `phone`"),
            (with_schema(r#""name": "x", "keywords": "rust""#), "array of strings"),
        ];
        for (json, expected) in cases {
            let err = format!("{:#}", translate(&json).unwrap_err());
            assert!(err.contains(expected), "{json}: {err}");
        }
    }

    #[test]
    fn accepts_the_1_1_0_schema() {
        let json = r#"{"$schema": "https://agent-plugins.org/schemas/1.1.0/plugin.schema.json", "name": "x"}"#;
        let (manifest, notices) = translate(json).unwrap();
        assert_eq!(manifest.name.as_deref(), Some("x"));
        assert!(notices.is_empty(), "{notices:?}");
    }

    #[test]
    fn enforces_the_name_rule() {
        let longest = "a".repeat(MAX_NAME_LEN);
        for valid in ["acme.tools", "a", "a.-b", longest.as_str()] {
            let json = with_schema(&format!(r#""name": "{valid}""#));
            assert!(translate(&json).is_ok(), "{valid}");
        }
        let too_long = "a".repeat(MAX_NAME_LEN + 1);
        for invalid in [
            "",
            "My-Plugin",
            "-start",
            "end-",
            "has--double",
            "too.many..dots",
            "under_score",
            too_long.as_str(),
        ] {
            let json = with_schema(&format!(r#""name": "{invalid}""#));
            let err = translate(&json).unwrap_err().to_string();
            assert!(err.contains("invalid `name`"), "{invalid}: {err}");
        }
    }

    #[test]
    fn reports_and_ignores_unknown_fields_and_a_non_object_extensions() {
        let (manifest, notices) = translate(&with_schema(
            r#""name": "x", "commands": [], "extensions": []"#,
        ))
        .unwrap();
        assert_eq!(manifest.name.as_deref(), Some("x"));
        assert_eq!(
            notices,
            [
                "unknown field `commands` ignored",
                "`extensions` is not an object; ignored"
            ]
        );
    }

    #[test]
    fn reads_the_symposium_namespace_strictly() {
        let (manifest, _) = translate(&with_schema(
            r#""name": "x", "extensions": {"dev.symposium": {"depends-on": ["serde"], "predicates": ["env(CI)"]}}"#,
        ))
        .unwrap();
        assert_eq!(manifest.depends_on.0.len(), 1);
        assert_eq!(manifest.predicates.predicates.len(), 1);

        for invalid in [
            r#"{"depends": ["serde"]}"#,
            r#"{"predicates": ["not-a-call"]}"#,
            r#"{"depends-on": 5}"#,
            "true",
        ] {
            let json = with_schema(&format!(
                r#""name": "x", "extensions": {{"dev.symposium": {invalid}}}"#
            ));
            let err = format!("{:#}", translate(&json).unwrap_err());
            assert!(
                err.contains("invalid `extensions.dev.symposium`"),
                "{invalid}: {err}"
            );
        }
    }

    #[test]
    fn skills_are_the_immediate_children_holding_a_skill_md() {
        let tmp = package(
            &with_schema(r#""name": "x""#),
            &[
                File("skills/b-skill/SKILL.md", SKILL),
                File("skills/a-skill/SKILL.md", SKILL),
                File("skills/a-skill/nested/SKILL.md", SKILL),
                File("skills/no-skill/README.md", ""),
                File("skills/deep/too-deep/SKILL.md", SKILL),
                File("skills/stray.md", ""),
            ],
        );
        fs::create_dir_all(tmp.path().join("skills/dir-named/SKILL.md")).unwrap();
        let (manifest, notices) = read_package(tmp.path()).unwrap();
        assert_eq!(group_paths(&manifest), ["skills/a-skill", "skills/b-skill"]);
        assert!(notices.is_empty(), "{notices:?}");
    }

    #[test]
    fn skills_that_is_not_a_directory_invalidates_only_the_skills() {
        let tmp = package(&with_schema(r#""name": "x""#), &[File("skills", "")]);
        let (manifest, notices) = read_package(tmp.path()).unwrap();
        assert_eq!(manifest.name.as_deref(), Some("x"));
        assert!(manifest.skills.is_empty());
        assert_eq!(notices, ["`skills` is not a directory; no skills loaded"]);
    }

    #[test]
    fn reports_a_root_skill_md() {
        let tmp = package(&with_schema(r#""name": "x""#), &[File("SKILL.md", SKILL)]);
        let (manifest, notices) = read_package(tmp.path()).unwrap();
        assert!(manifest.skills.is_empty());
        assert_eq!(notices.len(), 1, "{notices:?}");
        assert!(
            notices[0].contains("move it to `skills/<name>/SKILL.md`"),
            "{notices:?}"
        );
    }

    const MCP_SCHEMA: &str = "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json";

    fn mcp_package(servers: &str) -> tempfile::TempDir {
        package(
            &with_schema(r#""name": "x""#),
            &[
                File(
                    "mcp.json",
                    &format!(r#"{{"$schema": "{MCP_SCHEMA}", "mcpServers": {{{servers}}}}}"#),
                ),
                File("bin/server", ""),
                File("skills/s/SKILL.md", SKILL),
            ],
        )
    }

    fn servers(manifest: &RawPluginManifest) -> Vec<&McpServer> {
        manifest.mcp_servers.iter().map(|raw| &raw.server).collect()
    }

    #[test]
    fn reads_mcp_servers_of_every_transport() {
        let tmp = mcp_package(
            r#""local": {"type": "stdio", "command": "./bin/server",
                         "args": ["--data", "${PLUGIN_DATA}"], "env": {"MODE": "fast"}},
               "bare": {"type": "stdio", "command": "npx"},
               "remote": {"type": "streamable-http", "url": "https://example.com/mcp",
                          "headers": {"X-Tenant": "public"}},
               "dev": {"type": "streamable-http", "url": "http://127.0.0.1:8080/mcp"},
               "legacy": {"type": "sse", "url": "https://example.com/sse"}"#,
        );
        let (manifest, notices) = read_package(tmp.path()).unwrap();
        assert!(notices.is_empty(), "{notices:?}");
        let root = dunce::canonicalize(tmp.path()).unwrap();
        let servers = servers(&manifest);
        assert_eq!(servers.len(), 5);
        let McpServer::Stdio(local) = servers.iter().find(|s| name(s) == "local").unwrap() else {
            panic!("local is not stdio");
        };
        assert_eq!(local.command, root.join("bin").join("server"));
        assert_eq!(local.args, ["--data", "${PLUGIN_DATA}"]);
        assert_eq!(local.env, [EnvVariable::new("MODE", "fast")]);
        let McpServer::Stdio(bare) = servers.iter().find(|s| name(s) == "bare").unwrap() else {
            panic!("bare is not stdio");
        };
        assert_eq!(bare.command, PathBuf::from("npx"));
        assert!(matches!(
            servers.iter().find(|s| name(s) == "remote").unwrap(),
            McpServer::Http(http) if http.headers == [HttpHeader::new("X-Tenant", "public")]
        ));
        assert!(matches!(
            servers.iter().find(|s| name(s) == "legacy").unwrap(),
            McpServer::Sse(_)
        ));
        assert_eq!(group_paths(&manifest), ["skills/s"]);
    }

    fn name(server: &McpServer) -> &str {
        match server {
            McpServer::Stdio(s) => &s.name,
            McpServer::Http(s) => &s.name,
            McpServer::Sse(s) => &s.name,
            _ => "",
        }
    }

    #[test]
    fn an_invalid_mcp_server_is_skipped_alone() {
        let tmp = mcp_package(
            r#""good": {"type": "stdio", "command": "npx"},
               "ws": {"type": "websocket", "url": "wss://example.com"},
               "mixed": {"type": "stdio", "command": "npx", "url": "https://example.com"},
               "escapes": {"type": "stdio", "command": "./.."},
               "missing": {"type": "stdio", "command": "./bin/absent"},
               "relative": {"type": "stdio", "command": "bin/server"},
               "reserved": {"type": "stdio", "command": "npx", "env": {"PLUGIN_ROOT": "/x"}},
               "plain-http": {"type": "streamable-http", "url": "http://example.com/mcp"},
               "fragment": {"type": "streamable-http", "url": "https://example.com/mcp#top"},
               "twice": {"type": "sse", "url": "https://example.com", "headers": {"A": "1", "a": "2"}},
               "bad-name": {"type": "sse", "url": "https://example.com", "headers": {"X Tenant": "1"}},
               "bad-value": {"type": "sse", "url": "https://example.com", "headers": {"X-Tenant": "a\nb"}},
               "with-cwd": {"type": "stdio", "command": "npx", "cwd": "${PLUGIN_ROOT}"},
               "untyped": {"command": "npx"}"#,
        );
        let (manifest, notices) = read_package(tmp.path()).unwrap();
        let names: Vec<&str> = servers(&manifest).into_iter().map(name).collect();
        assert_eq!(names, ["good"]);
        let skipped = |server: &str, reason: &str| {
            assert!(
                notices
                    .iter()
                    .any(|n| n.starts_with(&format!("MCP server `{server}` skipped"))
                        && n.contains(reason)),
                "{server}: {notices:?}"
            );
        };
        skipped("ws", "unsupported transport `websocket`");
        skipped("mixed", "field `url` is not allowed");
        skipped("escapes", "resolves outside the package");
        skipped("missing", "does not exist in the package");
        skipped(
            "relative",
            "bare executable name or a path starting with `./`",
        );
        skipped("reserved", "must not set `PLUGIN_ROOT`");
        skipped("plain-http", "must use https");
        skipped("fragment", "user information or a fragment");
        skipped("twice", "more than once");
        skipped("bad-name", "`X Tenant` is not a valid HTTP header");
        skipped("bad-value", "`X-Tenant` is not a valid HTTP header");
        skipped("with-cwd", "`cwd` is not supported");
        skipped("untyped", "missing required field `type`");
        assert_eq!(notices.len(), 13, "{notices:?}");
    }

    #[test]
    fn an_invalid_mcp_json_disables_only_the_mcp_servers() {
        let other_version = "https://agent-plugins.org/schemas/1.1.0/mcp.schema.json";
        let cases = [
            ("{".to_string(), "is not valid JSON"),
            (
                r#"{"mcpServers": {}}"#.to_string(),
                "misses required field `$schema`",
            ),
            (
                format!(r#"{{"$schema": "{MCP_SCHEMA}"}}"#),
                "misses required field `mcpServers`",
            ),
            (
                format!(r#"{{"$schema": "{other_version}", "mcpServers": {{}}}}"#),
                "targets Agent Plugins 1.1.0 but `plugin.json` targets 1.0.0",
            ),
            (
                format!(r#"{{"$schema": "{MCP_SCHEMA}", "mcpServers": {{}}, "extra": 1}}"#),
                "not `extra`",
            ),
            (
                format!(r#"{{"$schema": "{MCP_SCHEMA}", "mcpServers": []}}"#),
                "must be an object",
            ),
        ];
        for (mcp_json, expected) in cases {
            let tmp = package(
                &with_schema(r#""name": "x""#),
                &[
                    File("mcp.json", &mcp_json),
                    File("skills/s/SKILL.md", SKILL),
                ],
            );
            let (manifest, notices) = read_package(tmp.path()).unwrap();
            assert!(manifest.mcp_servers.is_empty(), "{mcp_json}");
            assert_eq!(group_paths(&manifest), ["skills/s"], "{mcp_json}");
            assert_eq!(notices.len(), 1, "{mcp_json}: {notices:?}");
            assert!(
                notices[0].starts_with("no MCP servers loaded") && notices[0].contains(expected),
                "{mcp_json}: {notices:?}"
            );
        }

        let tmp = package(&with_schema(r#""name": "x""#), &[File("mcp.json/x", "")]);
        let (_, notices) = read_package(tmp.path()).unwrap();
        assert_eq!(notices, ["no MCP servers loaded: `mcp.json` is not a file"]);
    }

    #[test]
    fn plugin_vars_expand_once_and_reach_the_server_env() {
        let server = McpServer::Stdio(
            McpServerStdio::new("s", "./bin/server")
                .args(vec![
                    "${PLUGIN_ROOT}/config.json".into(),
                    "${PLUGIN_DATA}".into(),
                    "${PLUGIN_ROOTX} ${HOME} $PLUGIN_DATA".into(),
                ])
                .env(vec![EnvVariable::new("CACHE", "${PLUGIN_DATA}/cache")]),
        );
        let root = Path::new("/plugins/${PLUGIN_DATA}");
        let data = Path::new("/data/x");
        let McpServer::Stdio(expanded) = with_plugin_vars(server, root, data) else {
            panic!("not stdio");
        };
        assert_eq!(expanded.command, PathBuf::from("./bin/server"));
        assert_eq!(
            expanded.args,
            [
                "/plugins/${PLUGIN_DATA}/config.json",
                "/data/x",
                "${PLUGIN_ROOTX} ${HOME} $PLUGIN_DATA",
            ]
        );
        assert_eq!(
            expanded.env,
            [
                EnvVariable::new("CACHE", "/data/x/cache"),
                EnvVariable::new("PLUGIN_ROOT", "/plugins/${PLUGIN_DATA}"),
                EnvVariable::new("PLUGIN_DATA", "/data/x"),
            ]
        );

        let remote = McpServer::Http(McpServerHttp::new(
            "r",
            "https://example.com/${PLUGIN_ROOT}",
        ));
        assert_eq!(with_plugin_vars(remote.clone(), root, data), remote);
    }

    #[cfg(unix)]
    #[test]
    fn a_path_resolving_outside_the_package_fails_at_the_narrowest_boundary() {
        use std::os::unix::fs::symlink;

        let outside = package(
            &with_schema(r#""name": "outside""#),
            &[File("skills/stolen/SKILL.md", SKILL)],
        );

        let linked_manifest = instantiate_fixture(&[File("skills/own/SKILL.md", SKILL)]);
        symlink(
            outside.path().join(AGENT_PLUGIN_FILE),
            linked_manifest.path().join(AGENT_PLUGIN_FILE),
        )
        .unwrap();
        let err = read_package(linked_manifest.path()).unwrap_err();
        assert!(
            err.to_string().contains("resolves outside the package"),
            "{err}"
        );

        let linked_skills = package(&with_schema(r#""name": "x""#), &[]);
        symlink(
            outside.path().join("skills"),
            linked_skills.path().join("skills"),
        )
        .unwrap();
        let (manifest, notices) = read_package(linked_skills.path()).unwrap();
        assert!(manifest.skills.is_empty());
        assert_eq!(
            notices,
            ["`skills` resolves outside the package; no skills loaded"]
        );

        let linked_child = package(
            &with_schema(r#""name": "x""#),
            &[
                File("skills/own/SKILL.md", SKILL),
                File("shared/kept/SKILL.md", SKILL),
            ],
        );
        symlink(
            outside.path().join("skills/stolen"),
            linked_child.path().join("skills/stolen"),
        )
        .unwrap();
        symlink(
            linked_child.path().join("shared/kept"),
            linked_child.path().join("skills/kept"),
        )
        .unwrap();
        let (manifest, notices) = read_package(linked_child.path()).unwrap();
        assert_eq!(group_paths(&manifest), ["skills/kept", "skills/own"]);
        assert_eq!(
            notices,
            ["skill `stolen` skipped: its `SKILL.md` resolves outside the package"]
        );
    }

    #[test]
    fn fill_takes_identity_only_where_the_symposium_manifest_omits_it() {
        let tmp = package(
            &with_schema(
                r#""name": "json-name", "version": "2.0.0", "description": "from json",
                   "extensions": {"dev.symposium": {"depends-on": ["serde"]}}"#,
            ),
            &[],
        );
        let mut manifest = RawPluginManifest {
            version: Some("1.0.0".into()),
            ..Default::default()
        };
        assert_eq!(fill(&mut manifest, tmp.path()), None);
        assert_eq!(manifest.name.as_deref(), Some("json-name"));
        assert_eq!(manifest.version.as_deref(), Some("1.0.0"));
        assert_eq!(manifest.description.as_deref(), Some("from json"));
        assert!(manifest.depends_on.0.is_empty());

        let malformed = package(r#"{"name": "x"}"#, &[]);
        let mut manifest = RawPluginManifest::default();
        let notice = fill(&mut manifest, malformed.path()).unwrap();
        assert!(
            notice.contains("missing required field `$schema`"),
            "{notice}"
        );
        assert!(manifest.name.is_none());

        let absent = instantiate_fixture(&[]);
        assert_eq!(fill(&mut RawPluginManifest::default(), absent.path()), None);
    }
}
