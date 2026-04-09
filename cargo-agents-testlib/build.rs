fn main() {
    // symposium-testlib lives at <workspace>/symposium-testlib/
    // fixtures live at <workspace>/tests/fixtures/
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let workspace_root = std::path::Path::new(&manifest_dir)
        .parent()
        .expect("symposium-testlib must be in a workspace subdirectory");
    let fixtures_dir = workspace_root.join("tests").join("fixtures");
    println!(
        "cargo:rustc-env=SYMPOSIUM_FIXTURES_DIR={}",
        fixtures_dir.display()
    );
}
