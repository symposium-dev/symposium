fn main() {
    // cargo-agents-testlib lives at <workspace>/cargo-agents-testlib/
    // fixtures live at <workspace>/tests/fixtures/
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let workspace_root = std::path::Path::new(&manifest_dir)
        .parent()
        .expect("cargo-agents-testlib must be in a workspace subdirectory");
    let fixtures_dir = workspace_root.join("tests").join("fixtures");
    println!(
        "cargo:rustc-env=CARGO_AGENTS_FIXTURES_DIR={}",
        fixtures_dir.display()
    );
}
