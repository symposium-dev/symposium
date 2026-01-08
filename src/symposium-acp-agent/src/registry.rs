//! Agent registry - fetching and resolving agents from the ACP registry.
//!
//! This module provides functionality to:
//! - Fetch the agent registry from GitHub
//! - Merge with built-in agents
//! - Resolve agent distributions to executable commands
//! - Download and cache binary distributions

use anyhow::{bail, Context, Result};
use sacp::schema::{EnvVariable, McpServer, McpServerStdio};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Registry URL - same as VSCode extension uses
const REGISTRY_URL: &str =
    "https://github.com/agentclientprotocol/registry/releases/latest/download/registry.json";

// ============================================================================
// Registry Types (matching the registry JSON format)
// ============================================================================

/// The full registry JSON structure
#[derive(Debug, Clone, Deserialize)]
pub struct RegistryJson {
    pub version: String,
    pub agents: Vec<RegistryEntry>,
    #[serde(default)]
    pub extensions: Vec<RegistryEntry>,
}

/// A single entry in the registry (agent or extension)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RegistryEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    pub distribution: Distribution,
}

/// Distribution methods for spawning an agent
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Distribution {
    #[serde(default)]
    pub local: Option<LocalDistribution>,
    #[serde(default)]
    pub npx: Option<NpxDistribution>,
    #[serde(default)]
    pub pipx: Option<PipxDistribution>,
    #[serde(default)]
    pub binary: Option<HashMap<String, BinaryDistribution>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LocalDistribution {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NpxDistribution {
    pub package: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PipxDistribution {
    pub package: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BinaryDistribution {
    pub archive: String,
    pub cmd: String,
    #[serde(default)]
    pub args: Vec<String>,
}

// ============================================================================
// Output Types (for JSON output from subcommands)
// ============================================================================

/// Agent listing entry - what `registry list` outputs
#[derive(Debug, Clone, Serialize)]
pub struct AgentListEntry {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// ============================================================================
// Built-in Agents
// ============================================================================

/// Get the path to the current executable (for built-in agents)
fn current_exe() -> Result<PathBuf> {
    std::env::current_exe().context("Failed to get current executable path")
}

/// Built-in agents that are always available
pub fn built_in_agents() -> Result<Vec<RegistryEntry>> {
    let exe = current_exe()?;
    let exe_str = exe.to_string_lossy().to_string();

    Ok(vec![RegistryEntry {
        id: "elizacp".to_string(),
        name: "ElizACP".to_string(),
        version: String::new(),
        description: Some("Built-in Eliza agent for testing".to_string()),
        distribution: Distribution {
            local: Some(LocalDistribution {
                command: exe_str.clone(),
                args: vec!["eliza".to_string()],
                env: HashMap::new(),
            }),
            npx: None,
            pipx: None,
            binary: None,
        },
    }])
}

// ============================================================================
// Registry Fetching
// ============================================================================

/// Fetch the registry from GitHub
pub async fn fetch_registry() -> Result<RegistryJson> {
    let response = reqwest::get(REGISTRY_URL)
        .await
        .context("Failed to fetch registry")?;

    if !response.status().is_success() {
        bail!(
            "Failed to fetch registry: {} {}",
            response.status().as_u16(),
            response.status().canonical_reason().unwrap_or("Unknown")
        );
    }

    let registry: RegistryJson = response
        .json()
        .await
        .context("Failed to parse registry JSON")?;

    Ok(registry)
}

/// List all available agents (built-ins + registry)
pub async fn list_agents() -> Result<Vec<AgentListEntry>> {
    // Start with built-ins
    let mut agents: Vec<AgentListEntry> = built_in_agents()?
        .into_iter()
        .map(|e| AgentListEntry {
            id: e.id,
            name: e.name,
            version: if e.version.is_empty() {
                None
            } else {
                Some(e.version)
            },
            description: e.description,
        })
        .collect();

    // Fetch and merge registry agents
    let registry = fetch_registry().await?;
    for entry in registry.agents {
        // Skip if we already have this agent (built-in takes precedence)
        if agents.iter().any(|a| a.id == entry.id) {
            continue;
        }
        agents.push(AgentListEntry {
            id: entry.id,
            name: entry.name,
            version: if entry.version.is_empty() {
                None
            } else {
                Some(entry.version)
            },
            description: entry.description,
        });
    }

    Ok(agents)
}

/// Extension listing entry - what `registry list-extensions` outputs
#[derive(Debug, Clone, Serialize)]
pub struct ExtensionListEntry {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// List all available extensions from the registry
pub async fn list_extensions() -> Result<Vec<ExtensionListEntry>> {
    let registry = fetch_registry().await?;

    let extensions: Vec<ExtensionListEntry> = registry
        .extensions
        .into_iter()
        .map(|e| ExtensionListEntry {
            id: e.id,
            name: e.name,
            version: if e.version.is_empty() {
                None
            } else {
                Some(e.version)
            },
            description: e.description,
        })
        .collect();

    Ok(extensions)
}

// ============================================================================
// Distribution Resolution
// ============================================================================

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

/// Get the cache directory for binary agents
pub fn get_binary_cache_dir(agent_id: &str, version: &str) -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not determine home directory")?;
    Ok(home
        .join(".symposium")
        .join("bin")
        .join(agent_id)
        .join(version))
}

/// Resolve an agent ID to an McpServer configuration
pub async fn resolve_agent(agent_id: &str) -> Result<McpServer> {
    // Check built-ins first
    for agent in built_in_agents()? {
        if agent.id == agent_id {
            return resolve_distribution(&agent);
        }
    }

    // Fetch registry and find the agent
    let registry = fetch_registry().await?;
    let entry = registry
        .agents
        .into_iter()
        .find(|a| a.id == agent_id)
        .with_context(|| format!("Agent '{}' not found in registry", agent_id))?;

    resolve_distribution(&entry)
}

/// Resolve a registry entry's distribution to an McpServer
fn resolve_distribution(entry: &RegistryEntry) -> Result<McpServer> {
    let dist = &entry.distribution;

    // Priority: local > npx > pipx > binary

    if let Some(local) = &dist.local {
        let env: Vec<EnvVariable> = local
            .env
            .iter()
            .map(|(k, v)| EnvVariable::new(k.clone(), v.clone()))
            .collect();

        return Ok(McpServer::Stdio(
            McpServerStdio::new(&entry.name, &local.command)
                .args(local.args.clone())
                .env(env),
        ));
    }

    if let Some(npx) = &dist.npx {
        let mut args = vec!["-y".to_string(), npx.package.clone()];
        args.extend(npx.args.clone());

        let env: Vec<EnvVariable> = npx
            .env
            .iter()
            .map(|(k, v)| EnvVariable::new(k.clone(), v.clone()))
            .collect();

        return Ok(McpServer::Stdio(
            McpServerStdio::new(&entry.name, "npx").args(args).env(env),
        ));
    }

    if let Some(pipx) = &dist.pipx {
        let mut args = vec!["run".to_string(), pipx.package.clone()];
        args.extend(pipx.args.clone());

        return Ok(McpServer::Stdio(
            McpServerStdio::new(&entry.name, "pipx").args(args),
        ));
    }

    if let Some(binary_map) = &dist.binary {
        let platform_key = get_platform_key();
        if let Some(binary) = binary_map.get(&platform_key) {
            let version = if entry.version.is_empty() {
                "latest"
            } else {
                &entry.version
            };
            let cache_dir = get_binary_cache_dir(&entry.id, version)?;
            let executable = binary.cmd.trim_start_matches("./");
            let executable_path = cache_dir.join(executable);

            // Check if we need to download
            if !executable_path.exists() {
                // For now, we'll do blocking download. Could make this async in future.
                download_and_cache_binary(&entry.id, version, binary, &cache_dir)?;
            }

            return Ok(McpServer::Stdio(
                McpServerStdio::new(&entry.name, executable_path).args(binary.args.clone()),
            ));
        }
    }

    bail!(
        "No compatible distribution found for agent '{}' on platform {}",
        entry.id,
        get_platform_key()
    );
}

/// Download and cache a binary distribution
fn download_and_cache_binary(
    agent_id: &str,
    version: &str,
    binary: &BinaryDistribution,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_key() {
        let key = get_platform_key();
        // Should be one of the expected formats
        assert!(
            key.contains('-'),
            "Platform key should contain a hyphen: {}",
            key
        );
    }

    #[test]
    fn test_built_in_agents() {
        let agents = built_in_agents().unwrap();
        assert!(!agents.is_empty());

        let elizacp = agents.iter().find(|a| a.id == "elizacp");
        assert!(elizacp.is_some(), "Should have elizacp built-in");
    }
}
