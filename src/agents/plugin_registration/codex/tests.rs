use super::*;

use std::time::SystemTime;

use expect_test::expect;

struct Codex {
    _tmp: tempfile::TempDir,
    dir: PathBuf,
    root: PathBuf,
}

impl Codex {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        Codex {
            dir: tmp.path().join("codex"),
            root: tmp.path().join("installed"),
            _tmp: tmp,
        }
    }

    fn stage(&self, name: &str, version: &str, body: &str) -> CompiledPlugin {
        let dir = self.root.join(name);
        let manifest = format!(r#"{{"name": "{name}", "version": "{version}"}}"#);
        write(&dir.join("plugin.json"), &manifest);
        write(&dir.join(".claude-plugin/plugin.json"), &manifest);
        write(&dir.join("skills/guide/SKILL.md"), body);
        write(&dir.join(sync::MARKER_FILE), "");
        write(&self.root.join(MARKETPLACE_MANIFEST), "{}");
        CompiledPlugin {
            name: name.into(),
            version: version.into(),
            dir,
        }
    }

    fn sync(&self, plugins: &[CompiledPlugin]) -> Result<bool> {
        sync_codex_plugins(&self.dir, &self.root, plugins, Duration::ZERO)
    }

    fn config_path(&self) -> PathBuf {
        self.dir.join("config.toml")
    }

    fn config(&self) -> String {
        let source = value(self.root.to_str().unwrap()).to_string();
        fs::read_to_string(self.config_path())
            .unwrap()
            .replace(source.trim(), r#""$ROOT""#)
    }

    fn cache(&self) -> PathBuf {
        self.dir.join("plugins/cache/symposium")
    }
}

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn tree(dir: &Path) -> String {
    sync::collect_skill_tree(dir)
        .unwrap()
        .into_iter()
        .map(|(rel, data)| {
            let rel = rel.to_string_lossy().replace('\\', "/");
            if data.is_some() {
                rel
            } else {
                format!("{rel}/")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn backdate(path: &Path) -> SystemTime {
    let an_hour_ago = SystemTime::now() - Duration::from_secs(3600);
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(an_hour_ago)
        .unwrap();
    an_hour_ago
}

fn modified(path: &Path) -> SystemTime {
    fs::metadata(path).unwrap().modified().unwrap()
}

#[test]
fn the_codex_directory_follows_codex_home() {
    let tmp = tempfile::tempdir().unwrap();
    let mut sym = Symposium::from_dir(tmp.path());
    let default = tmp.path().join(".codex");
    assert_eq!(codex_dir(&sym), default);

    sym.set_env(|name| (name == "CODEX_HOME").then(|| "/elsewhere/codex".into()));
    assert_eq!(codex_dir(&sym), Path::new("/elsewhere/codex"));

    sym.set_env(|name| (name == "CODEX_HOME").then(|| "".into()));
    assert_eq!(codex_dir(&sym), default, "an empty value does not relocate");
}

#[test]
fn delivery_writes_what_codex_plugin_add_writes() {
    let codex = Codex::new();
    let fresh = codex.stage("fresh", "0.0.0", "v1");

    assert!(codex.sync(&[fresh]).unwrap());

    expect![[r#"
        [marketplaces.symposium]
        source_type = "local"
        source = "$ROOT"

        [plugins."fresh@symposium"]
        enabled = true
    "#]]
    .assert_eq(&codex.config());
    expect![[r#"
        fresh/
        fresh/.symposium
        fresh/0.0.0/
        fresh/0.0.0/.claude-plugin/
        fresh/0.0.0/.claude-plugin/plugin.json
        fresh/0.0.0/.symposium
        fresh/0.0.0/plugin.json
        fresh/0.0.0/skills/
        fresh/0.0.0/skills/guide/
        fresh/0.0.0/skills/guide/SKILL.md"#]]
    .assert_eq(&tree(&codex.cache()));
}

#[test]
fn a_rerun_writes_nothing_and_an_edit_rewrites_the_copy() {
    let codex = Codex::new();
    codex.sync(&[codex.stage("fresh", "0.0.0", "v1")]).unwrap();
    let config_written = backdate(&codex.config_path());
    let skill = codex.cache().join("fresh/0.0.0/skills/guide/SKILL.md");
    let skill_written = backdate(&skill);

    codex.sync(&[codex.stage("fresh", "0.0.0", "v1")]).unwrap();
    assert_eq!(modified(&codex.config_path()), config_written);
    assert_eq!(modified(&skill), skill_written);

    codex.sync(&[codex.stage("fresh", "0.0.0", "v2")]).unwrap();
    assert_eq!(fs::read_to_string(&skill).unwrap(), "v2");
    assert_eq!(modified(&codex.config_path()), config_written);
}

#[test]
fn the_users_config_and_choices_survive() {
    let codex = Codex::new();
    let users_config = r#"# the user's comment
model = "gpt-5"

[mcp_servers.theirs]
command = "theirs"

[plugins."fresh@symposium"]
enabled = false # turned off in /plugins

[plugins."gone@symposium"]
enabled = true

[plugins."other@symposium-team"]
enabled = true

[projects."/somewhere"]
trust_level = "trusted"
"#;
    write(&codex.config_path(), users_config);

    codex
        .sync(&[
            codex.stage("fresh", "0.0.0", "v1"),
            codex.stage("added", "0.0.0", "v1"),
        ])
        .unwrap();

    expect![[r#"
        # the user's comment
        model = "gpt-5"

        [mcp_servers.theirs]
        command = "theirs"

        [plugins."fresh@symposium"]
        enabled = false # turned off in /plugins

        [plugins."other@symposium-team"]
        enabled = true

        [plugins."added@symposium"]
        enabled = true

        [projects."/somewhere"]
        trust_level = "trusted"

        [marketplaces.symposium]
        source_type = "local"
        source = "$ROOT"
    "#]]
    .assert_eq(&codex.config());
}

#[test]
fn an_empty_set_removes_only_what_symposium_wrote() {
    let codex = Codex::new();
    let users_config = "model = \"gpt-5\"\n\n[plugins.\"other@team\"]\nenabled = true\n";
    write(&codex.config_path(), users_config);
    let teams_cache = codex.dir.join("plugins/cache/team/other/1.0.0/plugin.json");
    write(&teams_cache, "{}");
    let codex_made = codex.cache().join("old/0.0.0/plugin.json");
    write(&codex_made, "{}");
    codex.sync(&[codex.stage("fresh", "0.0.0", "v1")]).unwrap();

    assert!(codex.sync(&[]).unwrap());

    assert_eq!(
        fs::read_to_string(codex.config_path()).unwrap(),
        users_config
    );
    assert!(!codex.cache().join("fresh").exists());
    assert!(teams_cache.is_file());
    assert!(
        codex_made.is_file(),
        "a copy without the marker is not symposium's to remove"
    );
}

#[test]
fn nothing_is_written_when_nothing_is_installed() {
    let codex = Codex::new();
    assert!(codex.sync(&[]).unwrap());
    assert!(!codex.dir.exists());
}

#[test]
fn another_marketplace_named_symposium_is_left_alone() {
    let codex = Codex::new();
    let users_config = "[marketplaces.symposium]\nsource_type = \"git\"\nsource = \"https://example.com/theirs.git\"\n\n[plugins.\"theirs@symposium\"]\nenabled = true\n";
    write(&codex.config_path(), users_config);

    let err = codex
        .sync(&[codex.stage("fresh", "0.0.0", "v1")])
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("registers another marketplace named `symposium`"),
        "{err:#}"
    );
    assert!(!codex.sync(&[]).unwrap());

    assert_eq!(
        fs::read_to_string(codex.config_path()).unwrap(),
        users_config
    );
    assert!(!codex.cache().exists());
}

#[test]
fn a_malformed_config_is_an_error_and_stays_untouched() {
    let codex = Codex::new();
    write(&codex.config_path(), "[[broken");

    assert!(codex.sync(&[codex.stage("fresh", "0.0.0", "v1")]).is_err());
    assert!(codex.sync(&[]).is_err());

    assert_eq!(fs::read_to_string(codex.config_path()).unwrap(), "[[broken");
    assert!(!codex.cache().exists());
}

#[test]
fn the_marketplace_is_registered_only_while_its_manifest_exists() {
    let codex = Codex::new();
    let fresh = codex.stage("fresh", "0.0.0", "v1");
    codex.sync(std::slice::from_ref(&fresh)).unwrap();

    fs::remove_file(codex.root.join(MARKETPLACE_MANIFEST)).unwrap();
    codex.sync(&[fresh]).unwrap();

    expect![[r#"

        [plugins."fresh@symposium"]
        enabled = true
    "#]]
    .assert_eq(&codex.config());
    assert!(codex.cache().join("fresh/0.0.0/plugin.json").is_file());
}

#[test]
fn the_cache_holds_one_version_directory_whoever_wrote_it() {
    let codex = Codex::new();
    let reinstalled_by_codex = codex.cache().join("fresh/0.0.0/skills/guide/SKILL.md");
    write(&reinstalled_by_codex, "older");
    write(&codex.cache().join("fresh/9.9.9/plugin.json"), "{}");

    codex.sync(&[codex.stage("fresh", "0.0.0", "v1")]).unwrap();
    assert_eq!(fs::read_to_string(&reinstalled_by_codex).unwrap(), "v1");
    assert!(sync::has_symposium_marker(&codex.cache().join("fresh")));
    assert!(!codex.cache().join("fresh/9.9.9").exists());

    codex.sync(&[codex.stage("fresh", "0.0.1", "v1")]).unwrap();
    let versions: Vec<_> = fs::read_dir(codex.cache().join("fresh"))
        .unwrap()
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name())
        .collect();
    assert_eq!(versions, ["0.0.1"]);
}

#[cfg(unix)]
#[test]
fn a_symlinked_config_stays_a_symlink() {
    let codex = Codex::new();
    let linked = codex.dir.parent().unwrap().join("dotfiles/config.toml");
    write(&linked, "model = \"gpt-5\"\n");
    fs::create_dir_all(&codex.dir).unwrap();
    std::os::unix::fs::symlink(&linked, codex.config_path()).unwrap();

    codex.sync(&[codex.stage("fresh", "0.0.0", "v1")]).unwrap();

    assert!(
        fs::symlink_metadata(codex.config_path())
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        fs::read_to_string(&linked)
            .unwrap()
            .contains(r#"[plugins."fresh@symposium"]"#)
    );
}
