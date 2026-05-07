use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::config::Symposium;

pub(crate) mod git;

/// How to acquire (and optionally pre-configure) a hook's executable.
///
/// Most variants describe acquisition: where bits come from. A few — `Shell`
/// in particular — describe invocation directly. Variants that resolve to a
/// concrete executable may carry default `args`; the hook command can override.
///
/// Acquisition shapes:
/// - **Single binary**: `Cargo`, `Local`, `Binary`. The hook runs that file directly.
/// - **Directory of files**: `Github`. A sub-path inside the directory must be
///   chosen, either on the installation (`path`) or on the consuming hook.
/// - **No file at all**: `Shell` runs a string via `sh -c`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "source", rename_all = "lowercase")]
pub enum InstallationKind {
    Local(LocalInstall),
    Shell(ShellInstall),
    Cargo(CargoInstall),
    Binary(BTreeMap<String, BinaryInstall>),
    Github(GithubInstall),
}

/// Local executable already on disk. `command` is the path; `args` are
/// invocation defaults.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LocalInstall {
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
}

/// A shell command run via `sh -c <command>`.
///
/// `args` are positional arguments (`$1`, `$2`, …) accessible inside the
/// shell command. Symposium spawns `sh -c <command> sh <args…>`, so `$0` is
/// the literal `"sh"` and user-supplied args start at `$1`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ShellInstall {
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
}

/// A binary obtained by `cargo install` (with optional binstall fast-path).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CargoInstall {
    /// The crate name on crates.io
    #[serde(rename = "crate")]
    pub crate_name: String,
    /// Optional version (defaults to latest)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Optional explicit binary name (if not specified, queried from crates.io)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary: Option<String>,
    /// Default args. Hook-level args (if any) override.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
}

/// A platform-specific prebuilt binary archive.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BinaryInstall {
    pub archive: String,
    pub cmd: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
}

/// A directory of files acquired from a GitHub repository (or subtree).
///
/// `path` (the file inside the repo) is optional here — the hook command may
/// supply it instead, and may override an installation-level `path`. `args` is
/// only meaningful when this installation already pins to a single file via
/// `path`; using `args` without `path` is a validation error.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GithubInstall {
    #[serde(alias = "git")]
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
}

impl InstallationKind {
    /// Default args declared on the installation, if any.
    pub fn default_args(&self) -> &[String] {
        match self {
            InstallationKind::Local(l) => &l.args,
            InstallationKind::Cargo(c) => &c.args,
            InstallationKind::Github(g) => &g.args,
            InstallationKind::Shell(s) => &s.args,
            InstallationKind::Binary(_) => &[],
        }
    }
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

/// Install a crate using cargo binstall (fast) or cargo install (fallback)
pub(crate) async fn install_cargo_crate(
    crate_name: &str,
    version: &str,
    binary_name: Option<String>,
    cache_dir: PathBuf,
    path: Option<String>,
    git: Option<String>,
) -> Result<()> {
    let crate_name = crate_name.to_string();
    let version = version.to_string();

    tokio::task::spawn_blocking(move || {
        install_cargo_crate_sync(&crate_name, &version, binary_name, &cache_dir, path, git)
    })
    .await
    .context("Cargo install task panicked")?
}

/// Install a crate using cargo binstall or cargo install (blocking)
fn install_cargo_crate_sync(
    crate_name: &str,
    version: &str,
    binary_name: Option<String>,
    cache_dir: &Path,
    path: Option<String>,
    git: Option<String>,
) -> Result<()> {
    use std::fs;
    use std::process::Command;

    // Clean up old versions first
    if let Some(parent) = cache_dir.parent() {
        if parent.exists() {
            for entry in fs::read_dir(parent)? {
                let entry = entry?;
                let path = entry.path();
                if path != cache_dir && path.is_dir() {
                    fs::remove_dir_all(&path).ok();
                }
            }
        }
    }

    // Create cache directory
    fs::create_dir_all(cache_dir)?;

    let crate_spec = format!("{}@{}", crate_name, version);

    // `cargo binstall` does not support `--path` (technically, there is manifest-path, but that's a bit different)
    if path.is_none() {
        // Try cargo binstall first (faster, uses prebuilt binaries)
        tracing::info!("Attempting cargo binstall for {}", crate_spec);
        let mut args = vec!["binstall", "--no-confirm", "--root"];
        if let Some(git) = &git {
            args.push("--git");
            args.push(git);
        }
        args.extend([cache_dir.to_str().unwrap(), &crate_spec]);
        let binstall_result = Command::new("cargo").args(args).output();

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
    }

    // Fall back to cargo install
    tracing::info!("Falling back to cargo install for {}", crate_spec);
    let mut args = vec!["install", "--root"];
    if let Some(path) = &path {
        args.push("--path");
        args.push(path);
    }
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

/// Get the current platform key for binary distribution lookup
pub fn get_platform_key() -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    match (os, arch) {
        ("macos", "aarch64") => "darwin-aarch64".to_string(),
        ("macos", "x86_64") => "darwin-x86_64".to_string(),
        ("linux", "x86_64") => "linux-x86_64".to_string(),
        ("linux", "aarch64") => "linux-aarch64".to_string(),
        ("windows", "x86_64") => "windows-x86_64".to_string(),
        _ => format!("{}-{}", os, arch),
    }
}

pub fn get_binary_cache_dir<'sym>(
    sym: &'sym Symposium,
    krate: &str,
    version: &str,
) -> Result<PathBuf> {
    let path = sym.cache_dir().join("binaries").join(krate).join(version);
    Ok(path)
}

/// Resolve a local installation: the command path is taken as-is.
fn resolve_local(local: &LocalInstall) -> Result<PathBuf> {
    Ok(local.command.clone().into())
}

/// Resolve a cargo installation: install if missing, return the binary path.
async fn resolve_cargo(sym: &Symposium, cargo: &CargoInstall) -> Result<PathBuf> {
    // Query crates.io for version and binary names
    let (version, bin_names) =
        query_crate_binaries(&cargo.crate_name, cargo.version.as_deref()).await?;

    // Determine binary name
    let binary_name = match &cargo.binary {
        Some(name) => name.clone(),
        None => {
            if bin_names.is_empty() {
                bail!("Crate '{}' has no binary targets", cargo.crate_name);
            } else if bin_names.len() == 1 {
                bin_names[0].clone()
            } else {
                bail!(
                    "Crate '{}' has multiple binaries {:?}, please specify one explicitly",
                    cargo.crate_name,
                    bin_names
                );
            }
        }
    };

    let cache_dir = get_binary_cache_dir(sym, &cargo.crate_name, &version)?;
    let binary_path = cache_dir
        .join("bin")
        .join(platform_binary_exe(&binary_name));

    // Check if we need to install
    if !binary_path.exists() {
        install_cargo_crate(
            &cargo.crate_name,
            &version,
            Some(binary_name),
            cache_dir,
            None,
            None,
        )
        .await?;
    }

    Ok(binary_path)
}

/// Resolve a per-platform binary archive: download and extract if missing,
/// return the path to the executable.
async fn resolve_binary(
    sym: &Symposium,
    binary_map: &BTreeMap<String, BinaryInstall>,
) -> Result<PathBuf> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let platform_key = get_platform_key();
    let binary = binary_map
        .get(&platform_key)
        .with_context(|| format!("No binary available for platform {}", platform_key))?;

    // Use a hash of the archive URL as the version for caching
    let mut hasher = DefaultHasher::new();
    binary.archive.hash(&mut hasher);
    let version = format!("{:016x}", hasher.finish());

    let cache_dir = get_binary_cache_dir(sym, "binary", &version)?;
    let executable = binary.cmd.trim_start_matches("./");
    let executable_path = cache_dir.join(executable);

    // Check if we need to download
    if !executable_path.exists() {
        download_and_cache_binary("binary", &version, binary, cache_dir).await?;
    }

    Ok(executable_path)
}

/// Acquire a github source: returns the cache directory containing the repo
/// (or its requested subtree). The caller picks a sub-path inside.
pub(crate) async fn acquire_github(sym: &Symposium, git: &GithubInstall) -> Result<PathBuf> {
    let git_url = &git.url;
    let source = crate::installation::git::parse_github_url(git_url)?;
    let cache_mgr = crate::installation::git::GitCacheManager::new(sym, "plugins");
    cache_mgr
        .get_or_fetch(&source, git_url, crate::plugins::UpdateLevel::Check)
        .await
}

/// The two runnable forms `resolve_installation` can produce.
#[derive(Debug)]
pub enum ResolvedCommand {
    Exec(PathBuf),
    Shell { command: String, args: Vec<String> },
}

/// Acquire (and possibly install) an installation, optionally resolving it
/// to a runnable.
///
/// Returns `Ok(Some(_))` when the installation produced something runnable
/// (a binary path or a shell command) and `Ok(None)` when it was acquired
/// without picking an executable — currently only github installations
/// referenced without any sub-path. Requirements-style callers treat both
/// as success; command-style callers treat `None` as an error.
pub async fn resolve_installation(
    sym: &Symposium,
    kind: &InstallationKind,
    sub_path: Option<&str>,
) -> Result<Option<ResolvedCommand>> {
    match kind {
        InstallationKind::Local(local) => Ok(Some(ResolvedCommand::Exec(resolve_local(local)?))),
        InstallationKind::Shell(s) => Ok(Some(ResolvedCommand::Shell {
            command: s.command.clone(),
            args: s.args.clone(),
        })),
        InstallationKind::Cargo(cargo) => {
            Ok(Some(ResolvedCommand::Exec(resolve_cargo(sym, cargo).await?)))
        }
        InstallationKind::Binary(binary_map) => Ok(Some(ResolvedCommand::Exec(
            resolve_binary(sym, binary_map).await?,
        ))),
        InstallationKind::Github(git) => {
            let cache_dir = acquire_github(sym, git).await?;
            match sub_path.or(git.path.as_deref()) {
                Some(chosen) => {
                    let file_path = cache_dir.join(chosen);
                    if !file_path.exists() {
                        bail!(
                            "path '{}' does not exist in cached repository {}",
                            chosen,
                            git.url
                        );
                    }
                    Ok(Some(ResolvedCommand::Exec(file_path)))
                }
                None => Ok(None),
            }
        }
    }
}

/// Download and cache a binary distribution
async fn download_and_cache_binary(
    agent_id: &str,
    version: &str,
    binary: &BinaryInstall,
    cache_dir: PathBuf,
) -> Result<()> {
    let agent_id = agent_id.to_string();
    let version = version.to_string();
    let binary = binary.clone();
    tokio::task::spawn_blocking(move || {
        download_and_cache_binary_sync(&agent_id, &version, &binary, &cache_dir)
    })
    .await
    .context("Download task panicked")?
}

/// Download and cache a binary distribution (blocking implementation)
fn download_and_cache_binary_sync(
    agent_id: &str,
    version: &str,
    binary: &BinaryInstall,
    cache_dir: &PathBuf,
) -> Result<()> {
    use std::fs;
    use std::io::Write;

    // Clean up old versions first
    if let Some(parent) = cache_dir.parent() {
        if parent.exists() {
            for entry in fs::read_dir(parent)? {
                let entry = entry?;
                let path = entry.path();
                if path != *cache_dir && path.is_dir() {
                    fs::remove_dir_all(&path).ok();
                }
            }
        }
    }

    // Create cache directory
    fs::create_dir_all(cache_dir)?;

    // Download the binary
    let response = reqwest::blocking::get(&binary.archive)
        .with_context(|| format!("Failed to download binary for {}", agent_id))?;

    if !response.status().is_success() {
        bail!(
            "Failed to download binary for {}: {} {}",
            agent_id,
            response.status().as_u16(),
            response.status().canonical_reason().unwrap_or("Unknown")
        );
    }

    let bytes = response.bytes()?;

    // Determine filename from URL
    let url = url::Url::parse(&binary.archive)?;
    let filename = url
        .path_segments()
        .and_then(|s| s.last())
        .unwrap_or("download");
    let download_path = cache_dir.join(filename);

    // Write to disk
    let mut file = fs::File::create(&download_path)?;
    file.write_all(&bytes)?;

    // Extract if archive
    if filename.ends_with(".tar.gz") || filename.ends_with(".tgz") {
        extract_tar_gz(&download_path, cache_dir)?;
        fs::remove_file(&download_path).ok();
    } else if filename.ends_with(".zip") {
        extract_zip(&download_path, cache_dir)?;
        fs::remove_file(&download_path).ok();
    }

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let executable = binary.cmd.trim_start_matches("./");
        let executable_path = cache_dir.join(executable);
        if executable_path.exists() {
            let mut perms = fs::metadata(&executable_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&executable_path, perms)?;
        }
    }

    tracing::info!(
        "Downloaded and cached {} v{} to {}",
        agent_id,
        version,
        cache_dir.display()
    );

    Ok(())
}

/// Extract a tar.gz archive
fn extract_tar_gz(archive_path: &PathBuf, dest_dir: &PathBuf) -> Result<()> {
    use flate2::read::GzDecoder;
    use std::fs::File;
    use tar::Archive;

    let file = File::open(archive_path)?;
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);
    archive.unpack(dest_dir)?;

    Ok(())
}

/// Extract a zip archive
fn extract_zip(archive_path: &PathBuf, dest_dir: &PathBuf) -> Result<()> {
    use std::fs::File;
    use zip::ZipArchive;

    let file = File::open(archive_path)?;
    let mut archive = ZipArchive::new(file)?;
    archive.extract(dest_dir)?;

    Ok(())
}
