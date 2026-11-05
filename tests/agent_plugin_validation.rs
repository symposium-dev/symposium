use std::path::PathBuf;
use std::process::{Command, Output};

/// The JSON identity is sufficient to load this package through a registry;
/// selecting a different supported validation path must not change that.
fn validate(relative_path: &str) -> Output {
    validate_from(relative_path, false)
}

fn validate_from(relative_path: &str, from_package: bool) -> Output {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let config = home.join(".symposium");
    let package = temp.path().join("plugins/portable");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(
        config.join("config.toml"),
        "[defaults]\nsymposium-recommendations = false\nuser-plugins = false\n",
    )
    .unwrap();
    std::fs::write(
        package.join("plugin.json"),
        r#"{
            "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
            "name": "portable-tools"
        }"#,
    )
    .unwrap();
    std::fs::write(package.join("Symposium.toml"), "").unwrap();

    Command::new(env!("CARGO_BIN_EXE_cargo-agents"))
        .args(["plugin", "validate", "--no-check-crates"])
        .arg(if from_package {
            PathBuf::from(relative_path)
        } else {
            temp.path().join(relative_path)
        })
        .current_dir(if from_package { &package } else { temp.path() })
        .env("SYMPOSIUM_HOME", config)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .output()
        .expect("run plugin validate")
}

fn assert_valid(output: &Output) {
    assert!(
        output.status.success(),
        "the package has a valid JSON identity and should validate: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn registry_parent_validates_a_toml_plugin_with_json_identity() {
    let output = validate("plugins");
    assert_valid(&output);
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("portable-tools"),
        "the registry scan must actually find the package: {output:?}"
    );
}

#[test]
fn direct_toml_validation_uses_json_identity_fallback() {
    assert_valid(&validate("plugins/portable/Symposium.toml"));
}

#[test]
fn bare_toml_filename_validation_uses_json_identity_fallback() {
    // Both names select the same manifest from the package directory. Keep
    // the `./` spelling as a passing control for the literal bare filename.
    assert_valid(&validate_from("./Symposium.toml", true));
    assert_valid(&validate_from("Symposium.toml", true));
}

#[test]
fn package_directory_validation_accepts_both_manifests() {
    assert_valid(&validate("plugins/portable"));
}

#[test]
fn direct_json_validation_accepts_a_package_with_toml() {
    assert_valid(&validate("plugins/portable/plugin.json"));
}
