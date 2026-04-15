//! Integration test harness for symposium.
//!
//! Provides `TestContext` (wrapping `Symposium::from_dir()`) and `with_fixture()`
//! for composable, isolated test environments.

use std::path::{Path, PathBuf};

use clap::Parser;

use symposium::cli::Cli;
use symposium::config::Symposium;
use symposium::dispatch::{self, DispatchResult};
use symposium::hook::{self, HookOutput, HookPayload};
use symposium::mcp::McpArgs;
use symposium::output::Output;

/// Test context wrapping an isolated `Symposium` instance.
pub struct TestContext {
    pub sym: Symposium,
    /// The temporary directory (kept alive for the test's duration).
    pub _tempdir: tempfile::TempDir,
    /// Root of the overlaid workspace (if a workspace fixture was included).
    pub workspace_root: Option<PathBuf>,
}

impl TestContext {
    /// Run a `symposium` command against this test context.
    ///
    /// Args are the same as CLI args after `symposium`, e.g.:
    /// ```ignore
    /// ctx.symposium(&["init", "--user", "--agent", "claude"]).await.unwrap();
    /// ctx.symposium(&["sync", "--workspace"]).await.unwrap();
    /// ```
    ///
    /// Uses the fixture's config dir as the user-global config and the
    /// workspace root (if present) as the working directory. Output is
    /// suppressed (quiet mode).
    pub async fn symposium(&mut self, args: &[&str]) -> anyhow::Result<()> {
        // Build full arg list with program name for clap
        let mut full_args = vec!["symposium"];
        full_args.push("-q"); // always quiet in tests
        full_args.extend_from_slice(args);

        let cli = Cli::try_parse_from(&full_args)
            .map_err(|e| anyhow::anyhow!("failed to parse args: {e}"))?;

        let out = Output::quiet();
        let cwd = self
            .workspace_root
            .clone()
            .unwrap_or_else(|| self.sym.config_dir().to_path_buf());

        match cli.command {
            Some(cmd) => symposium::cli::run(&mut self.sym, cmd, &cwd, &out).await,
            None => Ok(()),
        }
    }

    /// Call the shared dispatch function, returning the output string.
    ///
    /// Args are parsed via Clap just as the MCP server would.
    pub async fn invoke(&self, args: &[&str]) -> Result<String, String> {
        let parsed =
            McpArgs::try_parse_from(args).map_err(|e| format!("failed to parse args: {e}"))?;
        let cwd = self
            .workspace_root
            .as_deref()
            .unwrap_or_else(|| self.sym.config_dir());
        match dispatch::dispatch(&self.sym, parsed.command, cwd, dispatch::RenderMode::Mcp).await {
            DispatchResult::Ok(output) => Ok(output),
            DispatchResult::Err(e) => Err(e),
        }
    }

    /// Call the built-in hook logic with a typed payload, returning typed output.
    pub async fn invoke_hook(&self, payload: impl Into<HookPayload>) -> HookOutput {
        hook::dispatch_builtin(&self.sym, &payload.into()).await
    }

    /// Replace temp directory paths with a stable placeholder for snapshot tests.
    pub fn normalize_paths(&self, output: &str) -> String {
        let config_dir = self.sym.config_dir().to_string_lossy().to_string();
        output.replace(&config_dir, "$CONFIG_DIR")
    }
}

/// Directories discovered while copying fixture files.
struct FixtureScanResult {
    /// Directories (relative to tempdir root) containing a `config.toml`.
    config_dirs: Vec<PathBuf>,
    /// Directories (relative to tempdir root) containing a `Cargo.toml`.
    workspace_dirs: Vec<PathBuf>,
}

/// Create a test context by overlaying fixture fragments into a tempdir.
///
/// Each fixture name corresponds to a subdirectory under `tests/fixtures/`
/// in the symposium workspace. Files are copied in order, so later fixtures
/// override earlier ones.
///
/// After copying, the function reports which directories contain
/// `config.toml` and `Cargo.toml`. The `config.toml` directory becomes
/// the Symposium config dir; the `Cargo.toml` directory becomes the
/// workspace root. Panics if multiple of either are found.
///
/// # Example
///
/// ```text
/// fixtures/
///     plugins0/
///         dot-symposium/
///             config.toml
///             plugins/my-skill/SKILL.md
///     workspace0/
///         Cargo.toml
///         src/lib.rs
///
/// with_fixture(&["plugins0", "workspace0"])
///
/// $tmpdir/
///     dot-symposium/                <-- sym.config_dir()
///         config.toml               <-- from plugins0
///         plugins/my-skill/SKILL.md <-- from plugins0
///     Cargo.toml                    <-- from workspace0, workspace_root = $tmpdir
///     src/lib.rs                    <-- from workspace0
/// ```
pub fn with_fixture(fixtures: &[&str]) -> TestContext {
    let fixtures_base = Path::new(env!("SYMPOSIUM_FIXTURES_DIR"));
    let tempdir = tempfile::tempdir().expect("failed to create tempdir");
    let root = tempdir.path();

    let mut scan = FixtureScanResult {
        config_dirs: Vec::new(),
        workspace_dirs: Vec::new(),
    };

    for fixture_name in fixtures {
        let fixture_dir = fixtures_base.join(fixture_name);
        assert!(
            fixture_dir.is_dir(),
            "fixture not found: {}",
            fixture_dir.display()
        );
        copy_dir_recursive(&fixture_dir, root, &mut scan);
    }

    // Resolve config dir
    assert!(
        scan.config_dirs.len() <= 1,
        "multiple config.toml found in fixtures: {:?}",
        scan.config_dirs
    );
    let config_dir = scan
        .config_dirs
        .first()
        .cloned()
        .unwrap_or_else(|| root.to_path_buf());

    // Resolve workspace root
    assert!(
        scan.workspace_dirs.len() <= 1,
        "multiple Cargo.toml found in fixtures: {:?}",
        scan.workspace_dirs
    );
    let workspace_root = scan.workspace_dirs.first().cloned();

    let sym = Symposium::from_dir(&config_dir);

    TestContext {
        sym,
        _tempdir: tempdir,
        workspace_root,
    }
}

/// Recursively copy a directory tree, tracking special directories.
///
/// - A `config.toml` inside a `dot-symposium/` directory marks the
///   user config dir (the Symposium config root).
/// - A `Cargo.toml` marks the workspace root.
/// - Other `config.toml` files (e.g., `.symposium/config.toml` inside
///   the workspace) are copied but not treated as user config.
fn copy_dir_recursive(src: &Path, dst: &Path, scan: &mut FixtureScanResult) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path, scan);
        } else {
            std::fs::copy(&src_path, &dst_path).unwrap();

            let filename = entry.file_name();
            if filename == "config.toml" {
                // Only treat as user config dir if parent is dot-symposium
                let is_user_config = dst.file_name().is_some_and(|n| n == "dot-symposium");
                if is_user_config {
                    scan.config_dirs.push(dst.to_path_buf());
                }
            } else if filename == "Cargo.toml" {
                scan.workspace_dirs.push(dst.to_path_buf());
            }
        }
    }
}
