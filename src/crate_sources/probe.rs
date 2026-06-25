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

use crate::config::CargoDependencySpec;

use super::{FetchResult, normalize_crate_name};

/// Fetch a crate via a temporary dummy cargo package.
///
/// `version_req` may be any valid Cargo version requirement (e.g. `"=1.2.3"`,
/// `"^1.0"`, `"*"`).
pub async fn fetch_via_cargo(crate_name: &str, version_req: &str) -> Result<FetchResult> {
    let spec = CargoDependencySpec::Version(version_req.to_string());
    fetch_dependency_via_cargo(Some(crate_name), &spec).await
}

/// Fetch a crate via a temporary dummy package using Cargo dependency-table
/// syntax.
///
/// `dependency_key` is the dependency table key. It may be `None` for unkeyed
/// specs such as `{ path = "../my-crate" }`, where Cargo infers the package
/// from the source path. For registry dependencies, callers must provide a key.
pub async fn fetch_dependency_via_cargo(
    dependency_key: Option<&str>,
    spec: &CargoDependencySpec,
) -> Result<FetchResult> {
    let dependency_key = dependency_key.map(ToOwned::to_owned);
    let spec = spec.clone();

    tokio::task::spawn_blocking(move || fetch_dependency_sync(dependency_key.as_deref(), &spec))
        .await
        .context("cargo probe task panicked")?
}

fn fetch_dependency_sync(
    dependency_key: Option<&str>,
    spec: &CargoDependencySpec,
) -> Result<FetchResult> {
    let package_name = package_name_for_lookup(dependency_key, spec)?;
    tracing::debug!(
        ?dependency_key,
        ?spec,
        "fetching crate via cargo dependency probe"
    );

    let temp = tempfile::Builder::new()
        .prefix("symposium-crate-probe-")
        .tempdir()
        .context("failed to create temp directory for cargo probe")?;

    let manifest_key = dependency_key.unwrap_or(&package_name);
    write_dummy_package_for_dependency(temp.path(), Some(manifest_key), spec)?;
    run_cargo_fetch_for_dependency(temp.path(), Some(manifest_key), spec)?;
    find_crate_in_metadata(temp.path(), &package_name)
}

fn package_name_for_lookup(
    dependency_key: Option<&str>,
    spec: &CargoDependencySpec,
) -> Result<String> {
    if let Some(package) = spec.package() {
        return Ok(package.to_string());
    }
    if let Some(key) = dependency_key {
        return Ok(key.to_string());
    }
    if let Some(path) = spec.path() {
        return package_name_from_path(Path::new(path));
    }
    bail!("crate registry specs without a dependency key must include `path` or `package`")
}

fn package_name_from_path(path: &Path) -> Result<String> {
    let manifest_path = path.join("Cargo.toml");
    let contents = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let manifest: toml::Value = toml::from_str(&contents)
        .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
    manifest
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(toml::Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow::anyhow!("{} is missing package.name", manifest_path.display()))
}

fn write_dummy_package_for_dependency(
    dir: &Path,
    dependency_key: Option<&str>,
    spec: &CargoDependencySpec,
) -> Result<()> {
    std::fs::create_dir_all(dir.join("src"))?;
    std::fs::write(dir.join("src/lib.rs"), "")?;

    let dependency_toml = dependency_toml_entry(dependency_key, spec)?;
    let cargo_toml = format!(
        r#"[package]
name = "symposium-crate-probe"
version = "0.0.0"
edition = "2021"

[lib]
path = "src/lib.rs"

[dependencies]
{dependency_toml}

[workspace]
"#
    );
    std::fs::write(dir.join("Cargo.toml"), cargo_toml)?;
    Ok(())
}

fn run_cargo_fetch_for_dependency(
    dir: &Path,
    dependency_key: Option<&str>,
    spec: &CargoDependencySpec,
) -> Result<()> {
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
        let rendered = dependency_toml_entry(dependency_key, spec)
            .unwrap_or_else(|_| "<invalid dependency spec>".to_string());
        bail!("cargo fetch failed for `{rendered}`: {stderr}");
    }
    Ok(())
}

/// Render a single Cargo dependency table entry.
pub(crate) fn dependency_toml_entry(
    dependency_key: Option<&str>,
    spec: &CargoDependencySpec,
) -> Result<String> {
    let Some(key) = dependency_key else {
        bail!("dependency table entries require a key");
    };

    let value = dependency_value_for_toml(spec);
    Ok(format!("{key} = {}", value_to_inline_toml(&value)?))
}

/// Render dependency specs into a TOML table body.
pub(crate) fn dependency_table_toml<'a>(
    specs: impl IntoIterator<Item = (&'a str, &'a CargoDependencySpec)>,
) -> Result<String> {
    let mut entries = specs
        .into_iter()
        .map(|(key, spec)| dependency_toml_entry(Some(key), spec))
        .collect::<Result<Vec<_>>>()?;
    entries.sort();
    Ok(entries.join("\n"))
}

fn dependency_value_for_toml(spec: &CargoDependencySpec) -> toml::Value {
    match spec {
        CargoDependencySpec::Version(version) => {
            let mut table = toml::map::Map::new();
            table.insert("version".to_string(), toml::Value::String(version.clone()));
            toml::Value::Table(table)
        }
        CargoDependencySpec::Table(fields) => {
            toml::Value::Table(fields.clone().into_iter().collect())
        }
    }
}

fn value_to_inline_toml(value: &toml::Value) -> Result<String> {
    match value {
        toml::Value::Table(table) => {
            let mut entries = table
                .iter()
                .map(|(key, value)| Ok(format!("{key} = {}", scalar_value_to_toml(value)?)))
                .collect::<Result<Vec<_>>>()?;
            entries.sort();
            Ok(format!("{{ {} }}", entries.join(", ")))
        }
        value => scalar_value_to_toml(value),
    }
}

fn scalar_value_to_toml(value: &toml::Value) -> Result<String> {
    let wrapper_key = "dep";
    let wrapper = toml::Value::Table(toml::map::Map::from_iter([(
        wrapper_key.to_string(),
        value.clone(),
    )]));
    let rendered = toml::to_string(&wrapper)?;
    let (_, rhs) = rendered
        .trim()
        .split_once(" = ")
        .ok_or_else(|| anyhow::anyhow!("failed to render dependency spec as inline TOML"))?;
    Ok(rhs.to_string())
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
