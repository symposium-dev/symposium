use super::*;

use expect_test::expect;

fn compiled(staging: &Path, name: &str) -> CompiledPlugin {
    let dir = staging.join(name);
    write(&dir.join("plugin.json"), name);
    write(&dir.join(sync::MARKER_FILE), "");
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
fn enablement_keeps_the_users_config_and_a_plugin_they_turned_off() {
    let tmp = tempfile::tempdir().unwrap();
    let sym = Symposium::from_dir(tmp.path());
    let staging = sym.config_dir().join(INSTALLED_PLUGINS_SUBDIR);
    let config = tmp.path().join(".codex/config.toml");
    write(
        &config,
        "model = \"gpt-5\"\n\n[plugins.\"off@symposium\"]\nenabled = false\n\n[plugins.\"gone@symposium\"]\nenabled = true\n\n[plugins.\"theirs@team\"]\nenabled = true\n",
    );

    assert!(deliver_user_plugins(&sym, &[compiled(&staging, "fresh")]).is_some());

    let written = fs::read_to_string(&config).unwrap();
    let source = value(staging.display().to_string()).to_string();
    expect![[r#"
        model = "gpt-5"

        [plugins."off@symposium"]
        enabled = false

        [plugins."theirs@team"]
        enabled = true

        [plugins."fresh@symposium"]
        enabled = true

        [marketplaces.symposium]
        source_type = "local"
        source = "$ROOT"
    "#]]
    .assert_eq(&written.replace(source.trim(), r#""$ROOT""#));

    deliver_user_plugins(
        &sym,
        &[compiled(&staging, "fresh"), compiled(&staging, "off")],
    );
    assert_eq!(fs::read_to_string(&config).unwrap(), written);
}

#[test]
fn another_symposium_marketplace_is_left_alone_and_nothing_is_delivered() {
    let tmp = tempfile::tempdir().unwrap();
    let sym = Symposium::from_dir(tmp.path());
    let config = tmp.path().join(".codex/config.toml");
    let theirs = "[marketplaces.symposium]\nsource_type = \"git\"\nsource = \"https://example.com/tools.git\"\n";
    write(&config, theirs);

    let staging = sym.config_dir().join(INSTALLED_PLUGINS_SUBDIR);
    assert!(deliver_user_plugins(&sym, &[compiled(&staging, "fresh")]).is_none());

    assert_eq!(fs::read_to_string(&config).unwrap(), theirs);
    assert!(!tmp.path().join(".codex/plugins").exists());
}

#[test]
fn an_unparseable_config_is_left_untouched_and_nothing_is_delivered() {
    let tmp = tempfile::tempdir().unwrap();
    let sym = Symposium::from_dir(tmp.path());
    let config = tmp.path().join(".codex/config.toml");
    write(&config, "[[broken");

    let staging = sym.config_dir().join(INSTALLED_PLUGINS_SUBDIR);
    assert!(deliver_user_plugins(&sym, &[compiled(&staging, "fresh")]).is_none());

    assert_eq!(fs::read_to_string(&config).unwrap(), "[[broken");
    assert!(!tmp.path().join(".codex/plugins").exists());
}

#[test]
fn nothing_is_written_when_there_is_nothing_to_deliver() {
    let tmp = tempfile::tempdir().unwrap();
    let sym = Symposium::from_dir(tmp.path());

    assert!(deliver_user_plugins(&sym, &[]).is_some());

    assert!(!tmp.path().join(".codex").exists());
}
