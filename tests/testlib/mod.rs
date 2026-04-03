//! Integration test harness for Symposium.
//!
//! Provides `TestContext` (wrapping `Symposium::from_dir()`) and `with_fixture()`
//! for composable, isolated test environments.

use std::path::{Path, PathBuf};

use symposium::config::Symposium;
use symposium::dispatch::{self, DispatchResult};
use symposium::hook::{self, HookOutput, HookPayload};

/// Test context wrapping an isolated `Symposium` instance.
pub struct TestContext {
    pub sym: Symposium,
    /// The temporary directory (kept alive for the test's duration).
    _tempdir: tempfile::TempDir,
    /// Root of the overlaid workspace (if a workspace fixture was included).
    workspace_root: Option<PathBuf>,
}

impl TestContext {
    /// Call the shared dispatch function, returning the output string.
    pub async fn invoke(&self, args: &[&str]) -> Result<String, String> {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let cwd = self
            .workspace_root
            .as_deref()
            .unwrap_or_else(|| self.sym.config_dir());
        match dispatch::dispatch(&self.sym, &args, cwd).await {
            DispatchResult::Ok(output) => Ok(output),
            DispatchResult::Err(e) => Err(e),
        }
    }

    /// Call the built-in hook logic with a typed payload, returning typed output.
    pub async fn invoke_hook(&self, payload: &HookPayload) -> HookOutput {
        hook::dispatch_builtin(&self.sym, payload).await
    }

    /// Path to the workspace root (if a workspace fixture was included).
    pub fn workspace_root(&self) -> Option<&Path> {
        self.workspace_root.as_deref()
    }
}

/// Create a test context by overlaying fixture fragments into a tempdir.
///
/// Each fixture name corresponds to a directory under `tests/fixtures/`.
/// Files are copied in order, so later fixtures override earlier ones.
///
/// If any fixture contains a `Cargo.toml`, its path is recorded as the workspace root.
pub fn with_fixture(fixtures: &[&str]) -> TestContext {
    let tempdir = tempfile::tempdir().expect("failed to create tempdir");
    let root = tempdir.path();

    let fixtures_base = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut workspace_root: Option<PathBuf> = None;

    for fixture_name in fixtures {
        let fixture_dir = fixtures_base.join(fixture_name);
        assert!(
            fixture_dir.is_dir(),
            "fixture not found: {}",
            fixture_dir.display()
        );
        copy_dir_recursive(&fixture_dir, root);

        // Detect workspace root
        if fixture_dir.join("Cargo.toml").exists() {
            workspace_root = Some(root.to_path_buf());
        }
    }

    let sym = Symposium::from_dir(root);

    TestContext {
        sym,
        _tempdir: tempdir,
        workspace_root,
    }
}

/// Recursively copy a directory tree, overwriting existing files.
fn copy_dir_recursive(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path);
        } else {
            std::fs::copy(&src_path, &dst_path).unwrap();
        }
    }
}
