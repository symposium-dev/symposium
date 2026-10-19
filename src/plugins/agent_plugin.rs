use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::Value;
use symposium_sdk::manifest::RawPluginManifest;
use symposium_sdk::predicate::{DependsOnList, PredicateSet};

use crate::pm::layout::{AGENT_PLUGIN_FILE, SKILL_FILE};

const SUPPORTED_SCHEMAS: &[&str] = &["https://agent-plugins.org/schemas/1.0.0/plugin.schema.json"];
const SYMPOSIUM_NAMESPACE: &str = "dev.symposium";
const SKILLS_DIR: &str = "skills";
const MCP_FILE: &str = "mcp.json";
const MAX_NAME_LEN: usize = 64;

struct AgentPluginJson {
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
    if fs::symlink_metadata(root.join(MCP_FILE)).is_ok() {
        notices.push(format!(
            "`{MCP_FILE}` is not supported yet; its MCP servers are ignored"
        ));
    }
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

    match fields.get("$schema") {
        None => bail!("missing required field `$schema`"),
        Some(Value::String(schema)) if SUPPORTED_SCHEMAS.contains(&schema.as_str()) => {}
        Some(Value::String(schema)) => bail!(
            "unsupported `$schema` `{schema}`; supported: {}",
            SUPPORTED_SCHEMAS.join(", ")
        ),
        Some(_) => bail!("`$schema` must be a string"),
    }

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
        name,
        version,
        description,
        gate,
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

pub(crate) fn resolves_inside(root: &Path, path: &Path) -> bool {
    fs::canonicalize(path).is_ok_and(|resolved| resolved.starts_with(root))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{File, instantiate_fixture};
    use symposium_sdk::manifest::RawPluginSource;

    const SKILL: &str = "---\nname: s\ndescription: d\n---\nBody.\n";

    fn with_schema(fields: &str) -> String {
        format!(r#"{{"$schema": "{}", {fields}}}"#, SUPPORTED_SCHEMAS[0])
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
        let cases: [(String, &str); 10] = [
            ("{".into(), "is not valid JSON"),
            ("[]".into(), "must hold a JSON object"),
            (r#"{"name": "x"}"#.into(), "missing required field `$schema`"),
            (
                r#"{"$schema": "https://agent-plugins.org/schemas/1.1.0/plugin.schema.json", "name": "x"}"#.into(),
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
    fn reports_an_mcp_json_and_a_root_skill_md() {
        let tmp = package(
            &with_schema(r#""name": "x""#),
            &[File("mcp.json", "{}"), File("SKILL.md", SKILL)],
        );
        let (manifest, notices) = read_package(tmp.path()).unwrap();
        assert!(manifest.skills.is_empty());
        assert_eq!(notices.len(), 2, "{notices:?}");
        assert!(
            notices[0].contains("`mcp.json` is not supported"),
            "{notices:?}"
        );
        assert!(
            notices[1].contains("move it to `skills/<name>/SKILL.md`"),
            "{notices:?}"
        );
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
