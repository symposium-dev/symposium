//! Version resolution for Rust crates

use crate::eg::{Result, EgError};
use cargo_metadata::{MetadataCommand, CargoOpt};
use semver::{Version, VersionReq};

/// Handles version resolution using the three-tier strategy
pub struct VersionResolver;

impl VersionResolver {
    pub fn new() -> Self {
        Self
    }

    /// Resolve version using: explicit → current project → latest
    pub async fn resolve_version(&self, crate_name: &str, version_spec: Option<&str>) -> Result<String> {
        if let Some(spec) = version_spec {
            // Explicit version specified - find latest matching version
            self.resolve_version_constraint(crate_name, spec).await
        } else {
            // Try current project first
            if let Ok(version) = self.find_in_current_project(crate_name) {
                Ok(version)
            } else {
                // Fallback to latest
                self.get_latest_version(crate_name).await
            }
        }
    }

    /// Find crate version in current project's dependencies
    fn find_in_current_project(&self, crate_name: &str) -> Result<String> {
        let metadata = MetadataCommand::new()
            .features(CargoOpt::AllFeatures)
            .exec()?;

        // Look through all packages in the resolved dependency graph
        for package in metadata.packages {
            if package.name.as_str() == crate_name {
                return Ok(package.version.to_string());
            }
        }

        Err(EgError::CrateNotFound(crate_name.to_string()))
    }

    /// Resolve version constraint to latest matching version
    async fn resolve_version_constraint(&self, crate_name: &str, constraint: &str) -> Result<String> {
        let req = VersionReq::parse(constraint)?;
        let available_versions = self.get_available_versions(crate_name).await?;
        
        // Find the latest version that matches the constraint
        let mut matching_versions: Vec<_> = available_versions
            .into_iter()
            .filter(|v| req.matches(v))
            .collect();
        
        matching_versions.sort();
        
        matching_versions
            .last()
            .map(|v| v.to_string())
            .ok_or_else(|| EgError::NoMatchingVersions {
                crate_name: crate_name.to_string(),
                constraint: constraint.to_string(),
            })
    }

    /// Get latest version from crates.io
    async fn get_latest_version(&self, crate_name: &str) -> Result<String> {
        let client = crates_io_api::AsyncClient::new(
            "eg-library (https://github.com/symposium/eg)",
            std::time::Duration::from_millis(1000),
        ).map_err(|e| EgError::Other(e.to_string()))?;

        let crate_info = client.get_crate(crate_name).await
            .map_err(|_| EgError::CrateNotFound(crate_name.to_string()))?;

        Ok(crate_info.crate_data.max_version)
    }

    /// Get all available versions from crates.io
    async fn get_available_versions(&self, crate_name: &str) -> Result<Vec<Version>> {
        let client = crates_io_api::AsyncClient::new(
            "eg-library (https://github.com/symposium/eg)",
            std::time::Duration::from_millis(1000),
        ).map_err(|e| EgError::Other(e.to_string()))?;

        // Get crate info which includes versions
        let crate_info = client.get_crate(crate_name).await
            .map_err(|_| EgError::CrateNotFound(crate_name.to_string()))?;

        let mut parsed_versions = Vec::new();
        for version in crate_info.versions {
            if let Ok(v) = Version::parse(&version.num) {
                parsed_versions.push(v);
            }
        }

        Ok(parsed_versions)
    }
}
