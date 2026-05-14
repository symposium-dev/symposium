//! Self-update: check for newer versions and install them.
//!
//! Prefers downloading a prebuilt binary from the GitHub release.
//! Falls back to `cargo install` when no prebuilt binary is available
//! for the current platform.

use std::io::Cursor;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::config::{Symposium, UpdateSource};
use crate::output::Output;
use crate::state::CURRENT_VERSION;

const CRATE_NAME: &str = "symposium";
const USER_AGENT: &str = "symposium (https://github.com/symposium-dev/symposium)";
const REPO_URL: &str = "https://github.com/symposium-dev/symposium";
const BINARY_NAME: &str = "cargo-agents";

/// Query the registry for the latest published version of symposium
/// by running `cargo search symposium --limit 1`.
pub fn latest_version(sym: &Symposium) -> Result<semver::Version> {
    let output = sym
        .cargo_command()
        .args(["search", CRATE_NAME, "--limit", "1"])
        .output()
        .context("failed to run cargo search")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("cargo search failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_cargo_search_output(&stdout)
}

/// Parse the version from `cargo search` output.
///
/// Expected format: `symposium = "0.3.0"    # AI the Rust Way`
fn parse_cargo_search_output(output: &str) -> Result<semver::Version> {
    let version_str = output
        .lines()
        .next()
        .and_then(|line| line.strip_prefix(&format!("{CRATE_NAME} = \"")))
        .and_then(|rest| rest.split('"').next())
        .context("unexpected cargo search output")?;

    semver::Version::parse(version_str).context("failed to parse version from cargo search")
}

/// Check whether a newer version is available.  Returns `Some(latest)`
/// if an upgrade is available, `None` if we're current or ahead.
pub fn check_upgrade(sym: &Symposium) -> Result<Option<semver::Version>> {
    let current =
        semver::Version::parse(CURRENT_VERSION).context("failed to parse current version")?;
    let latest = latest_version(sym)?;
    if latest > current {
        Ok(Some(latest))
    } else {
        Ok(None)
    }
}

/// Run the self-update using the configured source strategy.
pub async fn self_update(sym: &Symposium, out: &Output) -> Result<()> {
    let target_version = match check_upgrade(sym) {
        Ok(Some(latest)) => {
            out.info(format!("updating symposium {CURRENT_VERSION} → {latest}"));
            latest
        }
        Ok(None) => {
            out.already_ok(format!("symposium {CURRENT_VERSION} is up to date"));
            return Ok(());
        }
        Err(e) => {
            out.warn(format!("could not check for updates: {e}"));
            return Err(e);
        }
    };

    match sym.config.update_source {
        UpdateSource::Source => {
            cargo_install(sym)?;
            out.done(format!("updated to {target_version} (cargo install)"));
        }
        UpdateSource::Binary => {
            let install_dir = cargo_bin_dir()?;
            match download_release(&target_version, &install_dir).await {
                Ok(()) => {
                    out.done(format!("updated to {target_version} (prebuilt binary)"));
                }
                Err(e) => {
                    tracing::warn!("prebuilt download failed: {e:#}");
                    out.info("prebuilt binary not available, falling back to cargo install...");
                    cargo_install(sym)?;
                    out.done(format!("updated to {target_version} (cargo install)"));
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Prebuilt binary download
// ---------------------------------------------------------------------------

fn target_triple() -> &'static str {
    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    {
        "aarch64-apple-darwin"
    }
    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    {
        "x86_64-unknown-linux-musl"
    }
    #[cfg(all(target_arch = "aarch64", target_os = "linux"))]
    {
        "aarch64-unknown-linux-musl"
    }
    #[cfg(all(target_arch = "x86_64", target_os = "windows"))]
    {
        "x86_64-pc-windows-msvc"
    }
}

fn release_url(version: &semver::Version) -> String {
    let target = target_triple();
    let ext = if cfg!(windows) { "zip" } else { "tar.gz" };
    format!("{REPO_URL}/releases/download/{CRATE_NAME}-v{version}/{BINARY_NAME}-{target}.{ext}")
}

async fn download_release(version: &semver::Version, install_dir: &std::path::Path) -> Result<()> {
    let url = release_url(version);
    tracing::info!(%url, "downloading release");

    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .context("failed to build HTTP client")?;

    let response = client
        .get(&url)
        .send()
        .await
        .context("failed to download release")?;

    if !response.status().is_success() {
        bail!("download failed: HTTP {} from {url}", response.status());
    }

    let bytes = response
        .bytes()
        .await
        .context("failed to read response body")?;

    let binary_bytes = if cfg!(windows) {
        extract_zip(&bytes)?
    } else {
        extract_tarball(&bytes)?
    };

    install_binary(&binary_bytes, install_dir)?;
    Ok(())
}

fn extract_tarball(archive_bytes: &[u8]) -> Result<Vec<u8>> {
    use flate2::read::GzDecoder;
    use std::io::Read;
    use tar::Archive;

    let gz = GzDecoder::new(Cursor::new(archive_bytes));
    let mut archive = Archive::new(gz);

    for entry in archive.entries().context("failed to read tar entries")? {
        let mut entry = entry.context("failed to read tar entry")?;
        let path = entry.path().context("failed to read entry path")?;

        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();

        if file_name == BINARY_NAME {
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .context("failed to read binary from archive")?;
            return Ok(buf);
        }
    }

    bail!("{BINARY_NAME} not found in archive");
}

#[cfg(windows)]
fn extract_zip(archive_bytes: &[u8]) -> Result<Vec<u8>> {
    use std::io::Read;

    let reader = Cursor::new(archive_bytes);
    let mut archive = zip::ZipArchive::new(reader).context("failed to read zip archive")?;

    let exe_name = format!("{BINARY_NAME}.exe");
    let mut file = archive
        .by_name(&exe_name)
        .context("binary not found in zip")?;

    let mut buf = Vec::new();
    file.read_to_end(&mut buf)
        .context("failed to read binary from zip")?;
    Ok(buf)
}

#[cfg(not(windows))]
fn extract_zip(_archive_bytes: &[u8]) -> Result<Vec<u8>> {
    bail!("zip extraction not expected on this platform");
}

fn install_binary(binary_bytes: &[u8], install_dir: &std::path::Path) -> Result<()> {
    use std::fs;

    let bin_name = if cfg!(windows) {
        format!("{BINARY_NAME}.exe")
    } else {
        BINARY_NAME.to_string()
    };
    let dest = install_dir.join(&bin_name);

    // Write to a temp file in the same directory, then atomically rename.
    let tmp = tempfile::NamedTempFile::new_in(install_dir)
        .context("failed to create temp file for binary")?;

    fs::write(tmp.path(), binary_bytes).context("failed to write binary")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o755))
            .context("failed to set executable permissions")?;
    }

    tmp.persist(&dest).context("failed to replace binary")?;

    tracing::info!(path = %dest.display(), "installed updated binary");
    Ok(())
}

// ---------------------------------------------------------------------------
// Fallback: cargo install
// ---------------------------------------------------------------------------

fn cargo_install(sym: &Symposium) -> Result<()> {
    let status = sym
        .cargo_command()
        .args(["install", CRATE_NAME, "--force"])
        .status()
        .context("failed to run cargo install")?;

    if !status.success() {
        bail!("cargo install failed with exit code {:?}", status.code());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn cargo_bin_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("CARGO_HOME") {
        return Ok(PathBuf::from(dir).join("bin"));
    }
    let home = dirs::home_dir().context("could not determine home directory")?;
    Ok(home.join(".cargo").join("bin"))
}

/// Re-execute the current process (replaces the process via exec on Unix,
/// spawn-and-exit on Windows).
pub fn re_exec() -> ! {
    let args: Vec<_> = std::env::args_os().collect();
    let program = &args[0];

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = Command::new(program).args(&args[1..]).exec();
        eprintln!("failed to re-exec: {err}");
        std::process::exit(1);
    }

    #[cfg(not(unix))]
    {
        let status = Command::new(program)
            .args(&args[1..])
            .status()
            .unwrap_or_else(|e| {
                eprintln!("failed to re-exec: {e}");
                std::process::exit(1);
            });
        std::process::exit(status.code().unwrap_or(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_search_output_typical() {
        let output = r#"symposium = "1.2.3"    # AI the Rust Way
... and 105 crates more (use --limit N to see more)
"#;
        let v = parse_cargo_search_output(output).unwrap();
        assert_eq!(v, semver::Version::new(1, 2, 3));
    }

    #[test]
    fn parse_search_output_prerelease() {
        let output = "symposium = \"0.4.0-beta.1\"    # AI the Rust Way\n";
        let v = parse_cargo_search_output(output).unwrap();
        assert_eq!(v.to_string(), "0.4.0-beta.1");
    }

    #[test]
    fn parse_search_output_no_description() {
        let output = "symposium = \"0.3.0\"\n";
        let v = parse_cargo_search_output(output).unwrap();
        assert_eq!(v, semver::Version::new(0, 3, 0));
    }

    #[test]
    fn parse_search_output_wrong_crate() {
        let output = "something-else = \"1.0.0\"    # Not symposium\n";
        assert!(parse_cargo_search_output(output).is_err());
    }

    #[test]
    fn parse_search_output_empty() {
        assert!(parse_cargo_search_output("").is_err());
    }

    #[test]
    fn extract_tarball_finds_binary() {
        let tarball = build_test_tarball("cargo-agents", b"fake-binary-content");
        let result = extract_tarball(&tarball).unwrap();
        assert_eq!(result, b"fake-binary-content");
    }

    #[test]
    fn extract_tarball_missing_binary() {
        let tarball = build_test_tarball("wrong-name", b"data");
        assert!(extract_tarball(&tarball).is_err());
    }

    #[test]
    fn install_binary_creates_executable() {
        let tmp = tempfile::tempdir().unwrap();
        install_binary(b"#!/bin/sh\necho hello", tmp.path()).unwrap();

        let installed = tmp.path().join(BINARY_NAME);
        assert!(installed.exists());
        assert_eq!(std::fs::read(&installed).unwrap(), b"#!/bin/sh\necho hello");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&installed).unwrap().permissions().mode();
            assert!(mode & 0o111 != 0, "binary should be executable");
        }
    }

    fn build_test_tarball(filename: &str, content: &[u8]) -> Vec<u8> {
        use flate2::Compression;
        use flate2::write::GzEncoder;

        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        {
            let mut builder = tar::Builder::new(&mut encoder);
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, filename, content).unwrap();
            builder.finish().unwrap();
        }
        encoder.finish().unwrap()
    }
}
