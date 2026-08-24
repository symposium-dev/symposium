use std::path::{Path, PathBuf};

use super::read;
use crate::plugins::{PluginSource, SkillDepth};

fn package(dir: &Path, manifest: &str) -> PathBuf {
    std::fs::create_dir_all(dir).expect("create package dir");
    std::fs::write(dir.join("plugin.json"), manifest).expect("write manifest");
    dir.to_path_buf()
}

fn skill(dir: &Path, rel: &str, name: &str) {
    let skill_dir = dir.join(rel);
    std::fs::create_dir_all(&skill_dir).expect("create skill dir");
    std::fs::write(
        skill_dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: d\n---\nbody\n"),
    )
    .expect("write SKILL.md");
}

const MINIMAL: &str = r#"{
  "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
  "name": "pdf-tools"
}"#;

#[test]
fn a_package_becomes_a_plugin_with_one_immediate_children_group() {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = package(&tmp.path().join("pdf-tools"), MINIMAL);

    let plugin = read::load(&dir, false).expect("load");
    assert_eq!(plugin.name, "pdf-tools");
    assert_eq!(plugin.skills.len(), 1);
    assert_eq!(
        plugin.skills[0].source,
        PluginSource::Path(PathBuf::from("skills")),
        "the format fixes the location and the manifest cannot redirect it"
    );
    assert_eq!(plugin.skills[0].depth, SkillDepth::ImmediateChildren);
}

#[test]
fn identity_fields_carry_over() {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = package(
        &tmp.path().join("pdf-tools"),
        r#"{
          "name": "pdf-tools",
          "version": "1.2.0",
          "description": "Table extraction guidance"
        }"#,
    );
    let plugin = read::load(&dir, false).expect("load");
    assert_eq!(plugin.version.as_deref(), Some("1.2.0"));
    assert_eq!(
        plugin.description.as_deref(),
        Some("Table extraction guidance")
    );
}

#[test]
fn a_package_with_no_gate_is_dormant_unless_its_position_gates_it() {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = package(&tmp.path().join("pdf-tools"), MINIMAL);

    assert!(
        read::load(&dir, false).expect("load").requires_use,
        "the format cannot say when a package applies, so a registry entry waits to be used"
    );
    assert!(
        !read::load(&dir, true).expect("load").requires_use,
        "a workspace member or a referenced crate is already gated by where it was found"
    );
}

#[test]
fn the_symposium_namespace_supplies_a_gate() {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = package(
        &tmp.path().join("pdf-tools"),
        r#"{
          "name": "pdf-tools",
          "extensions": {
            "dev.symposium": {
              "depends-on": ["lopdf"],
              "predicates": ["path_exists(pdftotext)"]
            }
          }
        }"#,
    );
    let plugin = read::load(&dir, false).expect("load");
    assert!(
        !plugin.requires_use,
        "a declared gate takes the package out of dormancy"
    );
    assert!(plugin.predicates.references_dep("lopdf"));
    assert_eq!(plugin.predicates.predicates.len(), 2);
}

#[test]
fn an_unrelated_extensions_namespace_is_ignored_without_being_inspected() {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = package(
        &tmp.path().join("pdf-tools"),
        r#"{
          "name": "pdf-tools",
          "extensions": { "com.example.other": { "whatever": [1, 2, 3] } }
        }"#,
    );
    let plugin = read::load(&dir, false).expect("load");
    assert!(plugin.requires_use, "still no gate of ours");
    assert!(plugin.predicates.predicates.is_empty());
}

#[test]
fn a_malformed_symposium_gate_rejects_the_package() {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = package(
        &tmp.path().join("pdf-tools"),
        r#"{
          "name": "pdf-tools",
          "extensions": { "dev.symposium": { "depends-on": ["lopdf"], "typo": true } }
        }"#,
    );
    let err = read::load(&dir, false).expect_err("must not load");
    assert!(
        format!("{err:#}").contains("dev.symposium"),
        "the gate was written for us, so ignoring it would over-activate: {err:#}"
    );
}

#[test]
fn a_name_breaking_the_formats_grammar_rejects_the_package() {
    let tmp = tempfile::tempdir().expect("tmp");
    for bad in ["Pdf_Tools", "-leading", ""] {
        let dir = package(&tmp.path().join("pkg"), &format!("{{\"name\": \"{bad}\"}}"));
        let err = read::load(&dir, false).expect_err("must not load");
        assert!(
            format!("{err:#}").contains("not 1 to 64 characters"),
            "unexpected error for {bad:?}: {err:#}"
        );
    }
}

#[test]
fn a_missing_name_or_broken_json_rejects_the_package() {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = package(&tmp.path().join("pkg"), r#"{"version": "1.0.0"}"#);
    assert!(read::load(&dir, false).is_err(), "name is required");

    let dir = package(&tmp.path().join("pkg2"), "{ not json");
    assert!(read::load(&dir, false).is_err());
}

#[test]
fn unknown_top_level_fields_do_not_stop_the_package_loading() {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = package(
        &tmp.path().join("pdf-tools"),
        r#"{
          "name": "pdf-tools",
          "license": "MIT",
          "keywords": ["pdf"],
          "somethingNew": { "from": "a later spec" }
        }"#,
    );
    let plugin = read::load(&dir, false).expect("load");
    assert_eq!(plugin.name, "pdf-tools");
}

#[test]
fn only_immediate_children_of_skills_hold_skills() {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = package(&tmp.path().join("pdf-tools"), MINIMAL);
    skill(&dir, "skills/extract", "extract");
    skill(&dir, "skills/nested/deeper", "deeper");

    let plugin = read::load(&dir, false).expect("load");
    let found = crate::skills::discover_skills(
        &dir.join("skills"),
        false,
        &crate::predicate::PredicateSet::default(),
        plugin.skills[0].depth,
    );
    let names: Vec<String> = found
        .iter()
        .filter_map(|r| r.as_ref().ok())
        .map(|s| s.name().to_string())
        .collect();
    assert_eq!(
        names,
        vec!["extract"],
        "deeper folders are not searched, per the format"
    );
}

#[test]
fn a_broken_skill_is_skipped_and_the_others_load() {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = package(&tmp.path().join("pdf-tools"), MINIMAL);
    skill(&dir, "skills/good", "good");
    std::fs::create_dir_all(dir.join("skills/broken")).expect("create");
    std::fs::write(dir.join("skills/broken/SKILL.md"), "no frontmatter here").expect("write");

    let found = crate::skills::discover_skills(
        &dir.join("skills"),
        false,
        &crate::predicate::PredicateSet::default(),
        SkillDepth::ImmediateChildren,
    );
    assert_eq!(found.len(), 2);
    assert_eq!(found.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(found.iter().filter(|r| r.is_err()).count(), 1);
}

#[cfg(unix)]
#[test]
fn a_skill_symlinked_out_of_the_package_is_refused() {
    let tmp = tempfile::tempdir().expect("tmp");
    let outside = tmp.path().join("outside");
    skill(&outside, "secret", "secret");

    let dir = package(&tmp.path().join("pdf-tools"), MINIMAL);
    std::fs::create_dir_all(dir.join("skills")).expect("create");
    std::os::unix::fs::symlink(outside.join("secret"), dir.join("skills/secret")).expect("symlink");

    let found = crate::skills::discover_skills(
        &dir.join("skills"),
        false,
        &crate::predicate::PredicateSet::default(),
        SkillDepth::ImmediateChildren,
    );
    assert_eq!(found.len(), 1);
    let err = found[0].as_ref().expect_err("must be refused");
    assert!(
        format!("{err:#}").contains("resolves outside"),
        "the copy would silently drop it, so it has to be reported: {err:#}"
    );
}

#[test]
fn a_sibling_manifest_supplies_only_what_the_toml_omits() {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = package(
        &tmp.path().join("pdf-tools"),
        r#"{"name": "portable-name", "version": "1.2.0", "description": "from json"}"#,
    );

    let identity = read::sibling_identity(&dir);
    assert_eq!(identity.name.as_deref(), Some("portable-name"));
    assert_eq!(identity.version.as_deref(), Some("1.2.0"));
    assert_eq!(identity.description.as_deref(), Some("from json"));

    let none = read::sibling_identity(&tmp.path().join("empty"));
    assert!(none.name.is_none() && none.version.is_none());
}

#[test]
fn a_broken_sibling_manifest_is_ignored_rather_than_rejecting_the_toml() {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = package(&tmp.path().join("pdf-tools"), "{ not json");
    let identity = read::sibling_identity(&dir);
    assert!(
        identity.name.is_none(),
        "the TOML defines this plugin; a broken companion must not reject it"
    );
}

#[test]
fn an_mcp_component_does_not_stop_the_skills_loading() {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = package(
        &tmp.path().join("pdf-tools"),
        r#"{"name": "pdf-tools", "extensions": {"dev.symposium": {"depends-on": ["lopdf"]}}}"#,
    );
    std::fs::write(dir.join("mcp.json"), r#"{"mcpServers": {}}"#).expect("write mcp.json");
    skill(&dir, "skills/extract", "extract");

    let plugin = read::load(&dir, false).expect("load");
    assert_eq!(
        plugin.skills.len(),
        1,
        "the format's other component type is reported as unsupported, not fatal"
    );
    assert!(plugin.predicates.references_dep("lopdf"));
}
