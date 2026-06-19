//! Acquisition and caching of tool binaries for symposium plugins.
//!
//! This crate provides the machinery to install cargo binaries and clone GitHub
//! repositories into a local cache. It is used by both the main `symposium`
//! binary and by hook handlers that need to invoke external tools.
#![deny(missing_docs)]

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub mod git;

/// Records the crates.io version resolved by the last freshness check, so a
/// `None` acquire can reuse it without re-querying. Lives at
/// `<cache>/binaries/<crate>/current`.
const CURRENT_VERSION_FILENAME: &str = "current";

/// Records the git commit a `cargo + git` binary was last installed from, so a
/// freshness check can skip the reinstall when the branch hasn't moved. Lives
/// inside the binary's version-keyed cache dir.
const COMMIT_SHA_FILENAME: &str = ".commit-sha";

/// Minimal context needed for acquisition.
///
/// Replaces the full `Symposium` config struct — hook handlers construct this
/// directly from environment or hardcoded paths.
#[derive(Debug, Clone)]
pub struct InstallContext {
    cache_dir: PathBuf,
    cargo_bin: Option<PathBuf>,
}

impl InstallContext {
    /// Create a new context rooted at the given cache directory.
    ///
    /// Acquired binaries and cloned repositories are stored under this path.
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache_dir,
            cargo_bin: None,
        }
    }

    /// Override the cargo binary used for `cargo install` / `cargo binstall`.
    ///
    /// If not set, the plain `"cargo"` from `$PATH` is used.
    pub fn with_cargo_bin(mut self, path: PathBuf) -> Self {
        self.cargo_bin = Some(path);
        self
    }

    /// The root directory where cached artifacts are stored.
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Build a [`Command`](std::process::Command) for the configured cargo binary.
    pub fn cargo_command(&self) -> std::process::Command {
        match &self.cargo_bin {
            Some(path) => std::process::Command::new(path),
            None => std::process::Command::new("cargo"),
        }
    }
}

/// Controls how aggressively cached sources are updated.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
pub enum UpdateLevel {
    /// Debounced: skip the API check if fetched recently.
    #[default]
    None,
    /// Always check freshness via API, but only download if stale.
    Check,
    /// Always re-download regardless of staleness.
    Fetch,
}

/// How to acquire bits onto disk.
///
/// `Source` describes acquisition only: cargo install, github clone. The
/// "what to run" lives separately on the installation as `executable` or
/// `script` (which resolve relative to the acquired source). An installation
/// can omit `Source` entirely — in that case `executable` / `script` are
/// taken as paths on disk and `install_commands` does any setup.
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "source", rename_all = "lowercase")]
pub enum Source {
    /// Install via `cargo install` or `cargo binstall`.
    Cargo(CargoSource),
    /// Clone from a GitHub repository.
    Github(GithubSource),
}

/// A binary obtained by `cargo install` (with optional binstall fast-path).
#[non_exhaustive]
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
    /// Install into the user's global cargo location (`~/.cargo/bin`) instead
    /// of a symposium-managed cache. The default (`false`) uses
    /// `cargo install --root <symposium-cache>` so binaries don't pollute the
    /// global namespace; hook execution adds the cache `bin/` to `$PATH` so
    /// scripts can still invoke them by name.
    #[serde(default, skip_serializing_if = "is_false")]
    pub global: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl CargoSource {
    /// Create a source for the given crate name, defaulting to the latest
    /// stable version from crates.io.
    pub fn new(crate_name: impl Into<String>) -> Self {
        Self {
            crate_name: crate_name.into(),
            version: None,
            git: None,
            global: false,
        }
    }

    /// Pin to a specific version (e.g. `"1.2.3"`).
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Install from a git repository URL instead of crates.io.
    ///
    /// When set, an `executable` hint is required since crates.io is not
    /// consulted to discover binary names.
    pub fn with_git(mut self, git_url: impl Into<String>) -> Self {
        self.git = Some(git_url.into());
        self
    }
}

/// A directory of files acquired from a GitHub repository (or subtree).
/// The file to run inside the cloned tree is picked by `executable` /
/// `script` on the installation or hook.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GithubSource {
    /// The GitHub URL to clone.
    #[serde(alias = "git")]
    pub url: String,
}

impl GithubSource {
    /// Create a source for the given GitHub URL.
    pub fn new(url: impl Into<String>) -> Self {
        Self { url: url.into() }
    }
}

fn platform_binary_exe(binary_name: &str) -> String {
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
async fn query_crate_binaries(
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
///
/// `cache_dir` is `Some` for scoped installs (passed via `--root`) and `None`
/// for global installs (uses cargo's default location).
async fn install_cargo_crate(
    ctx: &InstallContext,
    crate_name: &str,
    version: &str,
    binary_name: Option<String>,
    cache_dir: Option<PathBuf>,
    git: Option<String>,
    force: bool,
) -> Result<()> {
    let ctx = ctx.clone();
    let crate_name = crate_name.to_string();
    let version = version.to_string();

    tokio::task::spawn_blocking(move || {
        install_cargo_crate_sync(
            &ctx,
            &crate_name,
            &version,
            binary_name,
            cache_dir.as_deref(),
            git,
            force,
        )
    })
    .await
    .context("Cargo install task panicked")?
}

fn install_cargo_crate_sync(
    ctx: &InstallContext,
    crate_name: &str,
    version: &str,
    binary_name: Option<String>,
    cache_dir: Option<&Path>,
    git: Option<String>,
    force: bool,
) -> Result<()> {
    use std::fs;

    if let Some(cache_dir) = cache_dir {
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
    }

    // Empty version → just the crate name. Avoids `cargo install rtk@` which
    // cargo rejects.
    let crate_spec = if version.is_empty() {
        crate_name.to_string()
    } else {
        format!("{}@{}", crate_name, version)
    };
    let cache_dir_str = cache_dir.map(|p| p.to_str().unwrap().to_string());

    // Try cargo binstall first (faster, uses prebuilt binaries).
    tracing::info!("Attempting cargo binstall for {}", crate_spec);
    let mut binstall_args: Vec<&str> = vec!["binstall", "--no-confirm"];
    if force {
        binstall_args.push("--force");
    }
    if let Some(dir) = cache_dir_str.as_deref() {
        binstall_args.push("--root");
        binstall_args.push(dir);
    }
    if let Some(git) = &git {
        binstall_args.push("--git");
        binstall_args.push(git);
    }
    binstall_args.push(&crate_spec);
    let binstall_result = ctx.cargo_command().args(binstall_args).output();

    let binary_path = match (cache_dir, binary_name.as_ref()) {
        (Some(dir), Some(bin)) => Some(dir.join("bin").join(platform_binary_exe(bin))),
        _ => None,
    };

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
    let mut args: Vec<&str> = vec!["install"];
    if force {
        args.push("--force");
    }
    if let Some(dir) = cache_dir_str.as_deref() {
        args.push("--root");
        args.push(dir);
    }
    if let Some(git) = &git {
        args.push("--git");
        args.push(git);
    }
    args.push(&crate_spec);
    let install_result = ctx
        .cargo_command()
        .args(&args)
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

fn get_binary_cache_dir(ctx: &InstallContext, krate: &str, version: &str) -> Result<PathBuf> {
    let path = ctx.cache_dir().join("binaries").join(krate).join(version);
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

/// Outcome of acquiring a cargo source.
struct AcquiredCargo {
    /// Symposium-managed cache dir, or `None` for global installs (no --root).
    cache_dir: Option<PathBuf>,
    /// The resolved binary name, when known.
    resolved_executable: Option<String>,
}

/// Acquire a cargo installation: install if missing.
///
/// Three branches, in priority order:
/// - `global = true`: skip crates.io, install with no `--root` (binary lands
///   in the user's `$CARGO_HOME/bin`). Validation guarantees the caller has
///   set `executable`, so no inference needed. `cache_dir = None` is the
///   signal that the source is unmanaged.
/// - `git` source: skip crates.io, install with `--root <cache>` and
///   `--git <url>`. Validation guarantees `executable`.
/// - Plain crates.io: query for version + bin_names; auto-infer the binary
///   when the crate has exactly one.
async fn acquire_cargo(
    ctx: &InstallContext,
    cargo: &CargoSource,
    executable_hint: Option<&str>,
    update: UpdateLevel,
) -> Result<AcquiredCargo> {
    if cargo.global {
        // Validation requires `executable` for global cargo.
        let resolved = executable_hint
            .expect("validate_installation enforces `executable` for cargo + global")
            .to_string();
        install_cargo_crate(
            ctx,
            &cargo.crate_name,
            cargo.version.as_deref().unwrap_or(""),
            Some(resolved.clone()),
            None,
            cargo.git.clone(),
            false,
        )
        .await?;
        return Ok(AcquiredCargo {
            cache_dir: None,
            resolved_executable: Some(resolved),
        });
    }

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
        let cache_dir = get_binary_cache_dir(ctx, &cargo.crate_name, &cache_version)?;
        let probe = cache_dir.join("bin").join(platform_binary_exe(&resolved));
        let sha_file = cache_dir.join(COMMIT_SHA_FILENAME);

        // The cache key folds in only the URL + user version, not the resolved
        // commit, so a moved branch never invalidates it on its own. Under
        // `Check`/`Fetch` we resolve the remote `HEAD` with a cheap `git
        // ls-remote` and reinstall only when it differs from the commit we last
        // installed; under `None` we never touch the network.
        let remote_sha = if matches!(update, UpdateLevel::Check | UpdateLevel::Fetch) {
            remote_git_head_sha(git_url).await
        } else {
            None
        };
        let stored_sha = std::fs::read_to_string(&sha_file)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let sha_changed = match (&remote_sha, &stored_sha) {
            (Some(remote), Some(stored)) => remote != stored,
            // Remote resolved but nothing recorded yet → (re)install to capture it.
            (Some(_), None) => true,
            // Couldn't resolve the remote → don't reinstall on SHA grounds.
            (None, _) => false,
        };

        if !probe.exists() || matches!(update, UpdateLevel::Fetch) || sha_changed {
            // `--force` only needed to overwrite an existing install.
            let overwrite = probe.exists();
            install_cargo_crate(
                ctx,
                &cargo.crate_name,
                cargo.version.as_deref().unwrap_or(""),
                Some(resolved.clone()),
                Some(cache_dir.clone()),
                Some(git_url.to_string()),
                overwrite,
            )
            .await?;
            // Record the commit we just installed so the next check can compare.
            let installed_sha = match remote_sha {
                Some(sha) => Some(sha),
                None => remote_git_head_sha(git_url).await,
            };
            if let Some(sha) = installed_sha {
                let _ = std::fs::write(&sha_file, sha);
            }
        }
        return Ok(AcquiredCargo {
            cache_dir: Some(cache_dir),
            resolved_executable: Some(resolved),
        });
    }

    // Plain crates.io. Resolving the version means a crates.io query, so under
    // `None` we serve the version recorded by the last freshness check instead
    // — a dispatch never hits the network. `Check`/`Fetch` always re-resolve
    // (picking up newly published versions) and refresh the pointer.
    let registry_dir = ctx.cache_dir().join("binaries").join(&cargo.crate_name);
    let pointer = registry_dir.join(CURRENT_VERSION_FILENAME);
    if matches!(update, UpdateLevel::None)
        && let Some((cache_dir, resolved)) =
            cached_cargo_version(&registry_dir, &pointer, executable_hint)
    {
        return Ok(AcquiredCargo {
            cache_dir: Some(cache_dir),
            resolved_executable: resolved,
        });
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

    let cache_dir = get_binary_cache_dir(ctx, &cargo.crate_name, &version)?;
    let probe_path = resolved
        .as_ref()
        .map(|n| cache_dir.join("bin").join(platform_binary_exe(n)));

    // The cache dir is keyed on the resolved version, so a newer published
    // version installs into a fresh dir on its own. Only `Fetch` forces a
    // reinstall of the same version.
    let force = matches!(update, UpdateLevel::Fetch);
    let already = probe_path.as_ref().is_some_and(|p| p.exists());
    if !already || force {
        install_cargo_crate(
            ctx,
            &cargo.crate_name,
            &version,
            resolved.clone(),
            Some(cache_dir.clone()),
            None,
            force,
        )
        .await?;
    }

    // Record the resolved version so future `None` acquires can skip the query.
    if let Err(e) =
        std::fs::create_dir_all(&registry_dir).and_then(|()| std::fs::write(&pointer, &version))
    {
        tracing::debug!(error = %e, "failed to record current cargo version pointer");
    }

    Ok(AcquiredCargo {
        cache_dir: Some(cache_dir),
        resolved_executable: resolved,
    })
}

/// Resolve a cargo binary cache from the recorded `current` version pointer,
/// without consulting crates.io. Returns the version's cache dir and the binary
/// name to run, or `None` when there's no usable cached version (so the caller
/// falls back to a registry query).
fn cached_cargo_version(
    registry_dir: &Path,
    pointer: &Path,
    executable_hint: Option<&str>,
) -> Option<(PathBuf, Option<String>)> {
    let version = std::fs::read_to_string(pointer).ok()?;
    let version = version.trim();
    if version.is_empty() {
        return None;
    }
    let cache_dir = registry_dir.join(version);
    let bin_dir = cache_dir.join("bin");
    let resolved = match executable_hint {
        Some(name) => {
            if !bin_dir.join(platform_binary_exe(name)).exists() {
                return None;
            }
            Some(name.to_string())
        }
        // No hint: only safe to serve from cache when there's exactly one binary.
        None => Some(single_cached_binary(&bin_dir)?),
    };
    Some((cache_dir, resolved))
}

/// The sole binary name (platform extension stripped) in `bin_dir`, or `None`
/// when there are zero or several — in which case the caller re-queries to
/// disambiguate.
fn single_cached_binary(bin_dir: &Path) -> Option<String> {
    let mut found: Option<String> = None;
    for entry in std::fs::read_dir(bin_dir).ok()?.flatten() {
        if !entry.path().is_file() {
            continue;
        }
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if found.is_some() {
            return None;
        }
        let name = name
            .strip_suffix(".exe")
            .map(str::to_string)
            .unwrap_or(name);
        found = Some(name);
    }
    found
}

/// Resolve the remote default-branch (`HEAD`) commit of a git URL via `git
/// ls-remote` — cheap, no clone. `None` on any failure, leaving the caller to
/// fall back to a force reinstall.
async fn remote_git_head_sha(url: &str) -> Option<String> {
    let url = url.to_string();
    tokio::task::spawn_blocking(move || {
        let output = std::process::Command::new("git")
            .args(["ls-remote", &url, "HEAD"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        parse_ls_remote_sha(&String::from_utf8_lossy(&output.stdout))
    })
    .await
    .ok()
    .flatten()
}

/// Parse the commit SHA from `git ls-remote <url> HEAD` output, whose first
/// field is the hash (`<sha>\tHEAD`).
fn parse_ls_remote_sha(output: &str) -> Option<String> {
    let sha = output.split_whitespace().next()?;
    (sha.len() >= 7 && sha.chars().all(|c| c.is_ascii_hexdigit())).then(|| sha.to_string())
}

/// Acquire a github source: returns the cache directory containing the repo
/// (or its requested subtree).
async fn acquire_github(
    ctx: &InstallContext,
    git: &GithubSource,
    update: UpdateLevel,
) -> Result<PathBuf> {
    let cache_mgr = crate::git::GitCacheManager::new(ctx, "plugins");
    cache_mgr.fetch_url(&git.url, update).await
}

/// Intermediate result of acquiring a `Source`: where the bits landed and
/// the layout-specific hooks needed to turn an `executable` / `script` name
/// into a concrete path. Consumed by `hook::acquire_installation`, which
/// wraps this together with the installation's name, `install_commands`,
/// and (for no-source installs) absolute path defaults to produce the
/// fully-resolved `AcquiredInstallation`.
pub struct AcquiredSource {
    /// Where bits landed. `None` for global cargo (binary is in
    /// `~/.cargo/bin`, which we don't manage).
    pub base: Option<PathBuf>,
    /// For cargo, the binary name that was installed. Used as the fallback
    /// when neither the installation nor the hook supplies an explicit
    /// `executable`. `None` for github (which has no notion of "default
    /// binary").
    pub resolved_executable: Option<String>,
    /// Layout discriminator — cargo binaries live under `<base>/bin/`,
    /// github paths live under `<base>/` directly.
    pub kind: AcquiredKind,
}

/// How an [`AcquiredSource`] was obtained.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcquiredKind {
    /// Installed via `cargo install` / `cargo binstall`.
    Cargo,
    /// Cloned from a GitHub repository.
    Github,
}

impl AcquiredSource {
    /// Path to the executable, using `resolved_executable` as the name.
    /// Use [`Self::executable_named`] to specify the executable name from the outside.
    ///
    /// Returns `None` if no executable name was resolved (e.g., a GitHub
    /// source with no hint).
    pub fn executable(&self) -> Option<PathBuf> {
        let name = self.resolved_executable.as_deref()?;
        Some(self.executable_named(name))
    }

    /// Path of an `executable` declared on installation/hook.
    pub fn executable_named(&self, name: &str) -> PathBuf {
        let base = self
            .base
            .as_ref()
            .expect("resolve_executable called on unmanaged source");

        match self.kind {
            AcquiredKind::Cargo => base.join("bin").join(platform_binary_exe(name)),
            AcquiredKind::Github => base.join(name.trim_start_matches("./")),
        }
    }

    /// Path of a `script` declared on installation/hook.
    pub fn script_named(&self, name: &str) -> PathBuf {
        let base = self
            .base
            .as_ref()
            .expect("resolve_executable called on unmanaged source");

        match self.kind {
            AcquiredKind::Cargo => base.join("bin").join(name.trim_start_matches("./")),
            AcquiredKind::Github => base.join(name.trim_start_matches("./")),
        }
    }
}

/// Acquire a source, downloading / installing as needed.
///
/// `executable_hint` is only used for cargo (to pick which binary to install
/// for multi-binary crates, or as the binary name when using a git source).
///
/// `update` controls freshness: `None` serves the cache (git checks debounced),
/// while `Check`/`Fetch` force a re-pull. It's the lever that lets `SessionStart`
/// refresh a `cargo + git` binary whose branch moved (the version-keyed cache
/// otherwise never re-installs on its own).
pub async fn acquire_source(
    ctx: &InstallContext,
    source: &Source,
    executable_hint: Option<&str>,
    update: UpdateLevel,
) -> Result<AcquiredSource> {
    match source {
        Source::Cargo(c) => {
            let acquired = acquire_cargo(ctx, c, executable_hint, update).await?;
            Ok(AcquiredSource {
                base: acquired.cache_dir,
                resolved_executable: acquired.resolved_executable,
                kind: AcquiredKind::Cargo,
            })
        }
        Source::Github(g) => Ok(AcquiredSource {
            base: Some(acquire_github(ctx, g, update).await?),
            resolved_executable: None,
            kind: AcquiredKind::Github,
        }),
    }
}

/// Refresh an already-acquired source in place, leaving an un-acquired source
/// untouched. Unlike [`acquire_source`], this never installs a missing source:
/// it only *updates* what is already cached (with `UpdateLevel::Check`).
/// Returns `true` when a refresh ran.
///
/// This is for the once-per-session `SessionStart` warm-up, which should keep
/// already-installed tools current but must not eagerly install a tool a hook
/// may never use — that install happens lazily on first dispatch.
pub async fn refresh_source_if_present(
    ctx: &InstallContext,
    source: &Source,
    executable_hint: Option<&str>,
) -> Result<bool> {
    if !source_is_cached(ctx, source, executable_hint) {
        return Ok(false);
    }
    acquire_source(ctx, source, executable_hint, UpdateLevel::Check).await?;
    Ok(true)
}

/// Whether a source's bits are already on disk, computed from cache paths
/// without acquiring anything. Drives [`refresh_source_if_present`].
fn source_is_cached(ctx: &InstallContext, source: &Source, executable_hint: Option<&str>) -> bool {
    match source {
        // Global cargo lives in the user's `~/.cargo/bin`, outside our cache;
        // we don't manage it, so the prewarm leaves it alone.
        Source::Cargo(c) if c.global => false,
        Source::Cargo(c) => match c.git.as_deref() {
            Some(git_url) => {
                let Some(exe) = executable_hint else {
                    return false;
                };
                let cache_version = git_cache_version(git_url, c.version.as_deref());
                let Ok(cache_dir) = get_binary_cache_dir(ctx, &c.crate_name, &cache_version) else {
                    return false;
                };
                cache_dir
                    .join("bin")
                    .join(platform_binary_exe(exe))
                    .exists()
            }
            None => {
                let registry_dir = ctx.cache_dir().join("binaries").join(&c.crate_name);
                let pointer = registry_dir.join(CURRENT_VERSION_FILENAME);
                cached_cargo_version(&registry_dir, &pointer, executable_hint).is_some()
            }
        },
        Source::Github(g) => crate::git::GitCacheManager::new(ctx, "plugins")
            .cache_path_for_url(&g.url)
            .is_some_and(|p| p.exists()),
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

impl Runnable {
    /// Spawn this runnable with the given arguments, wait for it to finish,
    /// and return the captured output.
    pub fn spawn(
        &self,
        args: &[impl AsRef<std::ffi::OsStr>],
    ) -> std::io::Result<std::process::Output> {
        use std::process::Command;
        match self {
            Runnable::Exec(path) => Command::new(path).args(args).output(),
            Runnable::Script(path) => Command::new("sh").arg(path).args(args).output(),
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ls_remote_typical() {
        let out = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678\tHEAD\n";
        assert_eq!(
            parse_ls_remote_sha(out).as_deref(),
            Some("a1b2c3d4e5f60718293a4b5c6d7e8f9012345678")
        );
    }

    #[test]
    fn parse_ls_remote_takes_first_field() {
        // ls-remote separates the sha from the ref with a tab.
        let out = "deadbeefcafe1234\trefs/heads/main\n";
        assert_eq!(
            parse_ls_remote_sha(out).as_deref(),
            Some("deadbeefcafe1234")
        );
    }

    #[test]
    fn parse_ls_remote_rejects_empty_and_nonhex() {
        assert_eq!(parse_ls_remote_sha(""), None);
        assert_eq!(parse_ls_remote_sha("not-a-sha\tHEAD"), None);
        // Too short to be a real object id.
        assert_eq!(parse_ls_remote_sha("abc\tHEAD"), None);
    }

    fn write_cached_binary(registry_dir: &Path, version: &str, bin: &str) {
        let bin_dir = registry_dir.join(version).join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::write(bin_dir.join(platform_binary_exe(bin)), b"").unwrap();
    }

    #[test]
    fn cached_cargo_version_serves_pointer_with_hint() {
        let tmp = tempfile::tempdir().unwrap();
        let registry_dir = tmp.path().join("ripgrep");
        write_cached_binary(&registry_dir, "14.1.0", "rg");
        let pointer = registry_dir.join(CURRENT_VERSION_FILENAME);
        std::fs::write(&pointer, "14.1.0").unwrap();

        let (cache_dir, resolved) =
            cached_cargo_version(&registry_dir, &pointer, Some("rg")).unwrap();
        assert_eq!(cache_dir, registry_dir.join("14.1.0"));
        assert_eq!(resolved.as_deref(), Some("rg"));
    }

    #[test]
    fn cached_cargo_version_infers_single_binary_without_hint() {
        let tmp = tempfile::tempdir().unwrap();
        let registry_dir = tmp.path().join("ripgrep");
        write_cached_binary(&registry_dir, "14.1.0", "rg");
        let pointer = registry_dir.join(CURRENT_VERSION_FILENAME);
        std::fs::write(&pointer, "14.1.0\n").unwrap();

        let (_, resolved) = cached_cargo_version(&registry_dir, &pointer, None).unwrap();
        assert_eq!(resolved.as_deref(), Some("rg"));
    }

    #[test]
    fn cached_cargo_version_none_without_pointer() {
        let tmp = tempfile::tempdir().unwrap();
        let registry_dir = tmp.path().join("ripgrep");
        write_cached_binary(&registry_dir, "14.1.0", "rg");
        let pointer = registry_dir.join(CURRENT_VERSION_FILENAME);
        // No pointer written → must fall back to a query.
        assert!(cached_cargo_version(&registry_dir, &pointer, Some("rg")).is_none());
    }

    #[test]
    fn cached_cargo_version_none_when_binary_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let registry_dir = tmp.path().join("ripgrep");
        write_cached_binary(&registry_dir, "14.1.0", "rg");
        let pointer = registry_dir.join(CURRENT_VERSION_FILENAME);
        std::fs::write(&pointer, "14.1.0").unwrap();

        // Pointer is valid but the requested binary isn't there.
        assert!(cached_cargo_version(&registry_dir, &pointer, Some("other")).is_none());
    }

    #[test]
    fn cached_cargo_version_ambiguous_without_hint_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let registry_dir = tmp.path().join("multi");
        write_cached_binary(&registry_dir, "1.0.0", "a");
        write_cached_binary(&registry_dir, "1.0.0", "b");
        let pointer = registry_dir.join(CURRENT_VERSION_FILENAME);
        std::fs::write(&pointer, "1.0.0").unwrap();

        // Two binaries and no hint → can't disambiguate from cache.
        assert!(cached_cargo_version(&registry_dir, &pointer, None).is_none());
    }

    #[test]
    fn source_is_cached_crates_io_present_and_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = InstallContext::new(tmp.path().to_path_buf());
        let src = Source::Cargo(CargoSource::new("ripgrep"));

        // Absent → false: the prewarm must not eagerly install.
        assert!(!source_is_cached(&ctx, &src, Some("rg")));

        let registry_dir = ctx.cache_dir().join("binaries").join("ripgrep");
        write_cached_binary(&registry_dir, "14.1.0", "rg");
        std::fs::write(registry_dir.join(CURRENT_VERSION_FILENAME), "14.1.0").unwrap();
        assert!(source_is_cached(&ctx, &src, Some("rg")));
    }

    #[test]
    fn source_is_cached_global_is_never_managed() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = InstallContext::new(tmp.path().to_path_buf());
        let mut cargo = CargoSource::new("ripgrep");
        cargo.global = true;
        assert!(!source_is_cached(&ctx, &Source::Cargo(cargo), Some("rg")));
    }

    #[test]
    fn source_is_cached_cargo_git_probes_commit_keyed_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = InstallContext::new(tmp.path().to_path_buf());
        let cargo = CargoSource::new("tool").with_git("https://example.com/o/r");
        let src = Source::Cargo(cargo.clone());

        assert!(!source_is_cached(&ctx, &src, Some("tool")));

        let cache_version =
            git_cache_version(cargo.git.as_deref().unwrap(), cargo.version.as_deref());
        let bin_dir = get_binary_cache_dir(&ctx, &cargo.crate_name, &cache_version)
            .unwrap()
            .join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::write(bin_dir.join(platform_binary_exe("tool")), b"").unwrap();
        assert!(source_is_cached(&ctx, &src, Some("tool")));

        // A git source can't be probed without knowing the binary name.
        assert!(!source_is_cached(&ctx, &src, None));
    }

    #[tokio::test]
    async fn refresh_source_if_present_skips_absent_source() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = InstallContext::new(tmp.path().to_path_buf());
        let src = Source::Cargo(CargoSource::new("ripgrep"));

        // Nothing cached → no refresh runs and nothing is installed (no network).
        let refreshed = refresh_source_if_present(&ctx, &src, Some("rg"))
            .await
            .unwrap();
        assert!(!refreshed);
        assert!(!ctx.cache_dir().join("binaries").join("ripgrep").exists());
    }
}
