//! Fetch crate sources by running `cargo fetch` in a temporary dummy package.
//!
//! This module avoids hitting `crates.io` HTTP endpoints directly. Instead, it
//! creates a throwaway package that depends on the target crate, runs
//! `cargo fetch` to populate cargo's registry cache, and then reads
//! `cargo metadata` to locate the extracted source directory.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use cargo_metadata::MetadataCommand;

use super::{FetchResult, normalize_crate_name};

/// Fetch a crate via a temporary dummy cargo package.
///
/// `version_req` may be any valid Cargo version requirement (e.g. `"=1.2.3"`,
/// `"^1.0"`, `"*"`).
pub async fn fetch_via_cargo(crate_name: &str, version_req: &str) -> Result<FetchResult> {
    let crate_name = crate_name.to_string();
    let version_req = version_req.to_string();

    // The work is blocking (subprocess + filesystem + metadata parsing), so run
    // it on the blocking pool to avoid stalling the async runtime.
    tokio::task::spawn_blocking(move || fetch_sync(&crate_name, &version_req))
        .await
        .context("cargo probe task panicked")?
}

fn fetch_sync(crate_name: &str, version_req: &str) -> Result<FetchResult> {
    tracing::debug!(%crate_name, %version_req, "fetching crate via cargo probe");

    let temp = tempfile::Builder::new()
        .prefix("symposium-crate-probe-")
        .tempdir()
        .context("failed to create temp directory for cargo probe")?;

    write_dummy_package(temp.path(), crate_name, version_req)?;
    run_cargo_fetch(temp.path(), crate_name, version_req)?;
    find_crate_in_metadata(temp.path(), crate_name)
}

/// Write a minimal package at `dir` that depends on `crate_name = version_req`.
fn write_dummy_package(dir: &Path, crate_name: &str, version_req: &str) -> Result<()> {
    std::fs::create_dir_all(dir.join("src"))?;
    std::fs::write(dir.join("src/lib.rs"), "")?;

    // The `[workspace]` table makes this package its own workspace root so
    // that cargo does not try to attach it to a parent workspace if the
    // tempdir happens to be under one.
    let cargo_toml = format!(
        r#"[package]
name = "symposium-crate-probe"
version = "0.0.0"
edition = "2021"

[lib]
path = "src/lib.rs"

[dependencies]
{crate_name} = {{ version = "{version_req}" }}

[workspace]
"#
    );
    std::fs::write(dir.join("Cargo.toml"), cargo_toml)?;
    Ok(())
}

/// Run `cargo fetch` against the manifest at `dir/Cargo.toml`.
fn run_cargo_fetch(dir: &Path, crate_name: &str, version_req: &str) -> Result<()> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(&cargo)
        .arg("fetch")
        .arg("--manifest-path")
        .arg(dir.join("Cargo.toml"))
        .output()
        .with_context(|| format!("failed to invoke `{}`", cargo.to_string_lossy()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        bail!("cargo fetch failed for `{crate_name} = \"{version_req}\"`: {stderr}");
    }
    Ok(())
}

/// Look up the package for `crate_name` in the metadata for `dir` and return
/// its source path.
fn find_crate_in_metadata(dir: &Path, crate_name: &str) -> Result<FetchResult> {
    let metadata = MetadataCommand::new()
        .current_dir(dir)
        .exec()
        .context("failed to run cargo metadata")?;

    // Cargo normalizes hyphens/underscores in crate names; match loosely so
    // that a user query of `serde-json` finds the `serde_json` package (or
    // vice-versa).
    let normalized = normalize_crate_name(crate_name);
    let package = metadata
        .packages
        .iter()
        .find(|p| normalize_crate_name(&p.name) == normalized)
        .ok_or_else(|| anyhow::anyhow!("crate '{crate_name}' not found in cargo metadata"))?;

    let manifest_path: PathBuf = package.manifest_path.clone().into();
    let path = manifest_path
        .parent()
        .ok_or_else(|| {
            anyhow::anyhow!("manifest path `{}` has no parent", manifest_path.display())
        })?
        .to_path_buf();

    Ok(FetchResult {
        name: package.name.to_string(),
        version: package.version.to_string(),
        path,
    })
}
