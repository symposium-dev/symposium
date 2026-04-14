use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::config::Symposium;
use crate::distribution::{get_binary_cache_dir, install_cargo_crate, query_crate_binaries};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum InstallationSource {
    Cargo(CargoSource),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize)]
pub struct CargoSource {
    /// The crate name on crates.io
    #[serde(rename = "crate")]
    pub crate_name: String,
    /// Optional version (defaults to latest)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Optional explicit binary name (if not specified, queried from crates.io)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary: Option<String>,
    /// Argument to pass to `--path`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Argument to pass to `--git`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<String>,
}

pub async fn install_from_source(sym: &Symposium, source: &InstallationSource) -> Result<()> {
    match source {
        InstallationSource::Cargo(cargo) => install_cargo(sym, cargo).await,
    }
}

async fn install_cargo(sym: &Symposium, cargo: &CargoSource) -> Result<()> {
    let version = if cargo.git.is_none() {
        "latest".to_string()
    } else {
        query_crate_binaries(&cargo.crate_name, cargo.version.as_deref())
            .await?
            .0
    };

    let binary_name = cargo.binary.clone();
    let cache_dir = get_binary_cache_dir(sym, &cargo.crate_name, &version)?;
    install_cargo_crate(
        &cargo.crate_name,
        &version,
        binary_name,
        cache_dir,
        cargo.path.clone(),
        cargo.git.clone(),
    )
    .await?;
    Ok(())
}
