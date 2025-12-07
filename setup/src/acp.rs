//! ACP binary installation

use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Install ACP binaries: sacp-conductor, elizacp, sacp-tee from crates.io,
/// and symposium-acp-proxy from local repository
pub fn install_acp_binaries(repo_root: &Path, dry_run: bool) -> Result<()> {
    println!("📦 Installing ACP binaries...");

    // Verify we're in the symposium repository
    verify_symposium_repo(repo_root)?;

    // Install from crates.io
    install_from_crates_io(&["sacp-conductor", "elizacp", "sacp-tee"], dry_run)?;

    // Install symposium-acp-proxy from local repository
    install_symposium_acp_proxy(repo_root, dry_run)?;

    if !dry_run {
        println!("✅ ACP binaries installed successfully!");
    }
    Ok(())
}

/// Verify we're in a repository with symposium-acp-proxy in the workspace
fn verify_symposium_repo(repo_root: &Path) -> Result<()> {
    let cargo_toml = repo_root.join("Cargo.toml");
    if !cargo_toml.exists() {
        return Err(anyhow!(
            "❌ Not in a Cargo workspace. Cargo.toml not found at: {}",
            cargo_toml.display()
        ));
    }

    let contents = std::fs::read_to_string(&cargo_toml).context("Failed to read Cargo.toml")?;

    if !contents.contains("symposium-acp-proxy") {
        return Err(anyhow!(
            "❌ This doesn't appear to be the symposium repository.\n   Expected to find 'symposium-acp-proxy' in workspace members."
        ));
    }

    Ok(())
}

/// Install binaries from crates.io
fn install_from_crates_io(crates: &[&str], dry_run: bool) -> Result<()> {
    for crate_name in crates {
        if dry_run {
            println!("   Would install {} from crates.io", crate_name);
        } else {
            println!("   Installing {} from crates.io...", crate_name);

            let output = Command::new("cargo")
                .args(["install", crate_name, "--force"])
                .output()
                .context(format!("Failed to execute cargo install {}", crate_name))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(anyhow!(
                    "❌ Failed to install {}:\n   Error: {}",
                    crate_name,
                    stderr.trim()
                ));
            }

            println!("   ✅ {} installed", crate_name);
        }
    }

    Ok(())
}

/// Install symposium-acp-proxy from local repository
fn install_symposium_acp_proxy(repo_root: &Path, dry_run: bool) -> Result<()> {
    let symposium_acp_proxy_dir = repo_root.join("src/symposium-acp-proxy");

    if !symposium_acp_proxy_dir.exists() {
        return Err(anyhow!(
            "❌ symposium-acp-proxy directory not found at: {}",
            symposium_acp_proxy_dir.display()
        ));
    }

    println!("   Path: {}", symposium_acp_proxy_dir.display());

    if dry_run {
        println!("   Would install symposium-acp-proxy from local repository");
    } else {
        println!("   Installing symposium-acp-proxy from local repository...");

        let output = Command::new("cargo")
            .args(["install", "--path", ".", "--force"])
            .current_dir(&symposium_acp_proxy_dir)
            .output()
            .context("Failed to execute cargo install for symposium-acp-proxy")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!(
                "❌ Failed to install symposium-acp-proxy:\n   Error: {}",
                stderr.trim()
            ));
        }

        println!("   ✅ symposium-acp-proxy installed");
    }
    Ok(())
}

/// Get the expected installation path for ACP binaries
pub fn get_binary_path(binary_name: &str) -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME environment variable not set")?;
    Ok(PathBuf::from(home).join(".cargo/bin").join(binary_name))
}
