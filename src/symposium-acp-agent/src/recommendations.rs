//! Recommendations - what components to suggest for a workspace
//!
//! This module handles recommending extensions based on workspace
//! characteristics. Recommendations are loaded from a built-in TOML file that
//! is embedded in the binary.

use crate::registry::ComponentSource;
use crate::user_config::{ExtensionConfig, WorkspaceExtensionsConfig};
use anyhow::{Context, Result};
use cargo_metadata::{Metadata, MetadataCommand, Package, PackageId};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

/// Built-in recommendations TOML, embedded at compile time
const BUILTIN_RECOMMENDATIONS_TOML: &str = include_str!("builtin_recommendations.toml");

/// A recommendation for a component
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recommendation {
    /// The source of the component (this IS the identity)
    pub source: ComponentSource,

    /// Conditions that must be met for this recommendation to apply
    #[serde(default)]
    pub when: Option<When>,
}

impl Recommendation {
    /// Get the display name for this recommendation
    pub fn display_name(&self) -> String {
        self.source.display_name()
    }

    /// Explain why this recommendation should be added (for new recommendations)
    pub fn explain_why_added(&self) -> Vec<String> {
        self.when
            .as_ref()
            .map(|w| w.explain_why_added())
            .unwrap_or_default()
    }

    /// Explain why this recommendation is stale (for removed recommendations)
    pub fn explain_why_stale(&self) -> Vec<String> {
        self.when
            .as_ref()
            .map(|w| w.explain_why_stale())
            .unwrap_or_default()
    }

    /// Format explanation for display (joins all reasons)
    pub fn format_added_explanation(&self) -> String {
        let reasons = self.explain_why_added();
        if reasons.is_empty() {
            String::new()
        } else {
            format!("[{}]", reasons.join(", "))
        }
    }

    /// Format stale explanation for display (joins all reasons)
    pub fn format_stale_explanation(&self) -> String {
        let reasons = self.explain_why_stale();
        if reasons.is_empty() {
            String::new()
        } else {
            format!("[{}]", reasons.join(", "))
        }
    }
}

/// Conditions for when a recommendation applies
///
/// Multiple fields at the same level are combined with AND.
/// Use `any` for OR logic, `all` for explicit AND grouping.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct When {
    /// Single file must exist in workspace root
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_exists: Option<String>,

    /// All files must exist in workspace root (AND)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files_exist: Option<Vec<String>>,

    /// Single crate must be a dependency
    #[serde(skip_serializing_if = "Option::is_none")]
    pub using_crate: Option<String>,

    /// All crates must be dependencies (AND)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub using_crates: Option<Vec<String>>,

    /// Any of these conditions must match (OR)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub any: Option<Vec<When>>,

    /// All of these conditions must match (explicit AND)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all: Option<Vec<When>>,
}

/// Check if a crate is a direct dependency of the workspace.
///
/// Battery packs (crates ending in `-battery-pack`) are "transparent" - we also
/// check their dependencies recursively. This means if your workspace depends on
/// `cli-battery-pack` which depends on `clap`, then `using-crate = "clap"` will match.
fn is_using_crate(workspace_path: &Path, crate_name: &str) -> bool {
    let metadata = match MetadataCommand::new()
        .current_dir(workspace_path)
        .no_deps() // We only need workspace members initially
        .exec()
    {
        Ok(m) => m,
        Err(_) => return false,
    };

    // We need the full metadata to resolve battery pack dependencies
    let full_metadata = match MetadataCommand::new().current_dir(workspace_path).exec() {
        Ok(m) => m,
        Err(_) => return false,
    };

    let mut visited = HashSet::new();

    // Check direct dependencies of all workspace members
    for member_id in &metadata.workspace_members {
        if let Some(package) = full_metadata.packages.iter().find(|p| &p.id == member_id) {
            if has_dependency_recursive(&full_metadata, package, crate_name, &mut visited) {
                return true;
            }
        }
    }

    false
}

/// Recursively check if a package has a dependency on the given crate.
/// Battery packs are transparent - we recurse into their dependencies.
fn has_dependency_recursive(
    metadata: &Metadata,
    package: &Package,
    crate_name: &str,
    visited: &mut HashSet<PackageId>,
) -> bool {
    for dep in &package.dependencies {
        // Check if this dependency matches
        if dep.name == crate_name {
            return true;
        }

        // If it's a battery pack, recurse into its dependencies
        if dep.name.ends_with("-battery-pack") {
            // Find the resolved package for this dependency
            if let Some(dep_package) = metadata.packages.iter().find(|p| p.name == dep.name) {
                if visited.insert(dep_package.id.clone()) {
                    if has_dependency_recursive(metadata, dep_package, crate_name, visited) {
                        return true;
                    }
                }
            }
        }
    }

    false
}

impl When {
    /// Check if this condition is met for the given workspace.
    /// All specified conditions must be true (AND semantics).
    /// If no conditions are specified, returns true.
    pub fn is_met(&self, workspace_path: &Path) -> bool {
        // file-exists
        if let Some(path) = &self.file_exists {
            if !workspace_path.join(path).exists() {
                return false;
            }
        }

        // files-exist (all must exist)
        if let Some(paths) = &self.files_exist {
            for path in paths {
                if !workspace_path.join(path).exists() {
                    return false;
                }
            }
        }

        // using-crate
        if let Some(crate_name) = &self.using_crate {
            if !is_using_crate(workspace_path, crate_name) {
                return false;
            }
        }

        // using-crates (all must be dependencies)
        if let Some(crate_names) = &self.using_crates {
            for crate_name in crate_names {
                if !is_using_crate(workspace_path, crate_name) {
                    return false;
                }
            }
        }

        // any (OR - at least one must match)
        if let Some(conditions) = &self.any {
            if !conditions.iter().any(|c| c.is_met(workspace_path)) {
                return false;
            }
        }

        // all (explicit AND - all must match)
        if let Some(conditions) = &self.all {
            if !conditions.iter().all(|c| c.is_met(workspace_path)) {
                return false;
            }
        }

        true
    }

    /// Explain why this condition causes a recommendation to be added
    pub fn explain_why_added(&self) -> Vec<String> {
        let mut reasons = Vec::new();

        if let Some(path) = &self.file_exists {
            reasons.push(format!("because `{path}` exists"));
        }

        if let Some(paths) = &self.files_exist {
            for path in paths {
                reasons.push(format!("because `{path}` exists"));
            }
        }

        if let Some(crate_name) = &self.using_crate {
            reasons.push(format!("because using crate `{crate_name}`"));
        }

        if let Some(crate_names) = &self.using_crates {
            for name in crate_names {
                reasons.push(format!("because using crate `{name}`"));
            }
        }

        if let Some(conditions) = &self.any {
            // For 'any', just list one that matches
            for c in conditions {
                let sub_reasons = c.explain_why_added();
                if !sub_reasons.is_empty() {
                    reasons.extend(sub_reasons);
                    break; // Only need to explain one matching condition
                }
            }
        }

        if let Some(conditions) = &self.all {
            for c in conditions {
                reasons.extend(c.explain_why_added());
            }
        }

        reasons
    }

    /// Explain why this condition causes a recommendation to be stale
    pub fn explain_why_stale(&self) -> Vec<String> {
        let mut reasons = Vec::new();

        if let Some(path) = &self.file_exists {
            reasons.push(format!("because `{path}` no longer exists"));
        }

        if let Some(paths) = &self.files_exist {
            for path in paths {
                reasons.push(format!("because `{path}` no longer exists"));
            }
        }

        if let Some(crate_name) = &self.using_crate {
            reasons.push(format!("because no longer using crate `{crate_name}`"));
        }

        if let Some(crate_names) = &self.using_crates {
            for name in crate_names {
                reasons.push(format!("because no longer using crate `{name}`"));
            }
        }

        if let Some(conditions) = &self.any {
            // For 'any', all must fail for it to be stale
            for c in conditions {
                reasons.extend(c.explain_why_stale());
            }
        }

        if let Some(conditions) = &self.all {
            // For 'all', any one failing makes it stale
            for c in conditions {
                let sub_reasons = c.explain_why_stale();
                if !sub_reasons.is_empty() {
                    reasons.extend(sub_reasons);
                    break;
                }
            }
        }

        reasons
    }
}

/// The recommendations file format
#[derive(Debug, Clone, Deserialize)]
struct RecommendationsFile {
    /// Recommendations list
    #[serde(rename = "recommendation")]
    recommendations: Vec<Recommendation>,
}

/// Loaded recommendations
#[derive(Debug, Clone)]
pub struct Recommendations {
    /// All extension recommendations
    pub extensions: Vec<Recommendation>,
}

impl Recommendations {
    /// Create empty recommendations (for testing)
    pub fn empty() -> Self {
        Self { extensions: vec![] }
    }

    /// Load the built-in recommendations
    pub fn load_builtin() -> Result<Self> {
        Self::from_toml(BUILTIN_RECOMMENDATIONS_TOML)
    }

    /// Parse recommendations from TOML string
    pub fn from_toml(toml_str: &str) -> Result<Self> {
        let file: RecommendationsFile =
            toml::from_str(toml_str).context("Failed to parse recommendations TOML")?;

        Ok(Self {
            extensions: file.recommendations,
        })
    }

    /// Get recommendations that apply to a specific workspace
    pub fn for_workspace(&self, workspace_path: &Path) -> WorkspaceRecommendations {
        let extensions: Vec<Recommendation> = self
            .extensions
            .iter()
            .filter(|r| {
                r.when
                    .as_ref()
                    .map(|w| w.is_met(workspace_path))
                    .unwrap_or(true)
            })
            .cloned()
            .collect();

        WorkspaceRecommendations { extensions }
    }
}

/// Recommendations filtered for a specific workspace
#[derive(Debug, Clone, Default)]
pub struct WorkspaceRecommendations {
    pub extensions: Vec<Recommendation>,
}

impl WorkspaceRecommendations {
    /// Get all extension sources as a set (for diffing with config)
    pub fn extension_sources(&self) -> Vec<ComponentSource> {
        self.extensions.iter().map(|r| r.source.clone()).collect()
    }

    /// Get a recommendation by its source
    pub fn get_recommendation(&self, source: &ComponentSource) -> Option<&Recommendation> {
        self.extensions.iter().find(|r| &r.source == source)
    }

    /// Compare these recommendations against the workspace extensions config.
    ///
    /// If the config already matches the recommendations, returns None.
    ///
    /// Otherwise, returns the diff showing what to add and remove.
    pub fn diff_against(&self, config: &WorkspaceExtensionsConfig) -> Option<RecommendationDiff> {
        // Get the set of recommended sources
        let recommended_sources: HashSet<_> =
            self.extensions.iter().map(|r| r.source.clone()).collect();

        // Get the set of configured sources
        let configured_sources: HashSet<_> =
            config.extensions.iter().map(|e| e.source.clone()).collect();

        // New = recommended but not configured
        let mut to_add = Vec::new();
        for extension in &self.extensions {
            if !configured_sources.contains(&extension.source) {
                // New recommendation - add it enabled
                to_add.push(ExtensionConfig {
                    source: extension.source.clone(),
                    enabled: true,
                    when: extension.when.clone().unwrap_or_default(),
                });
            }
        }

        let mut to_remove = vec![];
        for extension in &config.extensions {
            if !recommended_sources.contains(&extension.source) {
                // Stale - remove it
                to_remove.push(extension.clone());
            }
        }

        if !to_add.is_empty() || !to_remove.is_empty() {
            Some(RecommendationDiff { to_add, to_remove })
        } else {
            None
        }
    }
}

/// A new recommendation that isn't in the user's config yet
#[derive(Debug, Clone, Default)]
pub struct RecommendationDiff {
    /// Sources for extension that were newly recommended
    pub to_add: Vec<ExtensionConfig>,

    /// Configuration for extensions that were removed
    pub to_remove: Vec<ExtensionConfig>,
}

impl RecommendationDiff {
    /// True if this diff has no changes
    pub fn is_empty(&self) -> bool {
        self.to_add.is_empty() && self.to_remove.is_empty()
    }

    /// Apply this diff to the given workspace extensions config
    pub fn apply(&self, config: &mut WorkspaceExtensionsConfig) {
        if self.is_empty() {
            return;
        }

        // Add new recommendations
        for ext in &self.to_add {
            config.extensions.push(ext.clone());
        }

        // Remove stale recommendations
        config
            .extensions
            .retain(|ext| !self.to_remove.iter().any(|r| r.source == ext.source));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use expect_test::expect;
    use serial_test::serial;
    use std::io::Write;

    /// Write content to a file and sync to disk to avoid race conditions with cargo metadata
    fn write_synced(path: &Path, content: &str) {
        let mut file = std::fs::File::create(path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file.sync_all().unwrap();
    }

    #[test]
    fn test_load_builtin_recommendations() {
        let recs = Recommendations::load_builtin().expect("Should load builtin recommendations");

        // Should have some extension recommendations
        assert!(
            !recs.extensions.is_empty(),
            "Should have extension recommendations"
        );

        // Should have sparkle (always recommended) - it's a cargo source
        assert!(
            recs.extensions.iter().any(|r| matches!(
                &r.source,
                ComponentSource::Cargo(dist) if dist.crate_name == "sparkle-mcp"
            )),
            "Should have sparkle recommendation"
        );
    }

    #[test]
    fn test_workspace_filtering() {
        let toml = r#"
[[recommendation]]
source.builtin = "always-on"

[[recommendation]]
source.builtin = "rust-only"
when.file-exists = "Cargo.toml"
"#;

        let recs = Recommendations::from_toml(toml).unwrap();

        // Create a temp directory without Cargo.toml
        let temp_dir = tempfile::tempdir().unwrap();
        let workspace_recs = recs.for_workspace(temp_dir.path());

        // Should only have the "always-on" extension
        assert_eq!(workspace_recs.extensions.len(), 1);
        assert_eq!(workspace_recs.extensions[0].display_name(), "always-on");

        // Now create Cargo.toml
        std::fs::write(temp_dir.path().join("Cargo.toml"), "[package]").unwrap();
        let workspace_recs = recs.for_workspace(temp_dir.path());

        // Should have both extensions
        assert_eq!(workspace_recs.extensions.len(), 2);
    }

    #[test]
    fn test_when_any_condition() {
        let toml = r#"
[[recommendation]]
source.builtin = "multi-lang"
when.any = [
    { file-exists = "Cargo.toml" },
    { file-exists = "package.json" },
]
"#;

        let recs = Recommendations::from_toml(toml).unwrap();
        let temp_dir = tempfile::tempdir().unwrap();

        // No matching files
        let workspace_recs = recs.for_workspace(temp_dir.path());
        assert_eq!(workspace_recs.extensions.len(), 0);

        // Create Cargo.toml
        std::fs::write(temp_dir.path().join("Cargo.toml"), "[package]").unwrap();
        let workspace_recs = recs.for_workspace(temp_dir.path());
        assert_eq!(workspace_recs.extensions.len(), 1);

        // Remove Cargo.toml, create package.json
        std::fs::remove_file(temp_dir.path().join("Cargo.toml")).unwrap();
        std::fs::write(temp_dir.path().join("package.json"), "{}").unwrap();
        let workspace_recs = recs.for_workspace(temp_dir.path());
        assert_eq!(workspace_recs.extensions.len(), 1);
    }

    #[test]
    fn test_when_multiple_conditions_and() {
        let toml = r#"
[[recommendation]]
source.builtin = "both-required"
when.file-exists = "Cargo.toml"
when.files-exist = ["src/lib.rs"]
"#;

        let recs = Recommendations::from_toml(toml).unwrap();
        let temp_dir = tempfile::tempdir().unwrap();

        // Neither file
        let workspace_recs = recs.for_workspace(temp_dir.path());
        assert_eq!(workspace_recs.extensions.len(), 0);

        // Only Cargo.toml
        std::fs::write(temp_dir.path().join("Cargo.toml"), "[package]").unwrap();
        let workspace_recs = recs.for_workspace(temp_dir.path());
        assert_eq!(workspace_recs.extensions.len(), 0);

        // Both files
        std::fs::create_dir_all(temp_dir.path().join("src")).unwrap();
        std::fs::write(temp_dir.path().join("src/lib.rs"), "").unwrap();
        let workspace_recs = recs.for_workspace(temp_dir.path());
        assert_eq!(workspace_recs.extensions.len(), 1);
    }

    // ========================================================================
    // Diff tests
    // ========================================================================

    fn make_workspace_recs(extensions: Vec<(&str, Option<When>)>) -> WorkspaceRecommendations {
        WorkspaceRecommendations {
            extensions: extensions
                .into_iter()
                .map(|(name, when)| Recommendation {
                    source: ComponentSource::Builtin(name.to_string()),
                    when,
                })
                .collect(),
        }
    }

    #[test]
    fn test_diff_new_recommendations() {
        let recs = make_workspace_recs(vec![
            (
                "foo",
                Some(When {
                    file_exists: Some("Cargo.toml".to_string()),
                    ..Default::default()
                }),
            ),
            ("bar", None),
        ]);
        let config = WorkspaceExtensionsConfig::new(vec![]); // Empty config

        let diff = recs.diff_against(&config).expect("should have changes");

        expect![[r#"
            RecommendationDiff {
                to_add: [
                    ExtensionConfig {
                        source: Builtin(
                            "foo",
                        ),
                        enabled: true,
                        when: When {
                            file_exists: Some(
                                "Cargo.toml",
                            ),
                            files_exist: None,
                            using_crate: None,
                            using_crates: None,
                            any: None,
                            all: None,
                        },
                    },
                    ExtensionConfig {
                        source: Builtin(
                            "bar",
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
                to_remove: [],
            }
        "#]]
        .assert_debug_eq(&diff);
    }

    #[test]
    fn test_diff_stale_extensions() {
        let recs = make_workspace_recs(vec![]); // No recommendations
        let mut config = WorkspaceExtensionsConfig::new(vec![]);

        // Add an extension that's not recommended
        config.extensions.push(ExtensionConfig {
            source: ComponentSource::Builtin("old-ext".to_string()),
            enabled: true,
            when: When {
                file_exists: Some("old.txt".to_string()),
                ..Default::default()
            },
        });

        let diff = recs.diff_against(&config).expect("should have changes");

        expect![[r#"
            RecommendationDiff {
                to_add: [],
                to_remove: [
                    ExtensionConfig {
                        source: Builtin(
                            "old-ext",
                        ),
                        enabled: true,
                        when: When {
                            file_exists: Some(
                                "old.txt",
                            ),
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
        .assert_debug_eq(&diff);
    }

    #[test]
    fn test_diff_no_changes_when_in_sync() {
        let recs = make_workspace_recs(vec![("foo", None)]);
        let mut config = WorkspaceExtensionsConfig::new(vec![]);

        // Add the same extension that's recommended
        config.extensions.push(ExtensionConfig {
            source: ComponentSource::Builtin("foo".to_string()),
            enabled: true,
            when: When::default(),
        });

        let diff = recs.diff_against(&config);
        assert!(diff.is_none(), "No changes expected when in sync");
    }

    #[test]
    fn test_diff_disabled_extension_not_new() {
        // If an extension is in config but disabled, it's still "known" - not new
        let recs = make_workspace_recs(vec![("foo", None)]);
        let mut config = WorkspaceExtensionsConfig::new(vec![]);
        config.extensions.push(ExtensionConfig {
            source: ComponentSource::Builtin("foo".to_string()),
            enabled: false, // Disabled
            when: When::default(),
        });

        let diff = recs.diff_against(&config);
        // foo is not new because it's already in config (even though disabled)
        assert!(diff.is_none(), "Disabled extension should not count as new");
    }

    #[test]
    fn test_diff_apply() {
        let recs = make_workspace_recs(vec![("foo", None), ("bar", None)]);
        let mut config = WorkspaceExtensionsConfig::new(vec![]);

        // Add a stale extension
        config.extensions.push(ExtensionConfig {
            source: ComponentSource::Builtin("old".to_string()),
            enabled: true,
            when: When::default(),
        });

        let diff = recs.diff_against(&config).expect("should have changes");
        diff.apply(&mut config);

        // foo and bar should be added and enabled
        let foo_source = ComponentSource::Builtin("foo".to_string());
        let foo_ext = config.extensions.iter().find(|e| e.source == foo_source);
        assert!(foo_ext.is_some() && foo_ext.unwrap().enabled);

        let bar_source = ComponentSource::Builtin("bar".to_string());
        let bar_ext = config.extensions.iter().find(|e| e.source == bar_source);
        assert!(bar_ext.is_some() && bar_ext.unwrap().enabled);

        // old should be removed
        let old_source = ComponentSource::Builtin("old".to_string());
        assert!(!config.extensions.iter().any(|e| e.source == old_source));
    }

    #[test]
    fn test_when_explanations() {
        let when = When {
            file_exists: Some("Cargo.toml".to_string()),
            ..Default::default()
        };
        let added = when.explain_why_added();
        assert_eq!(added, vec!["because `Cargo.toml` exists"]);

        let stale = when.explain_why_stale();
        assert_eq!(stale, vec!["because `Cargo.toml` no longer exists"]);
    }

    #[test]
    #[serial]
    #[ignore = "https://github.com/symposium-dev/symposium/issues/112"]
    fn test_using_crate_condition() {
        let temp_dir = tempfile::tempdir().unwrap();

        // Create a minimal Cargo project (use write_synced to avoid race with cargo metadata)
        write_synced(
            &temp_dir.path().join("Cargo.toml"),
            r#"
[package]
name = "test-project"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = "1"
"#,
        );

        std::fs::create_dir(temp_dir.path().join("src")).unwrap();
        write_synced(&temp_dir.path().join("src/lib.rs"), "");

        // Test using-crate condition
        let when = When {
            using_crate: Some("serde".to_string()),
            ..Default::default()
        };
        assert!(when.is_met(temp_dir.path()));

        // Test crate that's not a dependency
        let when = When {
            using_crate: Some("tokio".to_string()),
            ..Default::default()
        };
        assert!(!when.is_met(temp_dir.path()));
    }

    #[test]
    #[serial]
    #[ignore = "https://github.com/symposium-dev/symposium/issues/112"]
    fn test_using_crates_condition() {
        let temp_dir = tempfile::tempdir().unwrap();

        // Create a minimal Cargo project with multiple deps (use write_synced to avoid race with cargo metadata)
        write_synced(
            &temp_dir.path().join("Cargo.toml"),
            r#"
[package]
name = "test-project"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = "1"
anyhow = "1"
"#,
        );

        std::fs::create_dir(temp_dir.path().join("src")).unwrap();
        write_synced(&temp_dir.path().join("src/lib.rs"), "");

        // Both crates are dependencies
        let when = When {
            using_crates: Some(vec!["serde".to_string(), "anyhow".to_string()]),
            ..Default::default()
        };
        assert!(when.is_met(temp_dir.path()));

        // One crate is missing
        let when = When {
            using_crates: Some(vec!["serde".to_string(), "tokio".to_string()]),
            ..Default::default()
        };
        assert!(!when.is_met(temp_dir.path()));
    }

    #[test]
    #[serial]
    #[ignore = "https://github.com/symposium-dev/symposium/issues/112"]
    fn test_using_crate_in_recommendation() {
        let toml = r#"
[[recommendation]]
source.builtin = "serde-helper"
when.using-crate = "serde"
"#;

        let recs = Recommendations::from_toml(toml).unwrap();
        let temp_dir = tempfile::tempdir().unwrap();

        // Create a project without serde (use write_synced to avoid race with cargo metadata)
        write_synced(
            &temp_dir.path().join("Cargo.toml"),
            r#"
[package]
name = "test-project"
version = "0.1.0"
edition = "2021"
"#,
        );
        std::fs::create_dir(temp_dir.path().join("src")).unwrap();
        write_synced(&temp_dir.path().join("src/lib.rs"), "");

        let workspace_recs = recs.for_workspace(temp_dir.path());
        assert_eq!(workspace_recs.extensions.len(), 0);

        // Add serde dependency
        write_synced(
            &temp_dir.path().join("Cargo.toml"),
            r#"
[package]
name = "test-project"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = "1"
"#,
        );

        let workspace_recs = recs.for_workspace(temp_dir.path());
        assert_eq!(workspace_recs.extensions.len(), 1);
        assert_eq!(workspace_recs.extensions[0].display_name(), "serde-helper");
    }
}
