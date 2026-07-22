//! Extract the `[package.metadata.symposium]` table from a crate `Cargo.toml`.
//!
//! The table uses the *same schema* as a `SYMPOSIUM.toml` plugin manifest — a
//! crate can define its plugin inline in `Cargo.toml` instead of (or in
//! addition to) shipping a separate file.
//! [`CargoPm::load_plugin`](crate::pm::CargoPm::load_plugin) deserializes
//! whatever this returns into a plugin manifest and merges it with any
//! `SYMPOSIUM.toml` the crate ships (see
//! [`load_crate_manifest`](crate::plugins::load_crate_manifest)).

use std::path::Path;

use anyhow::Result;
use serde::Deserialize;

/// Read a crate `Cargo.toml` and return its `[package.metadata.symposium]`
/// table, if present. The table is returned verbatim; validation against the
/// plugin-manifest schema happens in `plugins`.
pub fn symposium_metadata(cargo_toml_path: &Path) -> Result<Option<toml::Table>> {
    let content = std::fs::read_to_string(cargo_toml_path)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", cargo_toml_path.display()))?;
    symposium_metadata_str(&content)
}

/// Extract `[package.metadata.symposium]` from a `Cargo.toml` string.
pub(crate) fn symposium_metadata_str(content: &str) -> Result<Option<toml::Table>> {
    let doc: CargoToml = toml::from_str(content)?;
    Ok(doc
        .package
        .and_then(|p| p.metadata)
        .and_then(|m| m.symposium))
}

// --- serde types for the Cargo.toml structure we navigate ---

#[derive(Deserialize)]
struct CargoToml {
    package: Option<CargoPackage>,
}

#[derive(Deserialize)]
struct CargoPackage {
    metadata: Option<PackageMetadata>,
    #[serde(flatten)]
    _rest: toml::Table,
}

#[derive(Deserialize)]
struct PackageMetadata {
    symposium: Option<toml::Table>,
    #[serde(flatten)]
    _rest: toml::Table,
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;

    #[test]
    fn extract_missing_metadata() {
        let toml = indoc! {r#"
            [package]
            name = "my-crate"
            version = "0.1.0"
            edition = "2021"
        "#};
        assert!(symposium_metadata_str(toml).unwrap().is_none());
    }

    #[test]
    fn extract_symposium_table() {
        // The block is returned verbatim, in the plugin-manifest schema.
        let toml = indoc! {r#"
            [package]
            name = "my-crate"
            version = "0.1.0"
            edition = "2021"

            [[package.metadata.symposium.skills]]
            source.path = "guidance"

            [[package.metadata.symposium.plugins]]
            source.cargo = "other-crate"
        "#};
        let table = symposium_metadata_str(toml).unwrap().unwrap();
        assert!(table.contains_key("skills"));
        assert!(table.contains_key("plugins"));
    }

    #[test]
    fn extract_ignores_unrelated_metadata() {
        // Other `[package.metadata.*]` keys don't interfere.
        let toml = indoc! {r#"
            [package]
            name = "my-crate"
            version = "0.1.0"
            edition = "2021"

            [package.metadata.docs.rs]
            all-features = true
        "#};
        assert!(symposium_metadata_str(toml).unwrap().is_none());
    }
}
