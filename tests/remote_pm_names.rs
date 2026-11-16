//! Moving Cargo out of process must preserve Cargo's package-name equivalence.

use std::path::Path;

use symposium::config::{PackageManagerConfig, Symposium};
use symposium::discovery::Names;
use symposium::pm::PackageId;

fn configured_symposium(dir: &Path) -> Symposium {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("config.toml"),
        indoc::indoc! {r#"
            [defaults]
            symposium-recommendations = false
            user-plugins = false

            [plugins]
            auto-enable = ["*"]
            disable = ["widget_lib"]
        "#},
    )
    .unwrap();
    Symposium::from_dir(dir)
}

fn replace_cargo_with_binary(sym: &mut Symposium) {
    sym.config.package_managers.push(PackageManagerConfig {
        name: "cargo".into(),
        command: env!("CARGO_BIN_EXE_symposium-pm-cargo").into(),
        args: vec![],
    });
}

#[tokio::test]
async fn configured_cargo_preserves_disable_name_equivalence() {
    let tmp = tempfile::tempdir().unwrap();
    let mut sym = configured_symposium(tmp.path());
    let id = PackageId::new("cargo", "widget-lib", "0.1.0");
    let builtin = sym.detached_managers();
    let deps = [id.clone()];
    let spellings = Names::new(&[], &deps, &builtin)
        .spellings(&sym.config.plugins)
        .await;
    assert!(sym.config.plugins.is_disabled(&id, &spellings));

    replace_cargo_with_binary(&mut sym);
    let remote = sym.detached_managers();
    let spellings = Names::new(&[], &deps, &remote)
        .spellings(&sym.config.plugins)
        .await;
    assert!(
        sym.config.plugins.is_auto_enabled(&id, &deps, &spellings),
        "the wildcard enables the package unless disable takes precedence"
    );
    assert!(
        sym.config.plugins.is_disabled(&id, &spellings),
        "running Cargo out of process must not let widget-lib escape disable = [widget_lib]"
    );
}

#[tokio::test]
async fn configured_cargo_identifies_an_underscore_alias() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("project");
    let dependency = tmp.path().join("widget-lib");
    for dir in [&root, &dependency] {
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "").unwrap();
    }
    std::fs::write(
        root.join("Cargo.toml"),
        indoc::indoc! {r#"
            [package]
            name = "host"
            version = "0.1.0"
            edition = "2021"

            [dependencies]
            widget-lib = { path = "../widget-lib" }
        "#},
    )
    .unwrap();
    std::fs::write(
        dependency.join("Cargo.toml"),
        "[package]\nname = \"widget-lib\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    let mut sym = configured_symposium(&tmp.path().join("config"));
    let builtin = sym.workspace(&root);
    let expected = Names::new(&[], builtin.dep_ids().await, builtin.pms())
        .nearest("widget_lib", Some("cargo"))
        .await;
    assert_eq!(
        expected.len(),
        1,
        "the built-in Cargo PM recognizes the alias"
    );

    replace_cargo_with_binary(&mut sym);
    let remote = sym.workspace(&root);
    let names = Names::new(&[], remote.dep_ids().await, remote.pms());
    assert_eq!(names.nearest("widget-lib", Some("cargo")).await, expected);
    let actual = names.nearest("widget_lib", Some("cargo")).await;
    assert_eq!(actual, expected);
}
