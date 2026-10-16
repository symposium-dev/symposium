use super::*;

#[test]
fn the_user_plugins_directory_follows_goose_path_root_only() {
    let tmp = tempfile::tempdir().unwrap();
    let mut sym = Symposium::from_dir(tmp.path());
    let default = tmp.path().join(".agents/plugins");
    assert_eq!(user_plugins_dir(&sym), default);

    sym.set_env(|name| (name == "GOOSE_PATH_ROOT").then(|| "/elsewhere/goose".into()));
    assert_eq!(
        user_plugins_dir(&sym),
        Path::new("/elsewhere/goose/.agents/plugins")
    );

    sym.set_env(|name| (name == "GOOSE_PATH_ROOT").then(|| "".into()));
    assert_eq!(
        user_plugins_dir(&sym),
        default,
        "an empty value does not relocate"
    );

    sym.set_env(|name| (name == "XDG_CONFIG_HOME").then(|| "/elsewhere/xdg".into()));
    assert_eq!(user_plugins_dir(&sym), default);
}
