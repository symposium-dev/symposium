use super::*;

use std::fs;
use std::time::{Duration, SystemTime};

const ROOT: &str = "/home/user/.symposium/installed";

const COPILOT_1_0_89_INSTALL: &str = r#"{
  "extraKnownMarketplaces": {
    "symposium": {
      "source": {
        "source": "directory",
        "path": "ROOT"
      }
    }
  },
  "enabledPlugins": {
    "probe@symposium": true
  }
}"#;

fn compiled(names: &[&str]) -> Vec<CompiledPlugin> {
    names
        .iter()
        .map(|name| CompiledPlugin {
            name: name.to_string(),
            version: "0.0.0".into(),
            dir: Path::new(ROOT).join(name),
        })
        .collect()
}

fn ours() -> Value {
    json!({ "source": { "source": "directory", "path": ROOT } })
}

fn github() -> Value {
    json!({ "source": { "source": "github", "repo": "someone/tools" } })
}

#[test]
fn reconcile_writes_only_what_symposium_owns() {
    let cases = [
        (
            "registers into empty settings",
            json!({}),
            &["fresh"][..],
            json!({
                "enabledPlugins": { "fresh@symposium": true },
                "extraKnownMarketplaces": { "symposium": ours() },
            }),
        ),
        (
            "keeps every key of the user's",
            json!({
                "model": "gpt-6",
                "hooks": { "sessionStart": [{ "type": "command", "bash": "x" }] },
                "enabledPlugins": { "tool@market": true },
                "extraKnownMarketplaces": { "market": github() },
            }),
            &["fresh"][..],
            json!({
                "model": "gpt-6",
                "hooks": { "sessionStart": [{ "type": "command", "bash": "x" }] },
                "enabledPlugins": { "tool@market": true, "fresh@symposium": true },
                "extraKnownMarketplaces": { "market": github(), "symposium": ours() },
            }),
        ),
        (
            "keeps a user's false",
            json!({
                "enabledPlugins": { "fresh@symposium": false },
                "extraKnownMarketplaces": { "symposium": ours() },
            }),
            &["fresh"][..],
            json!({
                "enabledPlugins": { "fresh@symposium": false },
                "extraKnownMarketplaces": { "symposium": ours() },
            }),
        ),
        (
            "removes only its own stale entries",
            json!({
                "enabledPlugins": {
                    "gone@symposium": true,
                    "declined@symposium": false,
                    "gone@symposium-dev": true,
                    "gone@market": true,
                },
                "extraKnownMarketplaces": { "symposium": ours() },
            }),
            &["fresh"][..],
            json!({
                "enabledPlugins": {
                    "declined@symposium": false,
                    "gone@symposium-dev": true,
                    "gone@market": true,
                    "fresh@symposium": true,
                },
                "extraKnownMarketplaces": { "symposium": ours() },
            }),
        ),
        (
            "keeps the user's fields on its own entry",
            json!({
                "enabledPlugins": { "fresh@symposium": true },
                "extraKnownMarketplaces": { "symposium": { "source": ours()["source"], "autoUpdate": true } },
            }),
            &["fresh"][..],
            json!({
                "enabledPlugins": { "fresh@symposium": true },
                "extraKnownMarketplaces": { "symposium": { "source": ours()["source"], "autoUpdate": true } },
            }),
        ),
        (
            "an empty set removes the registration",
            json!({
                "enabledPlugins": { "fresh@symposium": true, "tool@market": true },
                "extraKnownMarketplaces": { "symposium": ours(), "market": github() },
            }),
            &[][..],
            json!({
                "enabledPlugins": { "tool@market": true },
                "extraKnownMarketplaces": { "market": github() },
            }),
        ),
        (
            "an empty set drops the containers it emptied",
            json!({
                "enabledPlugins": { "fresh@symposium": true },
                "extraKnownMarketplaces": { "symposium": ours() },
            }),
            &[][..],
            json!({}),
        ),
        (
            "an empty set keeps a user's false and empty containers",
            json!({
                "enabledPlugins": { "fresh@symposium": false },
                "extraKnownMarketplaces": {},
            }),
            &[][..],
            json!({
                "enabledPlugins": { "fresh@symposium": false },
                "extraKnownMarketplaces": {},
            }),
        ),
    ];
    for (name, mut settings, plugins, expected) in cases {
        let changes = reconcile(&mut settings, Path::new(ROOT), &compiled(plugins)).unwrap();
        assert!(changes.is_some(), "{name}: treated as foreign");
        assert_eq!(settings, expected, "{name}");
    }
}

#[test]
fn a_symposium_marketplace_pointing_elsewhere_is_left_alone() {
    let elsewhere = json!({ "source": { "source": "directory", "path": "/old/installed" } });
    for entry in [github(), elsewhere] {
        for plugins in [&["fresh"][..], &[]] {
            let mut settings = json!({
                "enabledPlugins": { "gone@symposium": true },
                "extraKnownMarketplaces": { "symposium": entry.clone() },
            });
            let before = settings.clone();
            let changes = reconcile(&mut settings, Path::new(ROOT), &compiled(plugins)).unwrap();
            assert!(changes.is_none(), "{entry} with {plugins:?} is foreign");
            assert_eq!(settings, before);
        }
    }
}

#[test]
fn settings_of_the_wrong_shape_are_an_error() {
    for mut settings in [
        json!([]),
        json!({ "enabledPlugins": [] }),
        json!({ "extraKnownMarketplaces": "symposium" }),
    ] {
        assert!(
            reconcile(&mut settings, Path::new(ROOT), &compiled(&["fresh"])).is_err(),
            "{settings}"
        );
    }
}

#[test]
fn the_settings_file_follows_copilot_home() {
    let tmp = tempfile::tempdir().unwrap();
    let mut sym = Symposium::from_dir(tmp.path());
    let default = tmp.path().join(".copilot/settings.json");
    assert_eq!(settings_path(&sym), default);

    sym.set_env(|name| (name == "COPILOT_HOME").then(|| "/elsewhere/copilot".into()));
    assert_eq!(
        settings_path(&sym),
        Path::new("/elsewhere/copilot/settings.json")
    );

    sym.set_env(|name| (name == "COPILOT_HOME").then(|| "".into()));
    assert_eq!(
        settings_path(&sym),
        default,
        "an empty value does not relocate"
    );
}

#[test]
fn the_registration_matches_what_copilot_plugin_install_writes() {
    let tmp = tempfile::tempdir().unwrap();
    let sym = Symposium::from_dir(tmp.path());
    let root = tmp.path().join("installed");

    let delivered = sync_user_plugins(
        &sym,
        &root,
        &[CompiledPlugin {
            name: "probe".into(),
            version: "0.0.0".into(),
            dir: root.join("probe"),
        }],
        &Output::quiet(),
    )
    .unwrap();

    assert!(delivered);
    let written: Value =
        serde_json::from_str(&fs::read_to_string(settings_path(&sym)).unwrap()).unwrap();
    let mut truth: Value = serde_json::from_str(COPILOT_1_0_89_INSTALL).unwrap();
    truth[MARKETPLACES_KEY][MARKETPLACE]["source"]["path"] = json!(root.display().to_string());
    assert_eq!(written, truth);
}

#[test]
fn the_settings_file_is_written_only_when_something_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let sym = Symposium::from_dir(tmp.path());
    let path = settings_path(&sym);
    let out = Output::quiet();
    let root = Path::new(ROOT);

    assert!(sync_user_plugins(&sym, root, &[], &out).unwrap());
    assert!(!path.exists(), "nothing to remove creates no file");

    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let users_own = r#"{"model":"gpt-6"}"#;
    fs::write(&path, users_own).unwrap();
    sync_user_plugins(&sym, root, &[], &out).unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        users_own,
        "a file with nothing of symposium's is not reformatted"
    );

    sync_user_plugins(&sym, root, &compiled(&["fresh"]), &out).unwrap();
    let an_hour_ago = SystemTime::now() - Duration::from_secs(3600);
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(an_hour_ago)
        .unwrap();
    sync_user_plugins(&sym, root, &compiled(&["fresh"]), &out).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().modified().unwrap(),
        an_hour_ago
    );
}

#[test]
fn unreadable_settings_are_an_error_and_stay_untouched() {
    let tmp = tempfile::tempdir().unwrap();
    let sym = Symposium::from_dir(tmp.path());
    let path = settings_path(&sym);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    for contents in [
        "{",
        "// comments are not JSON\n{}",
        "[]",
        r#"{"enabledPlugins": []}"#,
    ] {
        fs::write(&path, contents).unwrap();
        let result = sync_user_plugins(
            &sym,
            Path::new(ROOT),
            &compiled(&["fresh"]),
            &Output::quiet(),
        );
        assert!(result.is_err(), "{contents:?}");
        assert_eq!(fs::read_to_string(&path).unwrap(), contents);
    }
}

#[test]
fn hooks_and_plugins_share_the_settings_file_in_either_order() {
    let registered = |plugins_first: bool| {
        let tmp = tempfile::tempdir().unwrap();
        let sym = Symposium::from_dir(tmp.path());
        let path = settings_path(&sym);
        let out = Output::quiet();
        let register_plugins = || {
            sync_user_plugins(&sym, Path::new(ROOT), &compiled(&["fresh"]), &out).unwrap();
        };
        if plugins_first {
            register_plugins();
            crate::agents::register_copilot_hooks_global(&path, &out).unwrap();
        } else {
            crate::agents::register_copilot_hooks_global(&path, &out).unwrap();
            register_plugins();
        }
        serde_json::from_str::<Value>(&fs::read_to_string(&path).unwrap()).unwrap()
    };

    let settings = registered(true);
    assert_eq!(settings, registered(false));
    assert!(settings["hooks"]["sessionStart"].is_array(), "{settings}");
    assert_eq!(settings[ENABLED_KEY]["fresh@symposium"], json!(true));
    assert_eq!(settings[MARKETPLACES_KEY][MARKETPLACE], ours());
}
