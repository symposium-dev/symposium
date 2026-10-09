use super::*;

use std::fs;
use std::time::SystemTime;

fn compiled(root: &Path, name: &str, body: &str) -> CompiledPlugin {
    let dir = root.join(name);
    fs::create_dir_all(dir.join(".claude-plugin")).unwrap();
    fs::create_dir_all(dir.join("skills/guide")).unwrap();
    let manifest = format!(r#"{{"name": "{name}"}}"#);
    fs::write(dir.join("plugin.json"), &manifest).unwrap();
    fs::write(dir.join(".claude-plugin/plugin.json"), &manifest).unwrap();
    fs::write(dir.join("skills/guide/SKILL.md"), body).unwrap();
    fs::write(dir.join(crate::sync::MARKER_FILE), "").unwrap();
    CompiledPlugin {
        name: name.into(),
        version: "0.0.0".into(),
        dir,
    }
}

fn mkdir(dir: &Path, files: &[&str]) {
    for file in files {
        let path = dir.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "").unwrap();
    }
}

#[test]
fn the_user_skills_directory_follows_claude_config_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let mut sym = Symposium::from_dir(tmp.path());
    let default = tmp.path().join(".claude/skills");
    assert_eq!(user_skills_dir(&sym), default);

    sym.set_env(|name| (name == "CLAUDE_CONFIG_DIR").then(|| "/elsewhere/claude".into()));
    assert_eq!(user_skills_dir(&sym), Path::new("/elsewhere/claude/skills"));

    sym.set_env(|name| (name == "CLAUDE_CONFIG_DIR").then(|| "".into()));
    assert_eq!(
        user_skills_dir(&sym),
        default,
        "an empty value does not relocate"
    );
}

#[test]
fn delivery_copies_plugins_and_reaps_only_what_symposium_installed() {
    let tmp = tempfile::tempdir().unwrap();
    let staging = tmp.path().join("installed");
    let skills_dir = tmp.path().join("skills");
    mkdir(&skills_dir, &["users-skill/SKILL.md"]);
    mkdir(
        &skills_dir,
        &[
            "users-plugin/.claude-plugin/plugin.json",
            "users-plugin/skills/x/SKILL.md",
        ],
    );
    mkdir(
        &skills_dir,
        &["per-skill-copy/SKILL.md", "per-skill-copy/.symposium"],
    );
    mkdir(
        &skills_dir,
        &[
            "stale-plugin/.claude-plugin/plugin.json",
            "stale-plugin/.symposium",
        ],
    );
    mkdir(&skills_dir, &["taken/SKILL.md"]);
    let fresh = compiled(&staging, "fresh", "v1");
    let taken = compiled(&staging, "taken", "v1");

    sync_skills_dir_plugins(&skills_dir, &[fresh.clone(), taken.clone()], Duration::ZERO);

    assert_eq!(
        fs::read_to_string(skills_dir.join("fresh/skills/guide/SKILL.md")).unwrap(),
        "v1"
    );
    assert!(skills_dir.join("fresh/.symposium").is_file());
    let taken_copy = skills_dir.join(format!(
        "taken-{}",
        crate::skills::hash_origin_key(&"taken")
    ));
    assert!(
        taken_copy.join(".claude-plugin/plugin.json").is_file(),
        "a user's directory in the way moves the copy aside"
    );
    assert!(skills_dir.join("taken/SKILL.md").is_file());
    assert!(!skills_dir.join("stale-plugin").exists());
    for kept in ["users-skill", "users-plugin", "per-skill-copy"] {
        assert!(skills_dir.join(kept).exists(), "{kept} must be left alone");
    }

    let copied = skills_dir.join("fresh/skills/guide/SKILL.md");
    let an_hour_ago = SystemTime::now() - Duration::from_secs(3600);
    fs::File::options()
        .write(true)
        .open(&copied)
        .unwrap()
        .set_modified(an_hour_ago)
        .unwrap();
    sync_skills_dir_plugins(&skills_dir, &[fresh.clone(), taken.clone()], Duration::ZERO);
    assert_eq!(
        fs::metadata(&copied).unwrap().modified().unwrap(),
        an_hour_ago
    );

    let fresh = compiled(&staging, "fresh", "v2");
    sync_skills_dir_plugins(&skills_dir, &[fresh, taken], Duration::ZERO);
    assert_eq!(fs::read_to_string(&copied).unwrap(), "v2");

    sync_skills_dir_plugins(&skills_dir, &[], Duration::ZERO);
    assert!(!skills_dir.join("fresh").exists());
    assert!(!taken_copy.exists());
    for kept in ["users-skill", "users-plugin", "per-skill-copy", "taken"] {
        assert!(skills_dir.join(kept).exists(), "{kept} must be left alone");
    }
}

#[test]
fn delivery_never_writes_claude_settings() {
    let tmp = tempfile::tempdir().unwrap();
    let sym = Symposium::from_dir(tmp.path());
    let settings = tmp.path().join(".claude/settings.json");
    mkdir(tmp.path(), &[".claude/settings.json"]);
    fs::write(
        &settings,
        r#"{"enabledPlugins": {"fresh@skills-dir": false}}"#,
    )
    .unwrap();
    let fresh = compiled(&tmp.path().join("installed"), "fresh", "v1");

    let delivered = sync_user_plugins(&sym, tmp.path(), &[fresh], &Output::quiet()).unwrap();

    assert!(delivered);
    assert!(
        tmp.path()
            .join(".claude/skills/fresh/plugin.json")
            .is_file()
    );
    assert_eq!(
        fs::read_to_string(&settings).unwrap(),
        r#"{"enabledPlugins": {"fresh@skills-dir": false}}"#,
        "a user's own `false` stays"
    );
}
