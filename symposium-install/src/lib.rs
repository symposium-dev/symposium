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
        if !probe.exists() {
            install_cargo_crate(
                ctx,
                &cargo.crate_name,
                cargo.version.as_deref().unwrap_or(""),
                Some(resolved.clone()),
                Some(cache_dir.clone()),
                Some(git_url.to_string()),
            )
            .await?;
        }
        return Ok(AcquiredCargo {
            cache_dir: Some(cache_dir),
            resolved_executable: Some(resolved),
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

    let already = probe_path.as_ref().is_some_and(|p| p.exists());
    if !already {
        install_cargo_crate(
            ctx,
            &cargo.crate_name,
            &version,
            resolved.clone(),
            Some(cache_dir.clone()),
            None,
        )
        .await?;
    }

    Ok(AcquiredCargo {
        cache_dir: Some(cache_dir),
        resolved_executable: resolved,
    })
}

/// Acquire a github source: returns the cache directory containing the repo
/// (or its requested subtree).
async fn acquire_github(ctx: &InstallContext, git: &GithubSource) -> Result<PathBuf> {
    let cache_mgr = crate::git::GitCacheManager::new(ctx, "plugins");
    cache_mgr.fetch_url(&git.url, UpdateLevel::Check).await
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
pub async fn acquire_source(
    ctx: &InstallContext,
    source: &Source,
    executable_hint: Option<&str>,
) -> Result<AcquiredSource> {
    match source {
        Source::Cargo(c) => {
            let acquired = acquire_cargo(ctx, c, executable_hint).await?;
            Ok(AcquiredSource {
                base: acquired.cache_dir,
                resolved_executable: acquired.resolved_executable,
                kind: AcquiredKind::Cargo,
            })
        }
        Source::Github(g) => Ok(AcquiredSource {
            base: Some(acquire_github(ctx, g).await?),
            resolved_executable: None,
            kind: AcquiredKind::Github,
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
