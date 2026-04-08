//! Version resolution for Rust crates

use anyhow::{Context, Result, bail};
use semver::{Version, VersionReq};

/// Handles version resolution using the three-tier strategy:
/// explicit constraint → current workspace → latest on crates.io
pub struct VersionResolver<'a> {
    workspace: &'a [(String, semver::Version)],
}

impl<'a> VersionResolver<'a> {
    pub fn new(workspace: &'a [(String, semver::Version)]) -> Self {
        Self { workspace }
    }

    /// Resolve the canonical crate name and version.
    ///
    /// Returns `(canonical_name, version)` where `canonical_name` is the
    /// crate's published name (which may differ from the input in
    /// hyphen/underscore normalization).
    pub async fn resolve(
        &self,
        crate_name: &str,
        version_spec: Option<&str>,
    ) -> Result<(String, String)> {
        if let Some(spec) = version_spec {
            let version = self.resolve_version_constraint(crate_name, spec).await?;
            Ok((crate_name.to_string(), version))
        } else if let Some((canonical, version)) = self.find_in_workspace(crate_name) {
            Ok((canonical, version))
        } else {
            let version = self.get_latest_version(crate_name).await?;
            Ok((crate_name.to_string(), version))
        }
    }

    /// Find crate in the workspace, matching with hyphen/underscore normalization.
    ///
    /// Returns the canonical name (as published) and version.
    fn find_in_workspace(&self, crate_name: &str) -> Option<(String, String)> {
        let normalized = crate_name.replace('-', "_");
        self.workspace
            .iter()
            .find(|(name, _)| name.replace('-', "_") == normalized)
            .map(|(name, v)| (name.clone(), v.to_string()))
    }

    /// Resolve a version constraint to the latest matching version on crates.io
    async fn resolve_version_constraint(
        &self,
        crate_name: &str,
        constraint: &str,
    ) -> Result<String> {
        let req = VersionReq::parse(constraint)
            .with_context(|| format!("invalid version constraint: {constraint}"))?;
        let available = self.get_available_versions(crate_name).await?;

        let mut matching: Vec<_> = available.into_iter().filter(|v| req.matches(v)).collect();
        matching.sort();

        matching.last().map(|v| v.to_string()).ok_or_else(|| {
            anyhow::anyhow!("no versions of '{crate_name}' match constraint '{constraint}'")
        })
    }

    /// Get the latest version from crates.io
    async fn get_latest_version(&self, crate_name: &str) -> Result<String> {
        let client = crates_io_api::AsyncClient::new(
            "cargo-agents (https://github.com/symposium-dev/symposium)",
            std::time::Duration::from_millis(1000),
        )
        .context("failed to create crates.io client")?;

        let crate_info = client
            .get_crate(crate_name)
            .await
            .map_err(|_| anyhow::anyhow!("crate '{crate_name}' not found on crates.io"))?;

        Ok(crate_info.crate_data.max_version)
    }

    /// Get all available versions from crates.io
    async fn get_available_versions(&self, crate_name: &str) -> Result<Vec<Version>> {
        let client = crates_io_api::AsyncClient::new(
            "cargo-agents (https://github.com/symposium-dev/symposium)",
            std::time::Duration::from_millis(1000),
        )
        .context("failed to create crates.io client")?;

        let crate_info = client
            .get_crate(crate_name)
            .await
            .map_err(|_| anyhow::anyhow!("crate '{crate_name}' not found on crates.io"))?;

        let mut versions = Vec::new();
        for v in crate_info.versions {
            if let Ok(parsed) = Version::parse(&v.num) {
                versions.push(parsed);
            }
        }

        if versions.is_empty() {
            bail!("no versions found for '{crate_name}'");
        }

        Ok(versions)
    }
}
