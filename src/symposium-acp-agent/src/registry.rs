//! Agent registry - fetching and resolving agents from the ACP registry.
//!
//! This module provides functionality to:
//! - Fetch the agent registry from GitHub
//! - Merge with built-in agents
//! - Resolve agent distributions to executable commands
//! - Download and cache binary distributions

use crate::user_config::ConfigPaths;
use anyhow::{Context, Result, bail};
use sacp::schema::{EnvVariable, McpServer, McpServerHttp, McpServerSse, McpServerStdio};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use symposium_recommendations::{
    BinaryDistribution, CargoDistribution, ComponentSource, HttpDistribution, LocalDistribution,
    NpxDistribution, PipxDistribution,
};

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
    pub mods: Vec<RegistryEntry>,
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
    #[serde(default)]
    pub cargo: Option<CargoDistribution>,
}

// ============================================================================
// ComponentSource Resolution Extension
// ============================================================================

/// Extension trait to add resolution capabilities to ComponentSource
pub trait ComponentSourceExt {
    /// Resolve this source to an McpServer that can be spawned
    fn resolve(&self) -> impl std::future::Future<Output = Result<McpServer>> + Send;
}

impl ComponentSourceExt for ComponentSource {
    async fn resolve(&self) -> Result<McpServer> {
        match self {
            ComponentSource::Builtin(name) => resolve_builtin(name).await,
            ComponentSource::Registry(id) => resolve_from_registry(id).await,
            ComponentSource::Url(url) => resolve_from_url(url).await,
            ComponentSource::Local(local) => resolve_local(local),
            ComponentSource::Npx(npx) => resolve_npx(npx),
            ComponentSource::Pipx(pipx) => resolve_pipx(pipx),
            ComponentSource::Cargo(cargo) => resolve_cargo(cargo).await,
            ComponentSource::Binary(binary_map) => resolve_binary(binary_map).await,
            ComponentSource::Http(dist) => Ok(resolve_http(dist)),
            ComponentSource::Sse(dist) => Ok(resolve_sse(dist)),
        }
    }
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

    let mut agents = vec![RegistryEntry {
        id: "elizacp".to_string(),
        name: "ElizACP".to_string(),
        version: String::new(),
        description: Some("Built-in Eliza agent for testing".to_string()),
        distribution: Distribution {
            local: Some(LocalDistribution {
                name: None,
                command: exe_str.clone(),
                args: vec!["eliza".to_string()],
                env: BTreeMap::new(),
            }),
            npx: None,
            pipx: None,
            binary: None,
            cargo: None,
        },
    }];

    // Include kiro-cli if available on PATH
    if which::which("kiro-cli").is_ok() {
        agents.push(RegistryEntry {
            id: "kiro-cli".to_string(),
            name: "Kiro CLI".to_string(),
            version: String::new(),
            description: Some("Kiro CLI agent".to_string()),
            distribution: Distribution {
                local: Some(LocalDistribution {
                    name: None,
                    command: "kiro-cli".to_string(),
                    args: vec!["acp".to_string()],
                    env: BTreeMap::new(),
                }),
                npx: None,
                pipx: None,
                binary: None,
                cargo: None,
            },
        });
    }

    Ok(agents)
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
    let agents_with_sources = list_agents_with_sources().await?;
    Ok(agents_with_sources
        .into_iter()
        .map(|(entry, _)| entry)
        .collect())
}

/// Agent selection entry - agent info paired with its ComponentSource for storage.
#[derive(Debug, Clone)]
pub struct AgentSelectionEntry {
    pub id: String,
    pub name: String,
    pub source: ComponentSource,
}

/// List all available agents with their ComponentSource for selection UI.
///
/// Returns pairs of (AgentListEntry, ComponentSource) so the UI can display
/// the agent info and store the appropriate ComponentSource when selected.
pub async fn list_agents_with_sources() -> Result<Vec<(AgentListEntry, ComponentSource)>> {
    let mut agents = Vec::new();

    // Built-in agents - use their specific distribution as the source
    for entry in built_in_agents()? {
        let source = entry_to_component_source(&entry);
        agents.push((
            AgentListEntry {
                id: entry.id,
                name: entry.name,
                version: if entry.version.is_empty() {
                    None
                } else {
                    Some(entry.version)
                },
                description: entry.description,
            },
            source,
        ));
    }

    // Fetch and merge registry agents
    let registry = fetch_registry().await?;
    for entry in registry.agents {
        // Skip if we already have this agent (built-in takes precedence)
        if agents.iter().any(|(a, _)| a.id == entry.id) {
            continue;
        }
        let source = entry_to_component_source(&entry);
        agents.push((
            AgentListEntry {
                id: entry.id,
                name: entry.name,
                version: if entry.version.is_empty() {
                    None
                } else {
                    Some(entry.version)
                },
                description: entry.description,
            },
            source,
        ));
    }

    Ok(agents)
}

/// Look up an agent by ID and return its ComponentSource.
///
/// This checks built-in agents first, then fetches the registry.
/// The agent ID is the same format used by `registry resolve-agent`.
pub async fn lookup_agent_source(agent_id: &str) -> Result<ComponentSource> {
    let agents = list_agents_with_sources().await?;
    agents
        .into_iter()
        .find(|(entry, _)| entry.id == agent_id)
        .map(|(_, source)| source)
        .with_context(|| format!("Agent '{}' not found", agent_id))
}

/// Convert a RegistryEntry to the appropriate ComponentSource.
///
/// Uses the entry's distribution to determine the most specific source type.
fn entry_to_component_source(entry: &RegistryEntry) -> ComponentSource {
    let dist = &entry.distribution;

    // Use the most specific source based on distribution type
    if let Some(local) = &dist.local {
        ComponentSource::Local(local.clone())
    } else if let Some(npx) = &dist.npx {
        ComponentSource::Npx(npx.clone())
    } else if let Some(pipx) = &dist.pipx {
        ComponentSource::Pipx(pipx.clone())
    } else if let Some(cargo) = &dist.cargo {
        ComponentSource::Cargo(cargo.clone())
    } else if let Some(binary) = &dist.binary {
        // Convert HashMap to BTreeMap for ComponentSource
        let btree: BTreeMap<String, BinaryDistribution> =
            binary.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        ComponentSource::Binary(btree)
    } else {
        // Fallback to registry ID if no distribution specified
        ComponentSource::Registry(entry.id.clone())
    }
}

/// Mod listing entry - what `registry list-mods` outputs
#[derive(Debug, Clone, Serialize)]
pub struct ModListEntry {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// List all available mods from the registry
pub async fn list_mods() -> Result<Vec<ModListEntry>> {
    let registry = fetch_registry().await?;

    let mods: Vec<ModListEntry> = registry
        .mods
        .into_iter()
        .map(|m| ModListEntry {
            id: m.id,
            name: m.name,
            version: if m.version.is_empty() {
                None
            } else {
                Some(m.version)
            },
            description: m.description,
        })
        .collect();

    Ok(mods)
}

// ============================================================================
// Crates.io API
// ============================================================================

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
        .user_agent("symposium-acp-agent (https://github.com/symposium-dev/symposium)")
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

// ============================================================================
// Cargo Installation
// ============================================================================

/// Install a crate using cargo binstall (fast) or cargo install (fallback)
async fn install_cargo_crate(
    crate_name: &str,
    version: &str,
    binary_name: &str,
    cache_dir: &PathBuf,
) -> Result<PathBuf> {
    let crate_name = crate_name.to_string();
    let version = version.to_string();
    let binary_name = binary_name.to_string();
    let cache_dir = cache_dir.clone();

    tokio::task::spawn_blocking(move || {
        install_cargo_crate_sync(&crate_name, &version, &binary_name, &cache_dir)
    })
    .await
    .context("Cargo install task panicked")?
}

/// Install a crate using cargo binstall or cargo install (blocking)
fn install_cargo_crate_sync(
    crate_name: &str,
    version: &str,
    binary_name: &str,
    cache_dir: &PathBuf,
) -> Result<PathBuf> {
    use std::fs;
    use std::process::Command;

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

    let crate_spec = format!("{}@{}", crate_name, version);

    // Try cargo binstall first (faster, uses prebuilt binaries)
    tracing::info!("Attempting cargo binstall for {}", crate_spec);
    let binstall_result = Command::new("cargo")
        .args([
            "binstall",
            "--no-confirm",
            "--root",
            cache_dir.to_str().unwrap(),
            &crate_spec,
        ])
        .output();

    let binary_path = cache_dir.join("bin").join(binary_name);

    match binstall_result {
        Ok(output) if output.status.success() => {
            tracing::info!("Successfully installed {} via cargo binstall", crate_spec);
            if binary_path.exists() {
                return Ok(binary_path);
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

    // Fall back to cargo install
    tracing::info!("Falling back to cargo install for {}", crate_spec);
    let install_result = Command::new("cargo")
        .args([
            "install",
            "--root",
            cache_dir.to_str().unwrap(),
            &crate_spec,
        ])
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

    if binary_path.exists() {
        Ok(binary_path)
    } else {
        bail!(
            "Binary '{}' not found after installing {}",
            binary_name,
            crate_spec
        )
    }
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

/// Get the cache directory for binary agents.
///
/// This uses the default ConfigPaths location (`~/.symposium/bin/<agent_id>/<version>`).
pub fn get_binary_cache_dir(agent_id: &str, version: &str) -> Result<PathBuf> {
    let config_paths = ConfigPaths::default_location()?;
    Ok(config_paths.binary_cache_dir(agent_id, version))
}

/// Resolve an agent JSON or ID to an McpServer configuration
pub async fn resolve_agent(agent: &str) -> Result<McpServer> {
    let agent_id = if agent.starts_with('{') {
        let entry: RegistryEntry = serde_json::from_str(agent)
            .map_err(|e| sacp::util::internal_error(format!("Failed to parse JSON: {}", e)))?;

        let resolved = resolve_distribution(&entry).await?;
        if let Some(agent) = resolved {
            return Ok(agent);
        }
        entry.id
    } else {
        agent.to_string()
    };
    // Check built-ins first
    for entry in built_in_agents()? {
        if entry.id == agent_id {
            let Some(agent) = resolve_distribution(&entry).await? else {
                bail!("Failed to resolve built-in agent: {}", entry.id);
            };
            return Ok(agent);
        }
    }

    // Fetch registry and find the agent
    let registry = fetch_registry().await?;
    let entry = registry
        .agents
        .into_iter()
        .find(|a| a.id == agent_id)
        .with_context(|| format!("Agent '{}' not found in registry", agent_id))?;

    if let Some(agent) = resolve_distribution(&entry).await? {
        return Ok(agent);
    }

    bail!(
        "No compatible distribution found for agent '{}' on platform {}",
        entry.id,
        get_platform_key()
    );
}

/// Resolve a mod JSON or ID to an McpServer configuration
pub async fn resolve_mod(mod_spec: &str) -> Result<McpServer> {
    let mod_id = if mod_spec.starts_with('{') {
        let entry: RegistryEntry = serde_json::from_str(mod_spec)
            .map_err(|e| sacp::util::internal_error(format!("Failed to parse JSON: {}", e)))?;

        let resolved = resolve_distribution(&entry).await?;
        if let Some(agent) = resolved {
            return Ok(agent);
        }
        entry.id
    } else {
        mod_spec.to_string()
    };
    // Fetch registry and find the mod
    let registry = fetch_registry().await?;
    let entry = registry
        .mods
        .into_iter()
        .find(|m| m.id == mod_id)
        .with_context(|| format!("Mod '{}' not found in registry", mod_id))?;

    if let Some(agent) = resolve_distribution(&entry).await? {
        return Ok(agent);
    }

    bail!(
        "No compatible distribution found for mod '{}' on platform {}",
        entry.id,
        get_platform_key()
    );
}

// ============================================================================
// ComponentSource Resolution Functions
// ============================================================================

/// Resolve a built-in component by name
/// **BE CAREFUL**: Tests should *not* use `ComponentSource::Builtin`, because
/// this finds the *current_exe`, which for tests is *not* `symposium-acp-agent`.
async fn resolve_builtin(name: &str) -> Result<McpServer> {
    let exe = current_exe()?;
    let exe_str = exe.to_string_lossy().to_string();

    // Check if it's a known built-in
    match name {
        "eliza" => Ok(McpServer::Stdio(
            McpServerStdio::new("ElizACP", &exe_str).args(vec!["eliza".to_string()]),
        )),
        _ => bail!(
            "Unknown built-in component: '{}'. Available builtins: eliza",
            name
        ),
    }
}

/// Resolve a component from the registry by ID
async fn resolve_from_registry(id: &str) -> Result<McpServer> {
    // Check built-in agents first
    for entry in built_in_agents()? {
        if entry.id == id {
            let Some(server) = resolve_distribution(&entry).await? else {
                bail!("Failed to resolve built-in agent: {}", id);
            };
            return Ok(server);
        }
    }

    // Fetch registry
    let registry = fetch_registry().await?;

    // Check agents
    if let Some(entry) = registry.agents.iter().find(|a| a.id == id) {
        if let Some(server) = resolve_distribution(entry).await? {
            return Ok(server);
        }
    }

    // Check mods
    if let Some(entry) = registry.mods.iter().find(|m| m.id == id) {
        if let Some(server) = resolve_distribution(entry).await? {
            return Ok(server);
        }
    }

    bail!("Component '{}' not found in registry", id)
}

/// Resolve from a URL to extension.json
async fn resolve_from_url(url: &str) -> Result<McpServer> {
    // Fetch extension.json from URL
    let response = reqwest::get(url)
        .await
        .with_context(|| format!("Failed to fetch extension from URL: {}", url))?;

    if !response.status().is_success() {
        bail!(
            "Failed to fetch extension from URL: {} {}",
            response.status().as_u16(),
            response.status().canonical_reason().unwrap_or("Unknown")
        );
    }

    let entry: RegistryEntry = response
        .json()
        .await
        .context("Failed to parse extension.json")?;

    resolve_distribution(&entry).await?.with_context(|| {
        format!(
            "No compatible distribution found for extension from {}",
            url
        )
    })
}

/// Resolve a local distribution
fn resolve_local(local: &LocalDistribution) -> Result<McpServer> {
    let env: Vec<EnvVariable> = local
        .env
        .iter()
        .map(|(k, v)| EnvVariable::new(k.clone(), v.clone()))
        .collect();

    let name = local.name.clone().unwrap_or_else(|| {
        Path::new(&local.command)
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| local.command.clone())
    });
    Ok(McpServer::Stdio(
        McpServerStdio::new(name, &local.command)
            .args(local.args.clone())
            .env(env),
    ))
}

/// Resolve an NPX distribution
fn resolve_npx(npx: &NpxDistribution) -> Result<McpServer> {
    let mut args = vec!["-y".to_string(), npx.package.clone()];
    args.extend(npx.args.clone());

    let env: Vec<EnvVariable> = npx
        .env
        .iter()
        .map(|(k, v)| EnvVariable::new(k.clone(), v.clone()))
        .collect();

    Ok(McpServer::Stdio(
        McpServerStdio::new(&npx.package, "npx").args(args).env(env),
    ))
}

/// Resolve a Pipx distribution
fn resolve_pipx(pipx: &PipxDistribution) -> Result<McpServer> {
    let mut args = vec!["run".to_string(), pipx.package.clone()];
    args.extend(pipx.args.clone());

    Ok(McpServer::Stdio(
        McpServerStdio::new(&pipx.package, "pipx").args(args),
    ))
}

/// Resolve a Cargo distribution
async fn resolve_cargo(cargo: &CargoDistribution) -> Result<McpServer> {
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

    let cache_dir = get_binary_cache_dir(&cargo.crate_name, &version)?;
    let binary_path = cache_dir.join("bin").join(&binary_name);

    // Check if we need to install
    if !binary_path.exists() {
        install_cargo_crate(&cargo.crate_name, &version, &binary_name, &cache_dir).await?;
    }

    Ok(McpServer::Stdio(
        McpServerStdio::new(&cargo.crate_name, &binary_path).args(cargo.args.clone()),
    ))
}

/// Resolve a binary distribution
async fn resolve_binary(binary_map: &BTreeMap<String, BinaryDistribution>) -> Result<McpServer> {
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

    let cache_dir = get_binary_cache_dir("binary", &version)?;
    let executable = binary.cmd.trim_start_matches("./");
    let executable_path = cache_dir.join(executable);

    // Check if we need to download
    if !executable_path.exists() {
        download_and_cache_binary("binary", &version, binary, &cache_dir).await?;
    }

    Ok(McpServer::Stdio(
        McpServerStdio::new(executable, executable_path).args(binary.args.clone()),
    ))
}

/// Resolve a registry entry's distribution to an McpServer
pub async fn resolve_distribution(entry: &RegistryEntry) -> Result<Option<McpServer>> {
    let dist = &entry.distribution;

    // Priority: local > npx > pipx > binary

    if let Some(local) = &dist.local {
        let env: Vec<EnvVariable> = local
            .env
            .iter()
            .map(|(k, v)| EnvVariable::new(k.clone(), v.clone()))
            .collect();

        return Ok(Some(McpServer::Stdio(
            McpServerStdio::new(&entry.name, &local.command)
                .args(local.args.clone())
                .env(env),
        )));
    }

    if let Some(npx) = &dist.npx {
        let mut args = vec!["-y".to_string(), npx.package.clone()];
        args.extend(npx.args.clone());

        let env: Vec<EnvVariable> = npx
            .env
            .iter()
            .map(|(k, v)| EnvVariable::new(k.clone(), v.clone()))
            .collect();

        return Ok(Some(McpServer::Stdio(
            McpServerStdio::new(&entry.name, "npx").args(args).env(env),
        )));
    }

    if let Some(pipx) = &dist.pipx {
        let mut args = vec!["run".to_string(), pipx.package.clone()];
        args.extend(pipx.args.clone());

        return Ok(Some(McpServer::Stdio(
            McpServerStdio::new(&entry.name, "pipx").args(args),
        )));
    }

    if let Some(cargo) = &dist.cargo {
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

        let cache_dir = get_binary_cache_dir(&entry.id, &version)?;
        let binary_path = cache_dir.join("bin").join(&binary_name);

        // Check if we need to install
        if !binary_path.exists() {
            install_cargo_crate(&cargo.crate_name, &version, &binary_name, &cache_dir).await?;
        }

        return Ok(Some(McpServer::Stdio(
            McpServerStdio::new(&entry.name, &binary_path).args(cargo.args.clone()),
        )));
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
                download_and_cache_binary(&entry.id, version, binary, &cache_dir).await?;
            }

            return Ok(Some(McpServer::Stdio(
                McpServerStdio::new(&entry.name, executable_path).args(binary.args.clone()),
            )));
        }
    }

    Ok(None)
}

fn resolve_http(dist: &HttpDistribution) -> McpServer {
    let headers = dist
        .headers
        .iter()
        .map(|h| sacp::schema::HttpHeader::new(h.name.clone(), h.value.clone()))
        .collect();
    McpServer::Http(McpServerHttp::new(dist.name.clone(), dist.url.clone()).headers(headers))
}

fn resolve_sse(dist: &HttpDistribution) -> McpServer {
    let headers = dist
        .headers
        .iter()
        .map(|h| sacp::schema::HttpHeader::new(h.name.clone(), h.value.clone()))
        .collect();
    McpServer::Sse(McpServerSse::new(dist.name.clone(), dist.url.clone()).headers(headers))
}

/// Download and cache a binary distribution
async fn download_and_cache_binary(
    agent_id: &str,
    version: &str,
    binary: &BinaryDistribution,
    cache_dir: &PathBuf,
) -> Result<()> {
    let agent_id = agent_id.to_string();
    let version = version.to_string();
    let binary = binary.clone();
    let cache_dir = cache_dir.clone();
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

        // kiro-cli is conditionally included based on PATH availability
        // so we just check if it's present, it has the right structure
        if let Some(kiro) = agents.iter().find(|a| a.id == "kiro-cli") {
            assert!(kiro.distribution.local.is_some());
            let local = kiro.distribution.local.as_ref().unwrap();
            assert_eq!(local.command, "kiro-cli");
            assert_eq!(local.args, vec!["acp"]);
        }
    }

    #[test]
    fn test_cargo_distribution_deserialize() {
        let json = r#"{
            "cargo": {
                "crate": "some-extension",
                "version": "0.1.0"
            }
        }"#;
        let dist: Distribution = serde_json::from_str(json).unwrap();
        assert!(dist.cargo.is_some());
        let cargo = dist.cargo.unwrap();
        assert_eq!(cargo.crate_name, "some-extension");
        assert_eq!(cargo.version, Some("0.1.0".to_string()));
        assert!(cargo.binary.is_none());
    }

    #[tokio::test]
    async fn test_query_crate_binaries() {
        // Test with a known crate that has a binary
        let (version, bin_names) = query_crate_binaries("ripgrep", Some("14.1.0"))
            .await
            .unwrap();
        assert_eq!(version, "14.1.0");
        assert!(bin_names.contains(&"rg".to_string()));
    }

    #[tokio::test]
    async fn test_query_crate_binaries_latest() {
        // Test fetching latest version
        let (version, bin_names) = query_crate_binaries("bat", None).await.unwrap();
        assert!(!version.is_empty());
        assert!(bin_names.contains(&"bat".to_string()));
    }

    #[tokio::test]
    async fn test_cargo_binstall_sparkle() {
        // Integration test: actually install sparkle-mcp via cargo binstall
        // Uses a temp directory so we don't pollute the real cache
        let temp_dir = tempfile::tempdir().unwrap();
        let cache_dir = temp_dir.path().to_path_buf();

        // Query crates.io to get the binary name and a known version
        let (version, bin_names) = query_crate_binaries("sparkle-mcp", None)
            .await
            .expect("Failed to query crates.io for sparkle-mcp");

        assert!(!version.is_empty(), "Version should not be empty");
        assert!(!bin_names.is_empty(), "Should have at least one binary");

        let binary_name = &bin_names[0];

        // Actually install via cargo binstall (or cargo install fallback)
        let binary_path = install_cargo_crate("sparkle-mcp", &version, binary_name, &cache_dir)
            .await
            .expect("Failed to install sparkle-mcp");

        // Verify the binary exists
        assert!(
            binary_path.exists(),
            "Binary should exist at {:?}",
            binary_path
        );

        // Verify it's executable (on Unix)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(&binary_path).unwrap();
            let mode = metadata.permissions().mode();
            assert!(mode & 0o111 != 0, "Binary should be executable");
        }
    }

    #[test]
    fn test_component_source_serialization() {
        // Test that ComponentSource serializes to expected JSON format
        let source = ComponentSource::Builtin("ferris".to_string());
        let json = serde_json::to_string(&source).unwrap();
        assert_eq!(json, r#"{"builtin":"ferris"}"#);

        let source = ComponentSource::Cargo(CargoDistribution {
            crate_name: "sparkle-mcp".to_string(),
            version: None,
            binary: None,
            args: vec!["--acp".to_string()],
        });
        let json = serde_json::to_string(&source).unwrap();
        assert!(json.contains(r#""cargo""#));
        assert!(json.contains(r#""crate":"sparkle-mcp""#));
    }

    #[test]
    fn test_component_source_display_name() {
        assert_eq!(
            ComponentSource::Builtin("ferris".to_string()).display_name(),
            "ferris"
        );
        assert_eq!(
            ComponentSource::Registry("claude-code".to_string()).display_name(),
            "claude-code"
        );
        assert_eq!(
            ComponentSource::Npx(NpxDistribution {
                package: "@zed-industries/claude-code-acp@latest".to_string(),
                args: vec![],
                env: BTreeMap::new(),
            })
            .display_name(),
            "claude-code-acp"
        );
        assert_eq!(
            ComponentSource::Cargo(CargoDistribution {
                crate_name: "sparkle-mcp".to_string(),
                version: None,
                binary: None,
                args: vec![],
            })
            .display_name(),
            "sparkle-mcp"
        );
    }

    #[test]
    fn test_component_source_ordering() {
        // ComponentSource should be orderable for use in BTreeMap
        let mut sources = vec![
            ComponentSource::Cargo(CargoDistribution {
                crate_name: "zebra".to_string(),
                version: None,
                binary: None,
                args: vec![],
            }),
            ComponentSource::Builtin("alpha".to_string()),
            ComponentSource::Registry("beta".to_string()),
        ];
        sources.sort();
        // Builtin comes before Cargo, Registry, etc. due to enum variant order
        assert!(matches!(sources[0], ComponentSource::Builtin(_)));
    }
}
