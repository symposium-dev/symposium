use std::fs;
use std::path::Path;

use fixture_store::Store;

#[test]
fn settings_survive_reopen() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("settings_survive_reopen");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    store_durability::save_settings(&dir, &[("theme", "dark"), ("language", "en")]).unwrap();

    let reopened = Store::open(&dir).unwrap();
    assert_eq!(reopened.get("theme"), Some("dark"));
    assert_eq!(reopened.get("language"), Some("en"));
}
