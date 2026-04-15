//! Git-sourced plugin artifacts: GitHub URL parsing, API client, and cache management.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use crate::plugins::UpdateLevel;

/// Minimum interval between freshness checks for cached sources.
const DEBOUNCE_DURATION: std::time::Duration = std::time::Duration::from_secs(60);

/// A parsed GitHub repository reference.
#[derive(Debug, Clone, PartialEq)]
pub struct GitHubSource {
    pub owner: String,
    pub repo: String,
    /// Branch, tag, or commit ref. Empty string means default branch.
    pub git_ref: String,
    /// Path within the repo. Empty string means repo root.
    pub path: String,
}

impl GitHubSource {
    /// Filesystem-safe cache directory name.
    pub fn cache_key(&self) -> String {
        let mut key = format!("{}--{}", self.owner, self.repo);
        if !self.git_ref.is_empty() {
            key.push_str(&format!("@{}", self.git_ref));
        }
        if !self.path.is_empty() {
            let path_slug = self.path.replace('/', "--");
            key.push_str(&format!("--{}", path_slug));
        }
        key
    }
}

/// Parse a GitHub URL into its components.
///
/// Accepts:
/// - `https://github.com/owner/repo`
/// - `https://github.com/owner/repo/tree/branch/path/to/dir`
pub fn parse_github_url(url: &str) -> Result<GitHubSource> {
    let stripped = url
        .strip_prefix("https://github.com/")
        .with_context(|| format!("not a GitHub URL: {url}"))?;

    // Remove trailing slash
    let stripped = stripped.trim_end_matches('/');

    let parts: Vec<&str> = stripped.splitn(4, '/').collect();

    match parts.len() {
        // owner/repo
        2 => Ok(GitHubSource {
            owner: parts[0].to_string(),
            repo: parts[1].to_string(),
            git_ref: String::new(),
            path: String::new(),
        }),
        // owner/repo/tree/ref[/path...]
        n if n >= 4 && parts[2] == "tree" => {
            let rest = parts[3];
            // rest is "branch/path/to/dir" — first segment is the ref, rest is path
            // But branches can contain slashes... we take the first segment as the ref.
            let (git_ref, path) = match rest.split_once('/') {
                Some((r, p)) => (r.to_string(), p.to_string()),
                None => (rest.to_string(), String::new()),
            };
            Ok(GitHubSource {
                owner: parts[0].to_string(),
                repo: parts[1].to_string(),
                git_ref,
                path,
            })
        }
        _ => bail!("cannot parse GitHub URL: {url}"),
    }
}

// --- GitHub API client ---

/// Client for GitHub REST API operations.
pub struct GitHubClient {
    client: reqwest::Client,
}

impl GitHubClient {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent("symposium")
            .build()
            .expect("failed to build HTTP client");
        Self { client }
    }

    /// Resolve the commit SHA for a given ref (branch, tag, or "HEAD").
    pub async fn resolve_commit_sha(&self, source: &GitHubSource) -> Result<String> {
        let git_ref = if source.git_ref.is_empty() {
            "HEAD"
        } else {
            &source.git_ref
        };
        let url = format!(
            "https://api.github.com/repos/{}/{}/commits/{}",
            source.owner, source.repo, git_ref
        );

        let resp = self
            .client
            .get(&url)
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .await
            .with_context(|| format!("failed to query GitHub API: {url}"))?;

        if !resp.status().is_success() {
            bail!("GitHub API error (HTTP {}): {}", resp.status(), url);
        }

        let json: serde_json::Value = resp.json().await?;
        let sha = json["sha"]
            .as_str()
            .context("GitHub API response missing 'sha' field")?;
        Ok(sha.to_string())
    }

    /// Download the repository tarball for a given ref.
    pub async fn download_tarball(&self, source: &GitHubSource) -> Result<bytes::Bytes> {
        let git_ref = if source.git_ref.is_empty() {
            "HEAD"
        } else {
            &source.git_ref
        };
        let url = format!(
            "https://api.github.com/repos/{}/{}/tarball/{}",
            source.owner, source.repo, git_ref
        );

        let resp = self
            .client
            .get(&url)
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .await
            .with_context(|| format!("failed to download tarball: {url}"))?;

        if !resp.status().is_success() {
            bail!(
                "GitHub tarball download failed (HTTP {}): {}",
                resp.status(),
                url
            );
        }

        resp.bytes()
            .await
            .context("failed to read tarball response body")
    }
}

// --- Plugin cache manager ---

/// Metadata stored alongside cached plugin artifacts.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct PluginCacheMeta {
    pub commit_sha: String,
    pub fetched_at: String,
    pub source_url: String,
}

const CACHE_META_FILENAME: &str = ".symposium-cache-meta.json";

/// Manages downloading and caching of git-sourced plugin artifacts.
pub struct GitCacheManager {
    cache_dir: PathBuf,
    client: GitHubClient,
}

impl GitCacheManager {
    /// Create a cache manager for the given subdirectory under the cache root.
    ///
    /// Use `"plugins"` for individual skill git sources,
    /// `"plugin-sources"` for plugin source repositories.
    pub fn new(sym: &crate::config::Symposium, subdir: &str) -> Self {
        let cache_dir = sym.cache_dir().join(subdir);
        Self {
            cache_dir,
            client: GitHubClient::new(),
        }
    }

    /// Get the cached plugin directory, downloading or updating as needed.
    ///
    /// `update` controls how aggressively we check for updates:
    /// - `None`: skip API calls if cache was fetched within `DEBOUNCE_DURATION`
    /// - `Check`: always call the API to check freshness, download only if stale
    /// - `Fetch`: always re-download regardless of staleness
    pub async fn get_or_fetch(
        &self,
        source: &GitHubSource,
        source_url: &str,
        update: UpdateLevel,
    ) -> Result<PathBuf> {
        std::fs::create_dir_all(&self.cache_dir).with_context(|| {
            format!(
                "failed to create cache directory {}",
                self.cache_dir.display()
            )
        })?;

        let cache_key = source.cache_key();
        let plugin_dir = self.cache_dir.join(&cache_key);
        let meta_path = plugin_dir.join(CACHE_META_FILENAME);

        // If cached, check freshness according to update level
        if plugin_dir.exists() {
            if let Some(meta) = self.read_meta(&meta_path) {
                // Debounce: if fetched recently, skip the API call entirely
                if matches!(update, UpdateLevel::None) {
                    if let Ok(fetched_at) = chrono::DateTime::parse_from_rfc3339(&meta.fetched_at) {
                        let age = chrono::Utc::now() - fetched_at.with_timezone(&chrono::Utc);
                        if age < chrono::Duration::from_std(DEBOUNCE_DURATION).unwrap() {
                            tracing::debug!(%cache_key, "plugin cache is recent, skipping check");
                            return Ok(plugin_dir);
                        }
                    }
                }

                // Fetch level: skip freshness check, always re-download
                if matches!(update, UpdateLevel::Fetch) {
                    tracing::info!(%cache_key, "force-fetching plugin source");
                    let sha = self.client.resolve_commit_sha(source).await?;
                    self.fetch_and_cache_with_sha(
                        source,
                        source_url,
                        &plugin_dir,
                        &meta_path,
                        &sha,
                    )
                    .await?;
                    return Ok(plugin_dir);
                }

                // None (past debounce) or Check: check freshness via API
                match self.client.resolve_commit_sha(source).await {
                    Ok(remote_sha) => {
                        if meta.commit_sha == remote_sha {
                            tracing::debug!(%cache_key, "plugin cache is fresh");
                            // Update fetched_at so subsequent debounce checks use this time
                            self.touch_meta(&meta_path, &meta);
                            return Ok(plugin_dir);
                        }
                        tracing::info!(%cache_key, "plugin cache is stale, re-fetching");
                        self.fetch_and_cache_with_sha(
                            source,
                            source_url,
                            &plugin_dir,
                            &meta_path,
                            &remote_sha,
                        )
                        .await?;
                        return Ok(plugin_dir);
                    }
                    Err(e) => {
                        tracing::warn!(
                            %cache_key,
                            error = %e,
                            "failed to check freshness, using cached version"
                        );
                        return Ok(plugin_dir);
                    }
                }
            }
        }

        // Download and extract (fresh fetch — need to resolve SHA)
        let sha = self.client.resolve_commit_sha(source).await?;
        self.fetch_and_cache_with_sha(source, source_url, &plugin_dir, &meta_path, &sha)
            .await?;
        Ok(plugin_dir)
    }

    async fn fetch_and_cache_with_sha(
        &self,
        source: &GitHubSource,
        source_url: &str,
        plugin_dir: &std::path::Path,
        meta_path: &std::path::Path,
        sha: &str,
    ) -> Result<()> {
        let tarball = self.client.download_tarball(source).await?;

        // Extract to a temp directory first, then move into place
        std::fs::create_dir_all(&self.cache_dir).with_context(|| {
            format!(
                "failed to create cache directory `{}`",
                self.cache_dir.display()
            )
        })?;
        let temp_dir = tempfile::tempdir_in(&self.cache_dir).with_context(|| {
            format!(
                "failed to create temp directory for extraction in {}",
                self.cache_dir.display()
            )
        })?;

        extract_tarball(&tarball, temp_dir.path())?;

        // If a subpath is specified, we need to find and move just that subtree
        let source_dir = if source.path.is_empty() {
            temp_dir.path().to_path_buf()
        } else {
            let sub = temp_dir.path().join(&source.path);
            if !sub.is_dir() {
                bail!(
                    "path '{}' not found in repository {}/{}",
                    source.path,
                    source.owner,
                    source.repo
                );
            }
            sub
        };

        // Remove old cache and move new one into place
        if plugin_dir.exists() {
            std::fs::remove_dir_all(plugin_dir)
                .with_context(|| format!("failed to remove old cache: {}", plugin_dir.display()))?;
        }
        std::fs::create_dir_all(plugin_dir.parent().unwrap_or(plugin_dir))?;

        // Copy (not rename — source may be a subdirectory of temp_dir)
        copy_dir_recursive(&source_dir, plugin_dir)?;

        // Write meta
        let meta = PluginCacheMeta {
            commit_sha: sha.to_string(),
            fetched_at: chrono::Utc::now().to_rfc3339(),
            source_url: source_url.to_string(),
        };
        let meta_json = serde_json::to_string_pretty(&meta)?;
        std::fs::write(meta_path, meta_json)?;

        Ok(())
    }

    fn read_meta(&self, path: &std::path::Path) -> Option<PluginCacheMeta> {
        let content = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Update `fetched_at` to now so the debounce window resets.
    fn touch_meta(&self, path: &std::path::Path, meta: &PluginCacheMeta) {
        let updated = PluginCacheMeta {
            commit_sha: meta.commit_sha.clone(),
            fetched_at: chrono::Utc::now().to_rfc3339(),
            source_url: meta.source_url.clone(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&updated) {
            let _ = std::fs::write(path, json);
        }
    }
}

/// Extract a gzip tarball, flattening the single top-level directory.
fn extract_tarball(tarball_bytes: &[u8], dest: &std::path::Path) -> Result<()> {
    use flate2::read::GzDecoder;
    use tar::Archive;

    std::fs::create_dir_all(dest)?;

    let gz = GzDecoder::new(std::io::Cursor::new(tarball_bytes));
    let mut archive = Archive::new(gz);

    archive.unpack(dest).context("failed to extract tarball")?;

    // GitHub tarballs contain a single top-level directory (org-repo-sha/).
    // Flatten it.
    flatten_single_dir(dest)?;

    Ok(())
}

/// If a directory contains exactly one subdirectory and nothing else,
/// move that subdirectory's contents up one level.
fn flatten_single_dir(dir: &std::path::Path) -> Result<()> {
    let entries: Vec<_> = std::fs::read_dir(dir)?.collect::<std::result::Result<Vec<_>, _>>()?;

    if entries.len() == 1 && entries[0].file_type()?.is_dir() {
        let inner = entries[0].path();
        for entry in std::fs::read_dir(&inner)? {
            let entry = entry?;
            let src = entry.path();
            let dst = dir.join(entry.file_name());
            std::fs::rename(&src, &dst)?;
        }
        std::fs::remove_dir(&inner)?;
    }

    Ok(())
}

/// Recursively copy a directory tree.
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- URL parsing ---

    #[test]
    fn parse_owner_repo_only() {
        let source = parse_github_url("https://github.com/symposium-dev/recommendations").unwrap();
        assert_eq!(source.owner, "symposium-dev");
        assert_eq!(source.repo, "recommendations");
        assert_eq!(source.git_ref, "");
        assert_eq!(source.path, "");
    }

    #[test]
    fn parse_with_trailing_slash() {
        let source = parse_github_url("https://github.com/symposium-dev/recommendations/").unwrap();
        assert_eq!(source.owner, "symposium-dev");
        assert_eq!(source.repo, "recommendations");
    }

    #[test]
    fn parse_tree_branch_only() {
        let source = parse_github_url("https://github.com/org/repo/tree/main").unwrap();
        assert_eq!(source.owner, "org");
        assert_eq!(source.repo, "repo");
        assert_eq!(source.git_ref, "main");
        assert_eq!(source.path, "");
    }

    #[test]
    fn parse_tree_branch_with_path() {
        let source =
            parse_github_url("https://github.com/symposium-dev/recommendations/tree/main/serde")
                .unwrap();
        assert_eq!(source.owner, "symposium-dev");
        assert_eq!(source.repo, "recommendations");
        assert_eq!(source.git_ref, "main");
        assert_eq!(source.path, "serde");
    }

    #[test]
    fn parse_tree_branch_with_nested_path() {
        let source =
            parse_github_url("https://github.com/org/repo/tree/v2/plugins/serde/skills").unwrap();
        assert_eq!(source.owner, "org");
        assert_eq!(source.repo, "repo");
        assert_eq!(source.git_ref, "v2");
        assert_eq!(source.path, "plugins/serde/skills");
    }

    #[test]
    fn parse_not_github() {
        assert!(parse_github_url("https://gitlab.com/org/repo").is_err());
    }

    #[test]
    fn parse_too_few_segments() {
        assert!(parse_github_url("https://github.com/org").is_err());
    }

    #[test]
    fn parse_tree_without_ref() {
        // owner/repo/tree with no ref after /tree — should error, not panic
        assert!(parse_github_url("https://github.com/org/repo/tree").is_err());
        assert!(parse_github_url("https://github.com/org/repo/tree/").is_err());
    }

    // --- Cache key ---

    #[test]
    fn cache_key_owner_repo() {
        let source = GitHubSource {
            owner: "org".into(),
            repo: "repo".into(),
            git_ref: String::new(),
            path: String::new(),
        };
        assert_eq!(source.cache_key(), "org--repo");
    }

    #[test]
    fn cache_key_with_ref() {
        let source = GitHubSource {
            owner: "org".into(),
            repo: "repo".into(),
            git_ref: "main".into(),
            path: String::new(),
        };
        assert_eq!(source.cache_key(), "org--repo@main");
    }

    #[test]
    fn cache_key_with_ref_and_path() {
        let source = GitHubSource {
            owner: "symposium-dev".into(),
            repo: "recommendations".into(),
            git_ref: "main".into(),
            path: "plugins/serde".into(),
        };
        assert_eq!(
            source.cache_key(),
            "symposium-dev--recommendations@main--plugins--serde"
        );
    }

    // --- Tarball extraction ---

    #[test]
    fn flatten_single_dir_works() {
        let tmp = tempfile::tempdir().unwrap();
        let inner = tmp.path().join("org-repo-abc123");
        std::fs::create_dir(&inner).unwrap();
        std::fs::write(inner.join("file.txt"), "hello").unwrap();
        std::fs::create_dir(inner.join("subdir")).unwrap();
        std::fs::write(inner.join("subdir").join("nested.txt"), "world").unwrap();

        flatten_single_dir(tmp.path()).unwrap();

        assert!(tmp.path().join("file.txt").exists());
        assert!(tmp.path().join("subdir").join("nested.txt").exists());
        assert!(!inner.exists());
    }

    #[test]
    fn flatten_noop_if_multiple_entries() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "a").unwrap();
        std::fs::write(tmp.path().join("b.txt"), "b").unwrap();

        flatten_single_dir(tmp.path()).unwrap();

        // Nothing should change
        assert!(tmp.path().join("a.txt").exists());
        assert!(tmp.path().join("b.txt").exists());
    }

    #[test]
    fn copy_dir_recursive_works() {
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("a.txt"), "hello").unwrap();
        std::fs::create_dir(src.path().join("sub")).unwrap();
        std::fs::write(src.path().join("sub").join("b.txt"), "world").unwrap();

        let dst = tempfile::tempdir().unwrap();
        let dst_path = dst.path().join("output");
        copy_dir_recursive(src.path(), &dst_path).unwrap();

        assert_eq!(
            std::fs::read_to_string(dst_path.join("a.txt")).unwrap(),
            "hello"
        );
        assert_eq!(
            std::fs::read_to_string(dst_path.join("sub").join("b.txt")).unwrap(),
            "world"
        );
    }
}
