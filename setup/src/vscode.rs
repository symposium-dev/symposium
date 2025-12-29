//! VSCode extension build and installation

use anyhow::{Context, Result, anyhow};
use std::path::Path;
use std::process::Command;

/// Build and install the VSCode extension
pub fn build_and_install_extension(repo_root: &Path, dry_run: bool) -> Result<()> {
    let extension_dir = repo_root.join("vscode-extension");

    if !extension_dir.exists() {
        return Err(anyhow!(
            "❌ VSCode extension directory not found at: {}",
            extension_dir.display()
        ));
    }

    println!("📦 Building VSCode extension...");

    if dry_run {
        println!("   Would build symposium-acp-agent (cargo build --release)");
        println!("   Would copy binary to extension bin/ directory");
        println!("   Would install dependencies (npm install)");
        println!("   Would build extension (npm run webpack-dev)");
        println!("   Would package extension (npx vsce package)");
        println!("   Would install extension (code --install-extension)");
    } else {
        // Build the release binary
        build_agent_binary(repo_root)?;

        // Copy the symposium-acp-agent binary into the extension
        copy_binary_to_extension(repo_root, &extension_dir)?;

        // Install dependencies
        install_dependencies(&extension_dir)?;

        // Build extension
        build_extension(&extension_dir)?;

        // Package extension
        package_extension(&extension_dir)?;

        // Find and install the .vsix file
        install_extension(&extension_dir)?;

        println!("✅ VSCode extension installed successfully!");
    }
    Ok(())
}

/// Build the symposium-acp-agent binary in release mode
fn build_agent_binary(repo_root: &Path) -> Result<()> {
    println!("🔧 Building symposium-acp-agent (release)...");

    let status = Command::new("cargo")
        .args(["build", "--release", "-p", "symposium-acp-agent"])
        .current_dir(repo_root)
        .status()
        .context("Failed to execute cargo build")?;

    if !status.success() {
        return Err(anyhow!("❌ Failed to build symposium-acp-agent"));
    }

    Ok(())
}

/// Copy the symposium-acp-agent binary into the extension's bin directory
fn copy_binary_to_extension(repo_root: &Path, extension_dir: &Path) -> Result<()> {
    println!("📋 Copying symposium-acp-agent binary...");

    // Determine the binary name based on platform
    let binary_name = if cfg!(target_os = "windows") {
        "symposium-acp-agent.exe"
    } else {
        "symposium-acp-agent"
    };

    // Source: target/release (we just built it)
    let source = repo_root.join("target").join("release").join(binary_name);

    if !source.exists() {
        return Err(anyhow!(
            "❌ symposium-acp-agent binary not found at {}",
            source.display()
        ));
    }

    // Destination: vscode-extension/bin/<platform>-<arch>/
    // Use Node.js naming conventions (darwin/win32/linux, arm64/x64)
    let platform = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => other,
    };
    let platform_dir = format!("{}-{}", platform, arch);

    let dest_dir = extension_dir.join("bin").join(&platform_dir);
    std::fs::create_dir_all(&dest_dir)
        .with_context(|| format!("Failed to create directory: {}", dest_dir.display()))?;

    let dest = dest_dir.join(binary_name);

    std::fs::copy(&source, &dest).with_context(|| {
        format!(
            "Failed to copy binary from {} to {}",
            source.display(),
            dest.display()
        )
    })?;

    println!("   Copied {} to {}", source.display(), dest.display());

    Ok(())
}

/// Install npm dependencies
fn install_dependencies(extension_dir: &Path) -> Result<()> {
    println!("📥 Installing extension dependencies...");

    let output = Command::new("npm")
        .args(["install"])
        .current_dir(extension_dir)
        .output()
        .context("Failed to execute npm install")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "❌ Failed to install extension dependencies:\n   Error: {}",
            stderr.trim()
        ));
    }

    Ok(())
}

/// Build the extension
fn build_extension(extension_dir: &Path) -> Result<()> {
    println!("🔨 Building extension...");

    let output = Command::new("npm")
        .args(["run", "webpack-dev"])
        .current_dir(extension_dir)
        .output()
        .context("Failed to execute npm run webpack-dev")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "❌ Failed to build extension:\n   Error: {}",
            stderr.trim()
        ));
    }

    Ok(())
}

/// Package the extension as .vsix
fn package_extension(extension_dir: &Path) -> Result<()> {
    println!("📦 Packaging VSCode extension...");

    // Clean up any old .vsix files first to avoid ambiguity
    clean_old_vsix_files(extension_dir)?;

    let output = Command::new("npx")
        .args(["vsce", "package", "--no-dependencies"])
        .current_dir(extension_dir)
        .output()
        .context("Failed to execute vsce package")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "❌ Failed to package extension:\n   Error: {}",
            stderr.trim()
        ));
    }

    Ok(())
}

/// Install the packaged extension
fn install_extension(extension_dir: &Path) -> Result<()> {
    // Find the generated .vsix file
    let vsix_file = find_vsix_file(extension_dir)?;

    println!("📥 Installing VSCode extension: {}", vsix_file);

    let output = Command::new("code")
        .args(["--install-extension", &vsix_file])
        .current_dir(extension_dir)
        .output()
        .context("Failed to execute code --install-extension")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "❌ Failed to install VSCode extension:\n   Error: {}",
            stderr.trim()
        ));
    }

    Ok(())
}

/// Remove any existing .vsix files to avoid version conflicts
fn clean_old_vsix_files(extension_dir: &Path) -> Result<()> {
    let entries = std::fs::read_dir(extension_dir).context("Failed to read extension directory")?;

    for entry in entries {
        let entry = entry.context("Failed to read directory entry")?;
        let path = entry.path();
        if let Some(extension) = path.extension() {
            if extension == "vsix" {
                std::fs::remove_file(&path)
                    .with_context(|| format!("Failed to remove old vsix: {}", path.display()))?;
                println!(
                    "   Removed old package: {}",
                    path.file_name().unwrap().to_string_lossy()
                );
            }
        }
    }

    Ok(())
}

/// Find the .vsix file in the extension directory
fn find_vsix_file(extension_dir: &Path) -> Result<String> {
    let entries = std::fs::read_dir(extension_dir).context("Failed to read extension directory")?;

    for entry in entries {
        let entry = entry.context("Failed to read directory entry")?;
        let path = entry.path();
        if let Some(extension) = path.extension() {
            if extension == "vsix" {
                return Ok(path.file_name().unwrap().to_string_lossy().to_string());
            }
        }
    }

    Err(anyhow!("❌ No .vsix file found after packaging"))
}

/// Check if VSCode is available
pub fn check_vscode_available() -> Result<()> {
    if which::which("code").is_err() {
        return Err(anyhow!(
            "❌ VSCode 'code' command not found.\n   Please install VSCode and ensure the 'code' command is available.\n   Visit: https://code.visualstudio.com/"
        ));
    }
    Ok(())
}

/// Check if Node.js/npm is available
pub fn check_node_available() -> Result<()> {
    if which::which("npm").is_err() {
        return Err(anyhow!(
            "❌ npm not found.\n   Please install Node.js first.\n   Visit: https://nodejs.org/"
        ));
    }
    Ok(())
}
