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
fn project_delivery_lands_in_the_project_and_stays_out_of_git() {
    let tmp = tempfile::tempdir().unwrap();
    let mut sym = Symposium::from_dir(&tmp.path().join("home"));
    sym.config.sync_debounce_secs = 0;
    let project = tmp.path().join("project");
    let staging = project.join(".symposium/plugins");
    let delivered = project.join(".claude/skills/fresh");
    let plugin = compiled(&staging, "fresh");

    Agent::Claude.deliver_project_plugins(&sym, &project, std::slice::from_ref(&plugin));
    assert!(delivered.join(".claude-plugin/plugin.json").is_file());
    assert_eq!(
        fs::read_to_string(delivered.join(".gitignore")).unwrap(),
        "*\n"
    );
    assert!(!sym.home_dir().join(".claude").exists());

    write(&plugin.dir.join(".claude-plugin/plugin.json"), "v2");
    Agent::Claude.deliver_project_plugins(&sym, &project, std::slice::from_ref(&plugin));
    assert_eq!(
        fs::read_to_string(delivered.join(".claude-plugin/plugin.json")).unwrap(),
        "v2"
    );
    assert_eq!(
        fs::read_to_string(delivered.join(".gitignore")).unwrap(),
        "*\n",
        "replacing the content keeps the directory out of git"
    );

    Agent::Claude.deliver_project_plugins(&sym, &project, &[]);
    assert!(!delivered.exists());
}

#[test]
fn only_claude_takes_project_plugins() {
    let tmp = tempfile::tempdir().unwrap();
    let sym = Symposium::from_dir(tmp.path());
    for &agent in Agent::all() {
        let takes = agent
            .deliver_project_plugins(&sym, tmp.path(), &[])
            .is_some();
        assert_eq!(takes, agent == Agent::Claude, "{}", agent.config_name());
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
