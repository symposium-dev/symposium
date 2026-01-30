//! Remote recommendations fetching and caching.
//!
//! This module handles:
//! - Fetching recommendations from the remote URL
//! - Caching the result locally
//! - Loading local user recommendations
//! - Loading workspace-specific recommendations
//! - Merging all recommendation sources

use crate::user_config::ConfigPaths;
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::time::Duration;
use symposium_recommendations::Recommendations;

/// URL for the remote recommendations file.
const REMOTE_RECOMMENDATIONS_URL: &str =
    "http://recommendations.symposium.dev/recommendations.toml";

/// Filename for cached remote recommendations.
const CACHED_RECOMMENDATIONS_FILENAME: &str = "recommendations.toml";

/// Filename for user's local recommendations.
const LOCAL_RECOMMENDATIONS_FILENAME: &str = "recommendations.toml";

/// Directory for workspace-specific symposium config.
const WORKSPACE_SYMPOSIUM_DIR: &str = ".symposium";

/// Filename for workspace-specific recommendations.
const WORKSPACE_RECOMMENDATIONS_FILENAME: &str = "recommendations.toml";

/// HTTP request timeout in seconds.
const HTTP_TIMEOUT_SECS: u64 = 30;

/// Load all recommendations from all sources, merging them together.
///
/// Sources (in order, all are merged):
/// 1. Remote recommendations (downloaded and cached)
/// 2. User's local recommendations file
///
/// If the remote cannot be fetched, falls back to cached version.
/// If no recommendations can be loaded at all, returns an error.
pub async fn load_recommendations(config_paths: &ConfigPaths) -> Result<Recommendations> {
    // 1. Try to load remote recommendations (with caching fallback)
    let mut combined = load_remote_with_cache(config_paths).await?;

    // 2. Load user's local recommendations if present and merge
    if let Some(local_recs) = load_local_recommendations(config_paths)? {
        combined.mods.extend(local_recs.mods);
    }

    Ok(combined)
}

/// Fetch remote recommendations and cache them, or fall back to cache.
///
/// Returns an error if:
/// - Remote fetch fails AND no cached version exists
async fn load_remote_with_cache(config_paths: &ConfigPaths) -> Result<Recommendations> {
    let cache_path = config_paths
        .cache_dir()
        .join(CACHED_RECOMMENDATIONS_FILENAME);

    // Try to fetch from remote
    match fetch_remote_recommendations().await {
        Ok((toml_content, recommendations)) => {
            // Successfully fetched - cache the raw TOML string
            if let Err(e) = cache_recommendations(config_paths, &toml_content) {
                tracing::warn!("Failed to cache recommendations: {}", e);
            }
            Ok(recommendations)
        }
        Err(fetch_error) => {
            tracing::warn!("Failed to fetch remote recommendations: {}", fetch_error);

            // Try to load from cache
            if cache_path.exists() {
                tracing::info!("Using cached recommendations from {}", cache_path.display());
                let cached_toml = std::fs::read_to_string(&cache_path)
                    .context("Failed to read cached recommendations")?;
                Recommendations::from_toml(&cached_toml)
                    .context("Failed to parse cached recommendations")
            } else {
                bail!(
                    "Cannot load recommendations: remote fetch failed ({}) and no cache exists at {}",
                    fetch_error,
                    cache_path.display()
                )
            }
        }
    }
}

/// Fetch recommendations from the remote URL.
///
/// Returns both the raw TOML string (for caching) and the parsed recommendations.
async fn fetch_remote_recommendations() -> Result<(String, Recommendations)> {
    tracing::debug!(
        "Fetching recommendations from {}",
        REMOTE_RECOMMENDATIONS_URL
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
        .context("Failed to create HTTP client")?;

    let response = client
        .get(REMOTE_RECOMMENDATIONS_URL)
        .send()
        .await
        .context("Failed to connect to recommendations server")?;

    if !response.status().is_success() {
        bail!(
            "Failed to fetch recommendations: {} {}",
            response.status().as_u16(),
            response.status().canonical_reason().unwrap_or("Unknown")
        );
    }

    let content = response
        .text()
        .await
        .context("Failed to read recommendations response")?;

    // Parse and validate
    let recommendations =
        Recommendations::from_toml(&content).context("Remote recommendations failed to parse")?;

    Ok((content, recommendations))
}

/// Cache recommendations to disk.
fn cache_recommendations(config_paths: &ConfigPaths, content: &str) -> Result<()> {
    config_paths.ensure_cache_dir()?;
    let cache_path = config_paths
        .cache_dir()
        .join(CACHED_RECOMMENDATIONS_FILENAME);

    std::fs::write(&cache_path, content)
        .with_context(|| format!("Failed to write cache to {}", cache_path.display()))?;

    tracing::debug!("Cached recommendations to {}", cache_path.display());
    Ok(())
}

/// Load user's local recommendations file if it exists.
///
/// Location: `<config_dir>/config/recommendations.toml`
fn load_local_recommendations(config_paths: &ConfigPaths) -> Result<Option<Recommendations>> {
    let local_path = config_paths
        .root()
        .join("config")
        .join(LOCAL_RECOMMENDATIONS_FILENAME);

    if !local_path.exists() {
        return Ok(None);
    }

    tracing::debug!(
        "Loading local recommendations from {}",
        local_path.display()
    );

    let content = std::fs::read_to_string(&local_path).with_context(|| {
        format!(
            "Failed to read local recommendations from {}",
            local_path.display()
        )
    })?;

    let recommendations = Recommendations::from_toml(&content).with_context(|| {
        format!(
            "Failed to parse local recommendations from {}",
            local_path.display()
        )
    })?;

    Ok(Some(recommendations))
}

/// Load workspace-specific recommendations if they exist.
///
/// Location: `<workspace>/.symposium/recommendations.toml`
///
/// This allows projects to declare their own recommended mods that should
/// be suggested when working in that workspace.
pub fn load_workspace_recommendations(workspace_path: &Path) -> Result<Option<Recommendations>> {
    let workspace_recs_path = workspace_path
        .join(WORKSPACE_SYMPOSIUM_DIR)
        .join(WORKSPACE_RECOMMENDATIONS_FILENAME);

    if !workspace_recs_path.exists() {
        return Ok(None);
    }

    tracing::debug!(
        "Loading workspace recommendations from {}",
        workspace_recs_path.display()
    );

    let content = std::fs::read_to_string(&workspace_recs_path).with_context(|| {
        format!(
            "Failed to read workspace recommendations from {}",
            workspace_recs_path.display()
        )
    })?;

    let recommendations = Recommendations::from_toml(&content).with_context(|| {
        format!(
            "Failed to parse workspace recommendations from {}",
            workspace_recs_path.display()
        )
    })?;

    Ok(Some(recommendations))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Get the path where local recommendations should be placed.
    ///
    /// This is useful for error messages telling users where to put their file.
    fn local_recommendations_path(config_paths: &ConfigPaths) -> std::path::PathBuf {
        config_paths
            .root()
            .join("config")
            .join(LOCAL_RECOMMENDATIONS_FILENAME)
    }

    #[test]
    fn test_local_recommendations_path() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_paths = ConfigPaths::with_root(temp_dir.path());

        let path = local_recommendations_path(&config_paths);
        assert!(path.ends_with("config/recommendations.toml"));
    }

    #[tokio::test]
    async fn test_load_recommendations_merges_local() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_paths = ConfigPaths::with_root(temp_dir.path());

        // Create local recommendations with a unique mod
        let local_dir = config_paths.root().join("config");
        std::fs::create_dir_all(&local_dir).unwrap();
        std::fs::write(
            local_dir.join(LOCAL_RECOMMENDATIONS_FILENAME),
            r#"
[[recommendation]]
source.builtin = "test-local-mod"
"#,
        )
        .unwrap();

        // Load should succeed (fetches from remote) and include local mod
        let recs = load_recommendations(&config_paths).await.unwrap();

        // Should have the local mod merged in
        let names: Vec<_> = recs.mods.iter().map(|r| r.display_name()).collect();
        assert!(
            names.contains(&"test-local-mod".to_string()),
            "Local mod should be merged. Got: {:?}",
            names
        );

        // Should also have some remote mods (at least one)
        assert!(recs.mods.len() > 1, "Should have remote mods too");
    }

    #[tokio::test]
    async fn test_load_recommendations_caches_result() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_paths = ConfigPaths::with_root(temp_dir.path());

        // Load recommendations (will fetch from remote)
        let recs = load_recommendations(&config_paths).await.unwrap();
        assert!(!recs.mods.is_empty(), "Should have loaded recommendations");

        // Verify cache file was created
        let cache_path = config_paths
            .cache_dir()
            .join(CACHED_RECOMMENDATIONS_FILENAME);
        assert!(cache_path.exists(), "Cache file should exist");

        // Cache should be valid TOML
        let cache_content = std::fs::read_to_string(&cache_path).unwrap();
        Recommendations::from_toml(&cache_content).expect("Cache should be valid TOML");
    }

    #[tokio::test]
    async fn test_cache_fallback_with_invalid_url() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_paths = ConfigPaths::with_root(temp_dir.path());

        // Pre-populate cache
        config_paths.ensure_cache_dir().unwrap();
        let cache_path = config_paths
            .cache_dir()
            .join(CACHED_RECOMMENDATIONS_FILENAME);
        std::fs::write(
            &cache_path,
            r#"
[[recommendation]]
source.builtin = "cached-mod"
"#,
        )
        .unwrap();

        // The load_remote_with_cache function should use cache if remote fails
        // We can't easily test this without mocking, but at least verify cache is read
        let cache_content = std::fs::read_to_string(&cache_path).unwrap();
        let recs = Recommendations::from_toml(&cache_content).unwrap();
        assert_eq!(recs.mods.len(), 1);
        assert_eq!(recs.mods[0].display_name(), "cached-mod");
    }

    #[test]
    fn test_load_workspace_recommendations_when_present() {
        let temp_dir = tempfile::tempdir().unwrap();
        let workspace_path = temp_dir.path();

        // Create .symposium/recommendations.toml
        let symposium_dir = workspace_path.join(WORKSPACE_SYMPOSIUM_DIR);
        std::fs::create_dir_all(&symposium_dir).unwrap();
        std::fs::write(
            symposium_dir.join(WORKSPACE_RECOMMENDATIONS_FILENAME),
            r#"
[[recommendation]]
source.builtin = "workspace-mod"
"#,
        )
        .unwrap();

        let recs = load_workspace_recommendations(workspace_path)
            .unwrap()
            .expect("Should load workspace recommendations");

        assert_eq!(recs.mods.len(), 1);
        assert_eq!(recs.mods[0].display_name(), "workspace-mod");
    }

    #[test]
    fn test_load_workspace_recommendations_when_absent() {
        let temp_dir = tempfile::tempdir().unwrap();
        let workspace_path = temp_dir.path();

        // No .symposium directory
        let recs = load_workspace_recommendations(workspace_path).unwrap();
        assert!(recs.is_none(), "Should return None when no file exists");
    }

    #[test]
    fn test_load_workspace_recommendations_with_conditions() {
        let temp_dir = tempfile::tempdir().unwrap();
        let workspace_path = temp_dir.path();

        // Create .symposium/recommendations.toml with a conditional recommendation
        let symposium_dir = workspace_path.join(WORKSPACE_SYMPOSIUM_DIR);
        std::fs::create_dir_all(&symposium_dir).unwrap();
        std::fs::write(
            symposium_dir.join(WORKSPACE_RECOMMENDATIONS_FILENAME),
            r#"
[[recommendation]]
source.builtin = "always-mod"

[[recommendation]]
source.builtin = "rust-only-mod"
when.file-exists = "Cargo.toml"
"#,
        )
        .unwrap();

        let recs = load_workspace_recommendations(workspace_path)
            .unwrap()
            .expect("Should load workspace recommendations");

        // Both mods are loaded (conditions are evaluated later in for_workspace)
        assert_eq!(recs.mods.len(), 2);
    }
}
