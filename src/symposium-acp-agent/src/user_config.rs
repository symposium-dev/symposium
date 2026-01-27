//! User configuration types for Symposium.
//!
//! Configuration is stored per-workspace at:
//! `~/.symposium/config/<encoded-workspace-path>/config.json`
//!
//! The configuration uses `ComponentSource` as the identity for both
//! agents and extensions, enabling easy diffing with recommendations.

use crate::recommendations::When;
use crate::registry::ComponentSource;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Extension configuration entry
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ExtensionConfig {
    /// The source of this extension
    pub source: ComponentSource,

    /// Whether this extension is enabled
    pub enabled: bool,

    /// The conditions that caused this extension to be recommended.
    /// Used to explain why an extension is stale when the conditions no longer apply.
    pub when: When,
}

/// Per-workspace configuration for Symposium.
///
/// Uses `ComponentSource` as identity for both agent and extensions.
/// This makes it easy to compare with recommendations and detect changes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct WorkspaceConfig {
    /// The agent to use for this workspace
    pub agent: ComponentSource,

    /// Extensions with their enabled state
    /// The key is the JSON-serialized ComponentSource
    #[serde(default)]
    pub extensions: Vec<ExtensionConfig>,
}

// ============================================================================
// Global Agent Config (for default agent across workspaces)
// ============================================================================

/// Global agent configuration.
///
/// Stores the user's default agent choice. This is used to populate the initial
/// agent for new workspaces. Each workspace can override this independently.
///
/// Stored at `~/.symposium/config/agent.json`
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct GlobalAgentConfig {
    /// The default agent to use for new workspaces
    pub agent: ComponentSource,
}

impl GlobalAgentConfig {
    /// Create a new global agent config
    pub fn new(agent: ComponentSource) -> Self {
        Self { agent }
    }

    /// Get the path to the global agent config file
    pub fn path() -> Result<PathBuf> {
        let home = dirs::home_dir().context("Could not determine home directory")?;
        Ok(home.join(".symposium").join("config").join("agent.json"))
    }

    /// Load the global agent config. Returns None if it doesn't exist.
    pub fn load() -> Result<Option<Self>> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read global agent config from {}", path.display()))?;
        let config: Self = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse global agent config from {}", path.display()))?;
        Ok(Some(config))
    }

    /// Save the global agent config
    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("Failed to create config directory {}", dir.display()))?;
        }
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, content)
            .with_context(|| format!("Failed to write global agent config to {}", path.display()))?;
        Ok(())
    }
}

// ============================================================================
// Workspace Config
// ============================================================================

impl WorkspaceConfig {
    /// Create a new workspace config with the given agent and extensions
    pub fn new(agent: ComponentSource, extensions: Vec<ComponentSource>) -> Self {
        let extensions = extensions
            .into_iter()
            .map(|source| ExtensionConfig {
                source,
                enabled: true,
                when: When::default(),
            })
            .collect();

        Self { agent, extensions }
    }

    /// Get the config directory for a workspace
    pub fn workspace_config_dir(workspace_path: &Path) -> Result<PathBuf> {
        let home = dirs::home_dir().context("Could not determine home directory")?;
        let encoded = encode_path(workspace_path);
        Ok(home.join(".symposium").join("config").join(encoded))
    }

    /// Get the config file path for a workspace
    pub fn config_path(workspace_path: &Path) -> Result<PathBuf> {
        Ok(Self::workspace_config_dir(workspace_path)?.join("config.json"))
    }

    /// Load config for a workspace. Returns None if config doesn't exist.
    pub fn load(workspace_path: &Path) -> Result<Option<Self>> {
        let path = Self::config_path(workspace_path)?;
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config from {}", path.display()))?;
        let config: Self = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse config from {}", path.display()))?;
        Ok(Some(config))
    }

    /// Save config for a workspace
    pub fn save(&self, workspace_path: &Path) -> Result<()> {
        let path = Self::config_path(workspace_path)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("Failed to create config directory {}", dir.display()))?;
        }
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, content)
            .with_context(|| format!("Failed to write config to {}", path.display()))?;
        Ok(())
    }

    /// Get enabled extension sources in order
    pub fn enabled_extensions(&self) -> Vec<ComponentSource> {
        self.extensions
            .iter()
            .filter(|extension| extension.enabled)
            .map(|extension| extension.source.clone())
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
    let last_component = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("root");

    // Hash the full path
    let mut hasher = Sha256::new();
    hasher.update(path_str.as_bytes());
    let hash = hasher.finalize();

    // Format first 8 bytes (16 hex chars) of hash
    let hash_hex: String = hash.iter().take(8).map(|b| format!("{:02x}", b)).collect();

    format!("{}-{}", last_component, hash_hex)
}

// ============================================================================
// Legacy types for backwards compatibility
// ============================================================================

/// Legacy user configuration for Symposium.
/// Used for migration from old config format.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct SymposiumUserConfig {
    /// Downstream agent command (shell words, e.g., "npx -y @anthropic-ai/claude-code-acp")
    pub agent: String,

    /// Proxy extensions to enable
    pub proxies: Vec<ProxyEntry>,
}

/// A proxy extension entry in the legacy configuration.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct ProxyEntry {
    /// Proxy name (e.g., "sparkle", "ferris", "cargo")
    pub name: String,

    /// Whether this proxy is enabled
    pub enabled: bool,
}

impl SymposiumUserConfig {
    /// Get the legacy config directory path: ~/.symposium/
    pub fn dir() -> Result<PathBuf> {
        let home = dirs::home_dir().context("Could not determine home directory")?;
        Ok(home.join(".symposium"))
    }

    /// Get the legacy config file path: ~/.symposium/config.jsonc
    pub fn path() -> Result<PathBuf> {
        Ok(Self::dir()?.join("config.jsonc"))
    }

    /// Load legacy config from the given path, or the default path if None.
    /// Returns None if the config file doesn't exist.
    pub fn load(path: Option<impl AsRef<std::path::Path>>) -> Result<Option<Self>> {
        let path = match path {
            Some(p) => p.as_ref().to_path_buf(),
            None => Self::path()?,
        };
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path)?;
        let config: Self = serde_jsonc::from_str(&content)?;
        Ok(Some(config))
    }

    /// Save config to the default path.
    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::path()?)
    }

    /// Save config to a specific path.
    pub fn save_to(&self, path: &PathBuf) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Get the list of enabled proxy names.
    pub fn enabled_proxies(&self) -> Vec<String> {
        self.proxies
            .iter()
            .filter(|p| p.enabled)
            .map(|p| p.name.clone())
            .collect()
    }

    /// Parse the agent string into command arguments (shell words).
    pub fn agent_args(&self) -> Result<Vec<String>> {
        shell_words::split(&self.agent)
            .map_err(|e| anyhow::anyhow!("Failed to parse agent command: {}", e))
    }

    /// Create a default config with all proxies enabled.
    pub fn with_agent(agent: impl Into<String>) -> Self {
        Self {
            agent: agent.into(),
            proxies: vec![
                ProxyEntry {
                    name: "sparkle".to_string(),
                    enabled: true,
                },
                ProxyEntry {
                    name: "ferris".to_string(),
                    enabled: true,
                },
                ProxyEntry {
                    name: "cargo".to_string(),
                    enabled: true,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{CargoDistribution, NpxDistribution};
    use expect_test::expect;
    use std::collections::BTreeMap;

    #[test]
    fn test_workspace_config_new() {
        let agent = ComponentSource::Npx(NpxDistribution {
            package: "@zed-industries/claude-code-acp@latest".to_string(),
            args: vec![],
            env: BTreeMap::new(),
        });
        let extensions = vec![
            ComponentSource::Builtin("ferris".to_string()),
            ComponentSource::Cargo(CargoDistribution {
                crate_name: "sparkle-mcp".to_string(),
                version: None,
                binary: None,
                args: vec!["--acp".to_string()],
            }),
        ];

        let config = WorkspaceConfig::new(agent, extensions);

        expect![[r#"
            WorkspaceConfig {
                agent: Npx(
                    NpxDistribution {
                        package: "@zed-industries/claude-code-acp@latest",
                        args: [],
                        env: {},
                    },
                ),
                extensions: [
                    ExtensionConfig {
                        source: Builtin(
                            "ferris",
                        ),
                        enabled: true,
                        when: When {
                            file_exists: None,
                            files_exist: None,
                            using_crate: None,
                            using_crates: None,
                            grep: None,
                            any: None,
                            all: None,
                        },
                    },
                    ExtensionConfig {
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
                            grep: None,
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
    fn test_workspace_config_save_load_roundtrip() {
        let temp_dir = tempfile::tempdir().unwrap();
        let workspace_path = temp_dir.path();

        let agent = ComponentSource::Builtin("eliza".to_string());
        let extensions = vec![ComponentSource::Builtin("ferris".to_string())];
        let config = WorkspaceConfig::new(agent.clone(), extensions);

        // Save
        config.save(workspace_path).unwrap();

        // Load
        let loaded = WorkspaceConfig::load(workspace_path).unwrap().unwrap();

        assert_eq!(config, loaded);
    }

    #[test]
    fn test_encode_path() {
        let path = PathBuf::from("/Users/test/my-project");
        let encoded = encode_path(&path);

        // Should be in format: last_component-truncated_sha256_hash
        assert!(encoded.starts_with("my-project-"), "Should start with last component");
        assert_eq!(encoded.len(), "my-project-".len() + 16, "Hash should be 16 hex chars");

        // Same path should produce same encoding
        let encoded2 = encode_path(&path);
        assert_eq!(encoded, encoded2);

        // Different path should produce different encoding
        let other_path = PathBuf::from("/Users/test/other-project");
        let other_encoded = encode_path(&other_path);
        assert_ne!(encoded, other_encoded);
    }


}
