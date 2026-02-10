//! Component source types - how to obtain and run a component

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Component source - represents how to obtain and run a component.
///
/// This enum IS the identity for components in configuration. Two components
/// with the same `ComponentSource` are considered the same component.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ComponentSource {
    /// Built-in to symposium-acp-agent (i.e. "ferris" and"eliza")
    Builtin(String),

    /// From the ACP registry by ID
    Registry(String),

    /// From a URL to an extension.json
    Url(String),

    /// Local executable
    Local(LocalDistribution),

    /// NPX package
    Npx(NpxDistribution),

    /// Pipx package
    Pipx(PipxDistribution),

    /// Cargo crate
    Cargo(CargoDistribution),

    /// Platform-specific binary downloads
    Binary(BTreeMap<String, BinaryDistribution>),

    Http(HttpDistribution),

    Sse(HttpDistribution),
}

impl ComponentSource {
    /// Get a human-readable display name for this source
    pub fn display_name(&self) -> String {
        match self {
            ComponentSource::Builtin(name) => name.clone(),
            ComponentSource::Registry(id) => id.clone(),
            ComponentSource::Url(url) => {
                // Extract filename or last path segment
                url.rsplit('/').next().unwrap_or(url).to_string()
            }
            ComponentSource::Local(local) => {
                // If an explicit name is provided, use it. Otherwise use last component of command path
                if let Some(name) = &local.name {
                    name.clone()
                } else {
                    local
                        .command
                        .rsplit('/')
                        .next()
                        .unwrap_or(&local.command)
                        .to_string()
                }
            }
            ComponentSource::Npx(npx) => {
                // Extract package name without scope and version
                npx.package
                    .split('@')
                    .find(|s| !s.is_empty() && !s.starts_with('@'))
                    .unwrap_or(&npx.package)
                    .rsplit('/')
                    .next()
                    .unwrap_or(&npx.package)
                    .to_string()
            }
            ComponentSource::Pipx(pipx) => pipx.package.clone(),
            ComponentSource::Cargo(cargo) => cargo.crate_name.clone(),
            ComponentSource::Binary(_) => "binary".to_string(),
            ComponentSource::Http(dist) => dist.name.clone(),
            ComponentSource::Sse(dist) => dist.name.clone(),
        }
    }

    /// Check if this is a local source (not suitable for public recommendations)
    pub fn is_local(&self) -> bool {
        matches!(self, ComponentSource::Local(_))
    }
}

/// Local executable distribution
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize)]
pub struct LocalDistribution {
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

/// NPX package distribution
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize)]
pub struct NpxDistribution {
    pub package: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

/// Pipx package distribution
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize)]
pub struct PipxDistribution {
    pub package: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
}

/// Cargo crate distribution
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize)]
pub struct CargoDistribution {
    /// The crate name on crates.io
    #[serde(rename = "crate")]
    pub crate_name: String,
    /// Optional version (defaults to latest)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Optional explicit binary name (if not specified, queried from crates.io)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary: Option<String>,
    /// Additional args to pass to the binary
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
}

/// Binary distribution for a specific platform
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize)]
pub struct BinaryDistribution {
    pub archive: String,
    pub cmd: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
}

/// An HTTP header to set when making requests.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize)]
pub struct HttpHeader {
    /// The name of the HTTP header.
    pub name: String,
    /// The value to set for the HTTP header.
    pub value: String,
}

/// Available as an http server
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize)]
pub struct HttpDistribution {
    /// Human-readable name
    pub name: String,
    /// URL to the server/
    pub url: String,
    /// HTTP headers to set when making requests.
    pub headers: Vec<HttpHeader>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_name() {
        assert_eq!(
            ComponentSource::Builtin("ferris".to_string()).display_name(),
            "ferris"
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
        assert_eq!(
            ComponentSource::Npx(NpxDistribution {
                package: "@zed-industries/claude-code-acp@latest".to_string(),
                args: vec![],
                env: BTreeMap::new(),
            })
            .display_name(),
            "claude-code-acp"
        );
    }

    #[test]
    fn test_is_local() {
        assert!(ComponentSource::Local(LocalDistribution {
            command: "/usr/bin/foo".to_string(),
            args: vec![],
            name: None,
            env: BTreeMap::new(),
        })
        .is_local());

        assert!(!ComponentSource::Cargo(CargoDistribution {
            crate_name: "example".to_string(),
            version: None,
            binary: None,
            args: vec![],
        })
        .is_local());
    }

    #[test]
    fn test_serialization_roundtrip() {
        let source = ComponentSource::Cargo(CargoDistribution {
            crate_name: "sparkle-mcp".to_string(),
            version: Some("0.5.0".to_string()),
            binary: None,
            args: vec!["--acp".to_string()],
        });

        let json = serde_json::to_string(&source).unwrap();
        let parsed: ComponentSource = serde_json::from_str(&json).unwrap();
        assert_eq!(source, parsed);
    }
}
