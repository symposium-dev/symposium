//! User configuration types for Symposium.
//!
//! Configuration is split between:
//! - Global agent config: `~/.symposium/config/agent.json` - the selected agent for all workspaces
//! - Per-workspace mods: `~/.symposium/config/<encoded-workspace-path>/config.json`
//!
//! The configuration uses `ComponentSource` as the identity for mods,
//! enabling easy diffing with recommendations.

use crate::recommendations::When;
use crate::registry::ComponentSource;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ============================================================================
// ConfigPaths - the root configuration directory
// ============================================================================

/// Manages paths to Symposium configuration files and directories.
///
/// This struct provides paths to various configuration locations and ensures
/// directories exist when needed. By default, configuration is stored under
/// `~/.symposium/`. Tests can provide a custom root directory to avoid
/// modifying the user's home.
///
/// The struct only provides paths and directory creation - callers are
/// responsible for their own reads and writes.
#[derive(Debug, Clone)]
pub struct ConfigPaths {
    /// Root directory for all Symposium configuration (e.g., `~/.symposium`).
    root: PathBuf,
}

/// Environment variable to override the default config directory.
/// When set, this takes precedence over `~/.symposium`.
/// Useful for testing to avoid modifying the user's real configuration.
pub const SYMPOSIUM_CONFIG_DIR_ENV: &str = "SYMPOSIUM_CONFIG_DIR";

impl ConfigPaths {
    /// Create a ConfigPaths using the default location.
    ///
    /// Checks `SYMPOSIUM_CONFIG_DIR` environment variable first,
    /// falling back to `~/.symposium` if not set.
    pub fn default_location() -> Result<Self> {
        if let Ok(dir) = std::env::var(SYMPOSIUM_CONFIG_DIR_ENV) {
            return Ok(Self {
                root: PathBuf::from(dir),
            });
        }
        let home = dirs::home_dir().context("Could not determine home directory")?;
        Ok(Self {
            root: home.join(".symposium"),
        })
    }

    /// Create a ConfigPaths with a custom root directory.
    ///
    /// Useful for tests to isolate configuration from the user's home.
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Get the root directory (e.g., `~/.symposium`).
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Ensure a directory exists (like `mkdir -p`).
    fn ensure_dir(&self, path: &Path) -> Result<()> {
        std::fs::create_dir_all(path)
            .with_context(|| format!("Failed to create directory {}", path.display()))
    }

    // ------------------------------------------------------------------------
    // Global agent config
    // ------------------------------------------------------------------------

    /// Get the path to the global agent config file.
    ///
    /// Location: `<root>/config/agent.json`
    pub fn global_agent_config_path(&self) -> PathBuf {
        self.root.join("config").join("agent.json")
    }

    /// Ensure the global agent config directory exists and return the config path.
    ///
    /// Use this before writing to the global agent config file.
    pub fn ensure_global_agent_config_dir(&self) -> Result<PathBuf> {
        let path = self.global_agent_config_path();
        if let Some(dir) = path.parent() {
            self.ensure_dir(dir)?;
        }
        Ok(path)
    }

    // ------------------------------------------------------------------------
    // Workspace config
    // ------------------------------------------------------------------------

    /// Get the config directory for a workspace.
    ///
    /// Location: `<root>/config/<encoded-workspace-path>/`
    pub fn workspace_config_dir(&self, workspace_path: &Path) -> PathBuf {
        let encoded = encode_path(workspace_path);
        self.root.join("config").join(encoded)
    }

    /// Get the config file path for a workspace.
    ///
    /// Location: `<root>/config/<encoded-workspace-path>/config.json`
    pub fn workspace_config_path(&self, workspace_path: &Path) -> PathBuf {
        self.workspace_config_dir(workspace_path)
            .join("config.json")
    }

    /// Ensure the workspace config directory exists and return the config path.
    ///
    /// Use this before writing to the workspace config file.
    pub fn ensure_workspace_config_dir(&self, workspace_path: &Path) -> Result<PathBuf> {
        let path = self.workspace_config_path(workspace_path);
        if let Some(dir) = path.parent() {
            self.ensure_dir(dir)?;
        }
        Ok(path)
    }

    // ------------------------------------------------------------------------
    // Binary cache (for downloaded agents)
    // ------------------------------------------------------------------------

    /// Get the cache directory for a binary agent.
    ///
    /// Location: `<root>/bin/<agent_id>/<version>/`
    pub fn binary_cache_dir(&self, agent_id: &str, version: &str) -> PathBuf {
        self.root.join("bin").join(agent_id).join(version)
    }

    /// Ensure the binary cache directory exists and return the path.
    ///
    /// Use this before downloading agent binaries.
    pub fn ensure_binary_cache_dir(&self, agent_id: &str, version: &str) -> Result<PathBuf> {
        let path = self.binary_cache_dir(agent_id, version);
        self.ensure_dir(&path)?;
        Ok(path)
    }
}

/// Mod configuration entry
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ModConfig {
    /// The source of this mod
    pub source: ComponentSource,

    /// Whether this mod is enabled
    pub enabled: bool,

    /// The conditions that caused this mod to be recommended.
    /// Used to explain why a mod is stale when the conditions no longer apply.
    pub when: When,
}

/// Per-workspace mod configuration for Symposium.
///
/// Uses `ComponentSource` as identity for mods.
/// This makes it easy to compare with recommendations and detect changes.
///
/// Note: The agent is stored globally in `GlobalAgentConfig`, not per-workspace.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct WorkspaceModsConfig {
    /// Mods with their enabled state
    #[serde(default)]
    pub mods: Vec<ModConfig>,
}

// ============================================================================
// Global Agent Config (for default agent across workspaces)
// ============================================================================

/// Global agent configuration.
///
/// Stores the user's selected agent. This agent is used for all workspaces.
///
/// Stored at `~/.symposium/config/agent.json`
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct GlobalAgentConfig {
    /// The selected agent for all workspaces
    pub agent: ComponentSource,
}

impl GlobalAgentConfig {
    /// Create a new global agent config
    pub fn new(agent: ComponentSource) -> Self {
        Self { agent }
    }

    /// Load the global agent config.
    /// Returns None if the file doesn't exist.
    pub fn load(config_paths: &ConfigPaths) -> Result<Option<Self>> {
        let path = config_paths.global_agent_config_path();
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path).with_context(|| {
            format!("Failed to read global agent config from {}", path.display())
        })?;
        let config: Self = serde_json::from_str(&content).with_context(|| {
            format!(
                "Failed to parse global agent config from {}",
                path.display()
            )
        })?;
        Ok(Some(config))
    }

    /// Save the global agent config.
    /// Creates the parent directory if it doesn't exist.
    pub fn save(&self, config_paths: &ConfigPaths) -> Result<()> {
        let path = config_paths.ensure_global_agent_config_dir()?;
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, &content).with_context(|| {
            format!("Failed to write global agent config to {}", path.display())
        })?;
        Ok(())
    }
}

// ============================================================================
// Workspace Mods Config
// ============================================================================

impl WorkspaceModsConfig {
    /// Create a new workspace mods config
    pub fn new(mods: Vec<ModConfig>) -> Self {
        Self { mods }
    }

    /// Create a workspace mods config from a list of mod sources.
    /// All mods are enabled by default.
    pub fn from_sources(sources: Vec<ComponentSource>) -> Self {
        let mods = sources
            .into_iter()
            .map(|source| ModConfig {
                source,
                enabled: true,
                when: When::default(),
            })
            .collect();

        Self { mods }
    }

    /// Load the workspace mods config for the given workspace.
    /// Returns None if the file doesn't exist.
    ///
    /// Handles migration from old format that included an `agent` field.
    pub fn load(config_paths: &ConfigPaths, workspace_path: &Path) -> Result<Option<Self>> {
        let path = config_paths.workspace_config_path(workspace_path);
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read workspace config from {}", path.display()))?;

        // Try to parse - serde will ignore unknown fields like `agent` from old format
        let config: Self = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse workspace config from {}", path.display()))?;
        Ok(Some(config))
    }

    /// Save the workspace mods config for the given workspace.
    /// Creates the parent directory if it doesn't exist.
    pub fn save(&self, config_paths: &ConfigPaths, workspace_path: &Path) -> Result<()> {
        let path = config_paths.ensure_workspace_config_dir(workspace_path)?;
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, &content)
            .with_context(|| format!("Failed to write workspace config to {}", path.display()))?;
        Ok(())
    }

    /// Get enabled mod sources in order
    pub fn enabled_mods(&self) -> Vec<ComponentSource> {
        self.mods
            .iter()
            .filter(|m| m.enabled)
            .map(|m| m.source.clone())
            .collect()
    }
}

/// Encode a path for use as a directory name.
///
/// Format: `{last_component}-{truncated_sha256_hash}`
/// Example: `symposium-e3b0c44298fc1c14`
fn encode_path(path: &Path) -> String {
    use sha2::{Digest, Sha256};

    let path_str = path.to_string_lossy();

    // Get the last path component (or "root" for paths like "/")
    let last_component = path.file_name().and_then(|s| s.to_str()).unwrap_or("root");

    // Hash the full path
    let mut hasher = Sha256::new();
    hasher.update(path_str.as_bytes());
    let hash = hasher.finalize();

    // Format first 8 bytes (16 hex chars) of hash
    let hash_hex: String = hash.iter().take(8).map(|b| format!("{:02x}", b)).collect();

    format!("{}-{}", last_component, hash_hex)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::CargoDistribution;
    use expect_test::expect;

    #[test]
    fn test_workspace_mods_config_from_sources() {
        let sources = vec![
            ComponentSource::Builtin("ferris".to_string()),
            ComponentSource::Cargo(CargoDistribution {
                crate_name: "sparkle-mcp".to_string(),
                version: None,
                binary: None,
                args: vec!["--acp".to_string()],
            }),
        ];

        let config = WorkspaceModsConfig::from_sources(sources);

        expect![[r#"
            WorkspaceModsConfig {
                mods: [
                    ModConfig {
                        source: Builtin(
                            "ferris",
                        ),
                        enabled: true,
                        when: When {
                            file_exists: None,
                            files_exist: None,
                            using_crate: None,
                            using_crates: None,
                            any: None,
                            all: None,
                        },
                    },
                    ModConfig {
                        source: Cargo(
                            CargoDistribution {
                                crate_name: "sparkle-mcp",
                                version: None,
                                binary: None,
                                args: [
                                    "--acp",
                                ],
                            },
                        ),
                        enabled: true,
                        when: When {
                            file_exists: None,
                            files_exist: None,
                            using_crate: None,
                            using_crates: None,
                            any: None,
                            all: None,
                        },
                    },
                ],
            }
        "#]]
        .assert_debug_eq(&config);
    }

    #[test]
    fn test_workspace_mods_config_save_load_roundtrip() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_paths = ConfigPaths::with_root(temp_dir.path());
        let workspace_path = PathBuf::from("/some/workspace");

        let sources = vec![ComponentSource::Builtin("ferris".to_string())];
        let config = WorkspaceModsConfig::from_sources(sources);

        // Save
        config.save(&config_paths, &workspace_path).unwrap();

        // Load
        let loaded = WorkspaceModsConfig::load(&config_paths, &workspace_path)
            .unwrap()
            .unwrap();

        assert_eq!(config, loaded);
    }

    #[test]
    fn test_global_agent_config_save_load_roundtrip() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_paths = ConfigPaths::with_root(temp_dir.path());

        let config = GlobalAgentConfig::new(ComponentSource::Builtin("eliza".to_string()));

        // Save
        config.save(&config_paths).unwrap();

        // Load
        let loaded = GlobalAgentConfig::load(&config_paths).unwrap().unwrap();

        assert_eq!(config, loaded);
    }

    #[test]
    fn test_encode_path() {
        let path = PathBuf::from("/Users/test/my-project");
        let encoded = encode_path(&path);

        // Should be in format: last_component-truncated_sha256_hash
        assert!(
            encoded.starts_with("my-project-"),
            "Should start with last component"
        );
        assert_eq!(
            encoded.len(),
            "my-project-".len() + 16,
            "Hash should be 16 hex chars"
        );

        // Same path should produce same encoding
        let encoded2 = encode_path(&path);
        assert_eq!(encoded, encoded2);

        // Different path should produce different encoding
        let other_path = PathBuf::from("/Users/test/other-project");
        let other_encoded = encode_path(&other_path);
        assert_ne!(encoded, other_encoded);
    }

    #[test]
    fn test_global_agent_config_json_roundtrip() {
        // Test the JSON format used in CI setup
        let json = r#"{"agent":{"builtin":"eliza"}}"#;
        let config: GlobalAgentConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.agent, ComponentSource::Builtin("eliza".to_string()));

        // Verify serialization matches
        let serialized = serde_json::to_string(&config).unwrap();
        assert_eq!(serialized, json);
    }

    #[test]
    fn test_config_paths_env_override() {
        let temp_dir = tempfile::tempdir().unwrap();
        let custom_path = temp_dir.path().to_str().unwrap();

        // Set the environment variable
        std::env::set_var(SYMPOSIUM_CONFIG_DIR_ENV, custom_path);

        let config_paths = ConfigPaths::default_location().unwrap();
        assert_eq!(config_paths.root(), temp_dir.path());

        // Clean up
        std::env::remove_var(SYMPOSIUM_CONFIG_DIR_ENV);
    }
}
