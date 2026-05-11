use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::config::Symposium;

pub(crate) mod git;

/// How to acquire bits onto disk.
///
/// `Source` describes acquisition only: cargo install, github clone. The
/// "what to run" lives separately on the installation as `executable` or
/// `script` (which resolve relative to the acquired source). An installation
/// can omit `Source` entirely — in that case `executable` / `script` are
/// taken as paths on disk and `install_commands` does any setup.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "source", rename_all = "lowercase")]
pub enum Source {
    Cargo(CargoSource),
    Github(GithubSource),
}

/// A binary obtained by `cargo install` (with optional binstall fast-path).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CargoSource {
    /// The crate name (on crates.io, or as named in the git repo).
    #[serde(rename = "crate")]
    pub crate_name: String,
    /// Optional version (defaults to latest stable from crates.io; for git
    /// sources, used to derive a cache key).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Install from a git URL (`cargo install --git`) instead of crates.io.
    /// When set, the user must specify `executable` on the installation since
    /// crates.io is not consulted to discover binary names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<String>,
}

/// A directory of files acquired from a GitHub repository (or subtree).
/// The file to run inside the cloned tree is picked by `executable` /
/// `script` on the installation or hook.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GithubSource {
    #[serde(alias = "git")]
    pub url: String,
}

pub(crate) fn platform_binary_exe(binary_name: &str) -> String {
    if cfg!(windows) {
        format!("{}.exe", binary_name)
    } else {
        binary_name.to_string()
    }
}

/// Response from crates.io version endpoint
#[derive(Debug, Deserialize)]
struct CratesIoVersionResponse {
    version: CratesIoVersion,
}

#[derive(Debug, Deserialize)]
struct CratesIoVersion {
    bin_names: Vec<String>,
}

/// Response from crates.io crate endpoint (for getting latest version)
#[derive(Debug, Deserialize)]
struct CratesIoCrateResponse {
    #[serde(rename = "crate")]
    krate: CratesIoCrate,
}

#[derive(Debug, Deserialize)]
struct CratesIoCrate {
    max_stable_version: Option<String>,
    max_version: String,
}

/// Query crates.io for binary names of a crate
pub async fn query_crate_binaries(
    crate_name: &str,
    version: Option<&str>,
) -> Result<(String, Vec<String>)> {
    let client = reqwest::Client::builder()
        .user_agent("symposium (https://github.com/symposium-dev/symposium)")
        .build()?;

    // If no version specified, get the latest
    let version = match version {
        Some(v) => v.to_string(),
        None => {
            let url = format!("https://crates.io/api/v1/crates/{}", crate_name);
            let response = client
                .get(&url)
                .send()
                .await
                .with_context(|| format!("Failed to fetch crate info for {}", crate_name))?;

            if !response.status().is_success() {
                bail!("Crate '{}' not found on crates.io", crate_name);
            }

            let crate_info: CratesIoCrateResponse = response
                .json()
                .await
                .context("Failed to parse crates.io response")?;

            crate_info
                .krate
                .max_stable_version
                .unwrap_or(crate_info.krate.max_version)
        }
    };

    // Now get the version-specific info with bin_names
    let url = format!("https://crates.io/api/v1/crates/{}/{}", crate_name, version);
    let response = client.get(&url).send().await.with_context(|| {
        format!(
            "Failed to fetch version info for {}@{}",
            crate_name, version
        )
    })?;

    if !response.status().is_success() {
        bail!(
            "Version {} of crate '{}' not found on crates.io",
            version,
            crate_name
        );
    }

    let version_info: CratesIoVersionResponse = response
        .json()
        .await
        .context("Failed to parse crates.io version response")?;

    Ok((version, version_info.version.bin_names))
}

/// Install a crate using cargo binstall (fast) or cargo install (fallback).
pub(crate) async fn install_cargo_crate(
    crate_name: &str,
    version: &str,
    binary_name: Option<String>,
    cache_dir: PathBuf,
    git: Option<String>,
) -> Result<()> {
    let crate_name = crate_name.to_string();
    let version = version.to_string();

    tokio::task::spawn_blocking(move || {
        install_cargo_crate_sync(&crate_name, &version, binary_name, &cache_dir, git)
    })
    .await
    .context("Cargo install task panicked")?
}

fn install_cargo_crate_sync(
    crate_name: &str,
    version: &str,
    binary_name: Option<String>,
    cache_dir: &Path,
    git: Option<String>,
) -> Result<()> {
    use std::fs;
    use std::process::Command;

    if let Some(parent) = cache_dir.parent()
        && parent.exists()
    {
        for entry in fs::read_dir(parent)? {
            let entry = entry?;
            let path = entry.path();
            if path != cache_dir && path.is_dir() {
                fs::remove_dir_all(&path).ok();
            }
        }
    }

    fs::create_dir_all(cache_dir)?;

    let crate_spec = format!("{}@{}", crate_name, version);

    // Try cargo binstall first (faster, uses prebuilt binaries).
    tracing::info!("Attempting cargo binstall for {}", crate_spec);
    let mut binstall_args = vec!["binstall", "--no-confirm", "--root"];
    if let Some(git) = &git {
        binstall_args.push("--git");
        binstall_args.push(git);
    }
    binstall_args.extend([cache_dir.to_str().unwrap(), &crate_spec]);
    let binstall_result = Command::new("cargo").args(binstall_args).output();

    let binary_path = binary_name
        .as_ref()
        .map(|bin| cache_dir.join("bin").join(platform_binary_exe(bin)));

    match binstall_result {
        Ok(output) if output.status.success() => {
            tracing::info!("Successfully installed {} via cargo binstall", crate_spec);
            if binary_path.is_none() {
                return Ok(());
            }
            if let Some(bin) = binary_path.as_ref()
                && bin.exists()
            {
                return Ok(());
            }
        }
        Ok(output) => {
            tracing::debug!(
                "cargo binstall failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Err(e) => {
            tracing::debug!("cargo binstall not available: {}", e);
        }
    }

    tracing::info!("Falling back to cargo install for {}", crate_spec);
    let mut args = vec!["install", "--root"];
    if let Some(git) = &git {
        args.push("--git");
        args.push(git);
    }
    args.extend([cache_dir.to_str().unwrap(), &crate_spec]);
    let install_result = Command::new("cargo")
        .args(args)
        .output()
        .context("Failed to run cargo install")?;

    if !install_result.status.success() {
        bail!(
            "cargo install failed for {}: {}",
            crate_spec,
            String::from_utf8_lossy(&install_result.stderr)
        );
    }

    tracing::info!("Successfully installed {} via cargo install", crate_spec);
    Ok(())
}

pub fn get_binary_cache_dir(sym: &Symposium, krate: &str, version: &str) -> Result<PathBuf> {
    let path = sym.cache_dir().join("binaries").join(krate).join(version);
    Ok(path)
}

/// Cache key for a git-sourced cargo install. The user-supplied `version` (or
/// the literal `"git"` if absent) folds in with the URL so re-installs are
/// triggered when either changes.
fn git_cache_version(git_url: &str, version: Option<&str>) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    git_url.hash(&mut hasher);
    version.unwrap_or("git").hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Acquire a cargo installation: install if missing, return the cache dir
/// plus the resolved binary name (when discoverable from crates.io).
async fn acquire_cargo(
    sym: &Symposium,
    cargo: &CargoSource,
    executable_hint: Option<&str>,
) -> Result<(PathBuf, Option<String>)> {
    // For git sources, we don't consult crates.io. Cache key folds in the URL
    // and the user's version (if any). The user must specify `executable`
    // since we have no other way to pick a binary.
    if let Some(git_url) = cargo.git.as_deref() {
        let resolved = match executable_hint {
            Some(name) => name.to_string(),
            None => bail!(
                "cargo source for crate `{}` with `git` requires `executable` to be set \
                 (crates.io is not consulted, so the binary name is unknown)",
                cargo.crate_name
            ),
        };
        let cache_version = git_cache_version(git_url, cargo.version.as_deref());
        let cache_dir = get_binary_cache_dir(sym, &cargo.crate_name, &cache_version)?;
        let probe = cache_dir.join("bin").join(platform_binary_exe(&resolved));
        if !probe.exists() {
            install_cargo_crate(
                &cargo.crate_name,
                cargo.version.as_deref().unwrap_or(""),
                Some(resolved.clone()),
                cache_dir.clone(),
                Some(git_url.to_string()),
            )
            .await?;
        }
        return Ok((cache_dir, Some(resolved)));
    }

    let (version, bin_names) =
        query_crate_binaries(&cargo.crate_name, cargo.version.as_deref()).await?;

    let resolved = match executable_hint {
        Some(name) => Some(name.to_string()),
        None => match bin_names.as_slice() {
            [] => None,
            [single] => Some(single.clone()),
            multiple => bail!(
                "crate '{}' has multiple binaries {:?}, set `executable` to pick one",
                cargo.crate_name,
                multiple
            ),
        },
    };

    let cache_dir = get_binary_cache_dir(sym, &cargo.crate_name, &version)?;
    let probe_path = resolved
        .as_ref()
        .map(|n| cache_dir.join("bin").join(platform_binary_exe(n)));

    let already = probe_path.as_ref().map_or(false, |p| p.exists());
    if !already {
        install_cargo_crate(
            &cargo.crate_name,
            &version,
            resolved.clone(),
            cache_dir.clone(),
            None,
        )
        .await?;
    }

    Ok((cache_dir, resolved))
}

/// Acquire a github source: returns the cache directory containing the repo
/// (or its requested subtree).
pub(crate) async fn acquire_github(sym: &Symposium, git: &GithubSource) -> Result<PathBuf> {
    let git_url = &git.url;
    let source = crate::installation::git::parse_github_url(git_url)?;
    let cache_mgr = crate::installation::git::GitCacheManager::new(sym, "plugins");
    cache_mgr
        .get_or_fetch(&source, git_url, crate::plugins::UpdateLevel::Check)
        .await
}

/// Acquired source — where files ended up on disk, plus the kind so
/// `executable` / `script` can be resolved correctly relative to it.
#[derive(Debug)]
pub struct AcquiredSource {
    pub base: PathBuf,
    pub kind: AcquiredKind,
    /// For cargo, the binary name we ended up with (from `executable_hint`
    /// or by inferring the crate's sole binary). Used as the fallback when
    /// the hook command supplies no explicit executable. `None` for github.
    pub resolved_executable: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcquiredKind {
    Cargo,
    Github,
}

impl AcquiredSource {
    /// Path of an `executable` declared on installation/hook.
    pub fn resolve_executable(&self, name: &str) -> PathBuf {
        match self.kind {
            AcquiredKind::Cargo => self.base.join("bin").join(platform_binary_exe(name)),
            AcquiredKind::Github => self.base.join(name.trim_start_matches("./")),
        }
    }

    /// Path of a `script` declared on installation/hook.
    pub fn resolve_script(&self, name: &str) -> PathBuf {
        match self.kind {
            AcquiredKind::Cargo => self.base.join("bin").join(name.trim_start_matches("./")),
            AcquiredKind::Github => self.base.join(name.trim_start_matches("./")),
        }
    }
}

/// Acquire a source, downloading / installing as needed. The returned
/// `AcquiredSource` is used to resolve `executable` / `script` paths.
///
/// `executable_hint` is only used for cargo (to pick which binary to install
/// for multi-binary crates, or as the binary name when using a git source).
pub async fn acquire_source(
    sym: &Symposium,
    source: &Source,
    executable_hint: Option<&str>,
) -> Result<AcquiredSource> {
    match source {
        Source::Cargo(c) => {
            let (base, resolved) = acquire_cargo(sym, c, executable_hint).await?;
            Ok(AcquiredSource {
                base,
                kind: AcquiredKind::Cargo,
                resolved_executable: resolved,
            })
        }
        Source::Github(g) => Ok(AcquiredSource {
            base: acquire_github(sym, g).await?,
            kind: AcquiredKind::Github,
            resolved_executable: None,
        }),
    }
}

/// What an installation resolves to once acquired.
#[derive(Debug)]
pub enum Runnable {
    /// Run as a binary: `path args...`.
    Exec(PathBuf),
    /// Run as a shell script: `sh path args...`.
    Script(PathBuf),
}

/// Ensure a path is executable on Unix. No-op on other platforms.
pub fn make_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        if path.exists() {
            let mut perms = fs::metadata(path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(path, perms)?;
        }
    }
    let _ = path;
    Ok(())
}
