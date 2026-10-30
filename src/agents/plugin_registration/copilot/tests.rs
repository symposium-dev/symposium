use super::*;

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

const ROOT: &str = "/home/user/.symposium/installed";

fn compiled(names: &[&str]) -> Vec<CompiledPlugin> {
    names
        .iter()
        .map(|name| CompiledPlugin {
            name: name.to_string(),
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
fn register_changes_only_what_symposium_owns() {
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
            "keeps the user's keys and every false",
            json!({
                "hooks": { "sessionStart": [] },
                "enabledPlugins": {
                    "fresh@symposium": false,
                    "declined@symposium": false,
                    "gone@symposium": true,
                    "gone@symposium-dev": true,
                    "tool@market": true,
                },
                "extraKnownMarketplaces": {
                    "market": github(),
                    "symposium": { "source": ours()["source"], "autoUpdate": true },
                },
            }),
            &["fresh"][..],
            json!({
                "hooks": { "sessionStart": [] },
                "enabledPlugins": {
                    "fresh@symposium": false,
                    "declined@symposium": false,
                    "gone@symposium-dev": true,
                    "tool@market": true,
                },
                "extraKnownMarketplaces": {
                    "market": github(),
                    "symposium": { "source": ours()["source"], "autoUpdate": true },
                },
            }),
        ),
        (
            "an empty set unregisters",
            json!({
                "enabledPlugins": { "fresh@symposium": true, "declined@symposium": false },
                "extraKnownMarketplaces": { "symposium": ours() },
            }),
            &[][..],
            json!({
                "enabledPlugins": { "declined@symposium": false },
                "extraKnownMarketplaces": {},
            }),
        ),
    ];
    for (name, mut settings, plugins, expected) in cases {
        register(&mut settings, Path::new(ROOT), &compiled(plugins)).unwrap();
        assert_eq!(settings, expected, "{name}");
    }

    let moved = json!({ "source": { "source": "directory", "path": "/old/installed" } });
    for entry in [github(), moved] {
        let mut settings = json!({ "extraKnownMarketplaces": { "symposium": entry } });
        let before = settings.clone();
        assert!(register(&mut settings, Path::new(ROOT), &compiled(&["fresh"])).is_err());
        assert_eq!(settings, before);
    }
}

#[test]
fn files_are_written_only_when_something_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("installed");
    let settings = tmp.path().join("copilot/settings.json");

    deliver(&root, &settings, &[]).unwrap();
    assert!(!settings.exists(), "nothing to remove creates no file");

    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(&settings, r#"{"model":"gpt-6"}"#).unwrap();
    deliver(&root, &settings, &[]).unwrap();
    assert_eq!(
        fs::read_to_string(&settings).unwrap(),
        r#"{"model":"gpt-6"}"#,
        "a file with nothing of symposium's is not reformatted"
    );

    deliver(&root, &settings, &compiled(&["fresh"])).unwrap();
    let an_hour_ago = SystemTime::now() - Duration::from_secs(3600);
    let written: [&PathBuf; 1] = [&settings];
    for path in written {
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(an_hour_ago)
            .unwrap();
    }
    deliver(&root, &settings, &compiled(&["fresh"])).unwrap();
    for path in written {
        assert_eq!(fs::metadata(path).unwrap().modified().unwrap(), an_hour_ago);
    }
}
