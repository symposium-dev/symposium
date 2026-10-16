use super::*;

use std::fs;
use std::path::PathBuf;
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

fn read(path: PathBuf) -> String {
    fs::read_to_string(path).unwrap()
}

#[test]
fn copies_are_kept_in_step_and_only_marked_directories_are_reaped() {
    let tmp = tempfile::tempdir().unwrap();
    let mut sym = Symposium::from_dir(tmp.path());
    sym.config.sync_debounce_secs = 0;
    let staging = tmp.path().join("installed");
    let plugins_dir = tmp.path().join("plugins");
    mkdir(
        &plugins_dir,
        &["users-plugin/plugin.json", "users-plugin/skills/x/SKILL.md"],
    );
    mkdir(
        &plugins_dir,
        &[
            "stale/plugin.json",
            &format!("stale/{}", crate::sync::MARKER_FILE),
        ],
    );
    mkdir(&plugins_dir, &["taken/plugin.json"]);
    let fresh = compiled(&staging, "fresh", "v1");
    let taken = compiled(&staging, "taken", "v1");

    sync_plugin_copies(
        &sym,
        Agent::Goose,
        &plugins_dir,
        &[fresh.clone(), taken.clone()],
    );

    let copied = plugins_dir.join("fresh/skills/guide/SKILL.md");
    assert_eq!(read(copied.clone()), "v1");
    assert!(plugins_dir.join("fresh/.symposium").is_file());
    assert!(
        !plugins_dir.join("taken/skills").exists(),
        "a directory symposium does not own is skipped"
    );
    assert!(!plugins_dir.join("stale").exists());
    assert!(plugins_dir.join("users-plugin/skills/x/SKILL.md").is_file());

    let an_hour_ago = SystemTime::now() - Duration::from_secs(3600);
    fs::File::options()
        .write(true)
        .open(&copied)
        .unwrap()
        .set_modified(an_hour_ago)
        .unwrap();
    sync_plugin_copies(&sym, Agent::Goose, &plugins_dir, &[fresh, taken.clone()]);
    assert_eq!(
        fs::metadata(&copied).unwrap().modified().unwrap(),
        an_hour_ago,
        "an unchanged plugin is not rewritten"
    );

    let fresh = compiled(&staging, "fresh", "v2");
    sync_plugin_copies(&sym, Agent::Goose, &plugins_dir, &[fresh, taken]);
    assert_eq!(read(copied), "v2");

    sync_plugin_copies(&sym, Agent::Goose, &plugins_dir, &[]);
    assert!(!plugins_dir.join("fresh").exists());
    for kept in ["users-plugin", "taken"] {
        assert!(plugins_dir.join(kept).exists(), "{kept} must be left alone");
    }
}
