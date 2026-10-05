//! Fetch crate sources by running `cargo fetch` in a temporary dummy package.
//!
//! This module avoids hitting `crates.io` HTTP endpoints directly. Instead, it
//! creates a throwaway package that depends on the target crate, runs
//! `cargo fetch` to populate cargo's registry cache, and then reads
//! `cargo metadata` (or, for a binary-only crate, the probe's `Cargo.lock`) to
//! locate the extracted source directory.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use cargo_metadata::MetadataCommand;

use super::{FetchResult, normalize_crate_name};

const PROBE_PACKAGE: &str = "symposium-crate-probe";

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
    match find_crate_in_metadata(temp.path(), crate_name)? {
        Some(found) => Ok(found),
        None => {
            let cargo_home = home::cargo_home().context("failed to locate cargo home")?;
            find_crate_in_lockfile(temp.path(), crate_name, &cargo_home)
        }
    }
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
name = "{PROBE_PACKAGE}"
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
fn find_crate_in_metadata(dir: &Path, crate_name: &str) -> Result<Option<FetchResult>> {
    let metadata = MetadataCommand::new()
        .current_dir(dir)
        .exec()
        .context("failed to run cargo metadata")?;

    // Cargo normalizes hyphens/underscores in crate names; match loosely so
    // that a user query of `serde-json` finds the `serde_json` package (or
    // vice-versa).
    let normalized = normalize_crate_name(crate_name);
    let Some(package) = metadata
        .packages
        .iter()
        .find(|p| normalize_crate_name(&p.name) == normalized)
    else {
        return Ok(None);
    };

    let manifest_path: PathBuf = package.manifest_path.clone().into();
    let path = manifest_path
        .parent()
        .ok_or_else(|| {
            anyhow::anyhow!("manifest path `{}` has no parent", manifest_path.display())
        })?
        .to_path_buf();

    Ok(Some(FetchResult {
        name: package.name.to_string(),
        version: package.version.to_string(),
        path,
    }))
}

/// Locate `crate_name` through the probe's `Cargo.lock` and cargo's extracted
/// registry sources.
///
/// Cargo leaves a dependency without a lib target (a binary-only crate) out of
/// `cargo metadata` entirely, but `cargo fetch` still locks and extracts it.
fn find_crate_in_lockfile(dir: &Path, crate_name: &str, cargo_home: &Path) -> Result<FetchResult> {
    let lockfile = std::fs::read_to_string(dir.join("Cargo.lock"))
        .context("failed to read the probe's Cargo.lock")?;
    let lockfile: toml::Table = toml::from_str(&lockfile).context("failed to parse Cargo.lock")?;

    let packages = lockfile
        .get("package")
        .and_then(|packages| packages.as_array())
        .map(Vec::as_slice)
        .unwrap_or_default();
    let (name, version) = locked_probe_dependency(packages, crate_name).ok_or_else(|| {
        anyhow::anyhow!(
            "crate '{crate_name}' not found in cargo metadata or the probe's Cargo.lock"
        )
    })?;

    let registry_src = cargo_home.join("registry/src");
    let entries = std::fs::read_dir(&registry_src)
        .with_context(|| format!("failed to read `{}`", registry_src.display()))?;
    let path = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path().join(format!("{name}-{version}")))
        .find(|candidate| candidate.join(".cargo-ok").is_file())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "source for `{name} {version}` not found under `{}`",
                registry_src.display()
            )
        })?;

    Ok(FetchResult {
        name: name.to_string(),
        version: version.to_string(),
        path,
    })
}

/// The `(name, version)` the probe package's dependency on `crate_name` locked
/// to.
///
/// Read through the probe's own `dependencies` entry rather than the first
/// package with a matching name: the target may also appear, at another
/// version, among its own dependencies. Cargo writes the entry as `name`, or
/// as `name version` when the name alone is ambiguous.
fn locked_probe_dependency<'a>(
    packages: &'a [toml::Value],
    crate_name: &str,
) -> Option<(&'a str, &'a str)> {
    let field = |package: &'a toml::Value, key: &str| package.get(key)?.as_str();
    let probe = packages
        .iter()
        .find(|package| field(package, "name") == Some(PROBE_PACKAGE))?;

    let normalized = normalize_crate_name(crate_name);
    let mut entry = probe
        .get("dependencies")?
        .as_array()?
        .iter()
        .filter_map(|dep| dep.as_str())
        .map(|dep| dep.split(' '))
        .find(|parts| {
            parts
                .clone()
                .next()
                .is_some_and(|name| normalize_crate_name(name) == normalized)
        })?;
    let name = entry.next()?;
    let version = match entry.next() {
        Some(version) => version,
        None => packages
            .iter()
            .find(|package| field(package, "name") == Some(name))
            .and_then(|package| field(package, "version"))?,
    };
    Some((name, version))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_lockfile(dir: &Path, body: &str) {
        std::fs::write(dir.join("Cargo.lock"), body).unwrap();
    }

    fn extracted_source(cargo_home: &Path, dir_name: &str) -> PathBuf {
        let source = cargo_home
            .join("registry/src/index.crates.io-1949cf8c6b5b557f")
            .join(dir_name);
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join(".cargo-ok"), "").unwrap();
        source
    }

    #[test]
    fn lockfile_fallback_finds_a_binary_only_crate() {
        let probe = tempfile::tempdir().unwrap();
        write_lockfile(
            probe.path(),
            indoc::indoc! {r#"
                version = 4

                [[package]]
                name = "bin_only"
                version = "0.1.1"
                source = "registry+https://github.com/rust-lang/crates.io-index"

                [[package]]
                name = "symposium-crate-probe"
                version = "0.0.0"
                dependencies = ["bin_only"]
            "#},
        );
        let cargo_home = tempfile::tempdir().unwrap();
        let source = extracted_source(cargo_home.path(), "bin_only-0.1.1");

        let found = find_crate_in_lockfile(probe.path(), "bin-only", cargo_home.path()).unwrap();
        assert_eq!(found.name, "bin_only");
        assert_eq!(found.version, "0.1.1");
        assert_eq!(found.path, source);
    }

    #[test]
    fn lockfile_fallback_picks_the_version_the_probe_depends_on() {
        let probe = tempfile::tempdir().unwrap();
        write_lockfile(
            probe.path(),
            indoc::indoc! {r#"
                version = 4

                [[package]]
                name = "tool"
                version = "1.0.0"
                source = "registry+https://github.com/rust-lang/crates.io-index"

                [[package]]
                name = "tool"
                version = "2.0.0"
                source = "registry+https://github.com/rust-lang/crates.io-index"
                dependencies = ["tool 1.0.0"]

                [[package]]
                name = "symposium-crate-probe"
                version = "0.0.0"
                dependencies = ["tool 2.0.0"]
            "#},
        );
        let cargo_home = tempfile::tempdir().unwrap();
        extracted_source(cargo_home.path(), "tool-1.0.0");
        let source = extracted_source(cargo_home.path(), "tool-2.0.0");

        let found = find_crate_in_lockfile(probe.path(), "tool", cargo_home.path()).unwrap();
        assert_eq!(found.version, "2.0.0");
        assert_eq!(found.path, source);
    }
}
