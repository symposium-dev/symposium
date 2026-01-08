//! ACP binary installation

use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Install ACP binaries from local repository
pub fn install_acp_binaries(repo_root: &Path, dry_run: bool) -> Result<()> {
    println!("📦 Installing ACP binaries...");

    // Verify we're in the symposium repository
    verify_symposium_repo(repo_root)?;

    // Install symposium-acp-agent from local repository
    install_local_binaries(repo_root, dry_run)?;

    if !dry_run {
        println!("✅ ACP binaries installed successfully!");
    }
    Ok(())
}

/// Verify we're in a repository with symposium-acp-agent in the workspace
fn verify_symposium_repo(repo_root: &Path) -> Result<()> {
    let cargo_toml = repo_root.join("Cargo.toml");
    if !cargo_toml.exists() {
        return Err(anyhow!(
            "❌ Not in a Cargo workspace. Cargo.toml not found at: {}",
            cargo_toml.display()
        ));
    }

    let contents = std::fs::read_to_string(&cargo_toml).context("Failed to read Cargo.toml")?;

    if !contents.contains("symposium-acp-agent") {
        return Err(anyhow!(
            "❌ This doesn't appear to be the symposium repository.\n   Expected to find 'symposium-acp-agent' in workspace members."
        ));
    }

    Ok(())
}

/// Install local symposium binaries from the repository
fn install_local_binaries(repo_root: &Path, dry_run: bool) -> Result<()> {
    for binary_name in ["symposium-acp-agent"] {
        let binary_dir = repo_root.join("src").join(binary_name);

        if !binary_dir.exists() {
            return Err(anyhow!(
                "❌ {} directory not found at: {}",
                binary_name,
                binary_dir.display()
            ));
        }

        if dry_run {
            println!("   Would install {} from local repository", binary_name);
        } else {
            println!("   Installing {} from local repository...", binary_name);

            let output = Command::new("cargo")
                .args(["install", "--path", ".", "--force"])
                .current_dir(&binary_dir)
                .output()
                .context(format!(
                    "Failed to execute cargo install for {}",
                    binary_name
                ))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(anyhow!(
                    "❌ Failed to install {}:\n   Error: {}",
                    binary_name,
                    stderr.trim()
                ));
            }

            println!("   ✅ {} installed", binary_name);
        }
    }
    Ok(())
}

/// Get the expected installation path for ACP binaries
pub fn get_binary_path(binary_name: &str) -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME environment variable not set")?;
    Ok(PathBuf::from(home).join(".cargo/bin").join(binary_name))
}
