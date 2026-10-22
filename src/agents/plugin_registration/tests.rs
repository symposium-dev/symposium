use super::*;

use std::fs;
use std::path::Path;

fn compiled(staging: &Path, name: &str) -> CompiledPlugin {
    let dir = staging.join(name);
    write(&dir.join(".claude-plugin/plugin.json"), name);
    write(&dir.join(crate::sync::MARKER_FILE), "");
    CompiledPlugin {
        name: name.into(),
        dir,
    }
}

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

#[test]
fn a_directory_symposium_does_not_own_keeps_its_slot() {
    let tmp = tempfile::tempdir().unwrap();
    let sym = Symposium::from_dir(tmp.path());
    let skills = tmp.path().join(".claude/skills");
    write(&skills.join("taken/SKILL.md"), "mine");
    let staging = tmp.path().join("installed");

    Agent::Claude.deliver_user_plugins(
        &sym,
        &[compiled(&staging, "taken"), compiled(&staging, "free")],
    );

    assert_eq!(fs::read_dir(skills.join("taken")).unwrap().count(), 1);
    assert!(skills.join("free/.claude-plugin/plugin.json").is_file());
}

#[test]
fn only_plugin_directories_symposium_installed_are_reaped() {
    let tmp = tempfile::tempdir().unwrap();
    let sym = Symposium::from_dir(tmp.path());
    let skills = tmp.path().join(".claude/skills");
    write(&skills.join("users-skill/SKILL.md"), "");
    write(&skills.join("users-plugin/.claude-plugin/plugin.json"), "");
    write(&skills.join("per-skill-copy/.symposium"), "");
    write(&skills.join("stale/.claude-plugin/plugin.json"), "");
    write(&skills.join("stale/.symposium"), "");

    Agent::Claude.deliver_user_plugins(&sym, &[]);

    assert!(!skills.join("stale").exists());
    for kept in ["users-skill", "users-plugin", "per-skill-copy"] {
        assert!(skills.join(kept).exists(), "{kept} must be left alone");
    }
}

#[test]
fn delivery_never_writes_claude_settings() {
    let tmp = tempfile::tempdir().unwrap();
    let sym = Symposium::from_dir(tmp.path());
    let settings = tmp.path().join(".claude/settings.json");
    let disabled = r#"{"enabledPlugins": {"fresh@skills-dir": false}}"#;
    write(&settings, disabled);

    Agent::Claude.deliver_user_plugins(&sym, &[compiled(&tmp.path().join("installed"), "fresh")]);

    assert!(tmp.path().join(".claude/skills/fresh").is_dir());
    assert_eq!(fs::read_to_string(&settings).unwrap(), disabled);
}
