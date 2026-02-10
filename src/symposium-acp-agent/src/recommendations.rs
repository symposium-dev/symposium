//! Recommendations - what components to suggest for a workspace
//!
//! This module handles recommending mods based on workspace
//! characteristics. Recommendations are loaded from a built-in TOML file that
//! is embedded in the binary.

use crate::user_config::{ModConfig, WorkspaceModsConfig};
use anyhow::Result;
use cargo_metadata::{Metadata, MetadataCommand, Package, PackageId};
use std::collections::HashSet;
use std::path::Path;

// Re-export types from symposium-recommendations
pub use symposium_recommendations::{Recommendation, Recommendations, When};

/// Built-in recommendations TOML, embedded at compile time
const BUILTIN_RECOMMENDATIONS_TOML: &str = include_str!("builtin_recommendations.toml");

// ============================================================================
// Extension traits for workspace evaluation
// ============================================================================

/// Extension trait for Recommendation with formatting helpers
pub trait RecommendationExt {
    /// Explain why this recommendation should be added (for new recommendations)
    fn explain_why_added(&self) -> Vec<String>;

    /// Explain why this recommendation is stale (for removed recommendations)
    fn explain_why_stale(&self) -> Vec<String>;

    /// Format explanation for display (joins all reasons)
    fn format_added_explanation(&self) -> String;

    /// Format stale explanation for display (joins all reasons)
    fn format_stale_explanation(&self) -> String;
}

impl RecommendationExt for Recommendation {
    fn explain_why_added(&self) -> Vec<String> {
        self.when
            .as_ref()
            .map(|w| w.explain_why_added())
            .unwrap_or_default()
    }

    fn explain_why_stale(&self) -> Vec<String> {
        self.when
            .as_ref()
            .map(|w| w.explain_why_stale())
            .unwrap_or_default()
    }

    fn format_added_explanation(&self) -> String {
        let reasons = self.explain_why_added();
        if reasons.is_empty() {
            String::new()
        } else {
            format!("[{}]", reasons.join(", "))
        }
    }

    fn format_stale_explanation(&self) -> String {
        let reasons = self.explain_why_stale();
        if reasons.is_empty() {
            String::new()
        } else {
            format!("[{}]", reasons.join(", "))
        }
    }
}

/// Extension trait for When to evaluate conditions against a workspace
pub trait WhenExt {
    /// Check if this condition is met for the given workspace.
    fn is_met(&self, workspace_path: &Path) -> bool;
}

impl WhenExt for When {
    fn is_met(&self, workspace_path: &Path) -> bool {
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

// ============================================================================
// Extension trait for Recommendations
// ============================================================================

/// Extension trait for Recommendations with workspace filtering
pub trait RecommendationsExt {
    /// Load the built-in recommendations
    fn load_builtin() -> Result<Recommendations>;

    /// Get recommendations that apply to a specific workspace
    fn for_workspace(&self, workspace_path: &Path) -> WorkspaceRecommendations;
}

impl RecommendationsExt for Recommendations {
    fn load_builtin() -> Result<Recommendations> {
        Recommendations::from_toml(BUILTIN_RECOMMENDATIONS_TOML)
    }

    fn for_workspace(&self, workspace_path: &Path) -> WorkspaceRecommendations {
        use crate::remote_recommendations::load_workspace_recommendations;

        // Filter global recommendations by workspace conditions
        let mut mods: Vec<Recommendation> = self
            .mods
            .iter()
            .filter(|r| {
                r.when
                    .as_ref()
                    .map(|w| w.is_met(workspace_path))
                    .unwrap_or(true)
            })
            .cloned()
            .collect();

        // Merge workspace-specific recommendations if present
        match load_workspace_recommendations(workspace_path) {
            Ok(Some(workspace_recs)) => {
                // Filter workspace recommendations by their conditions too
                for rec in workspace_recs.mods {
                    let meets_condition = rec
                        .when
                        .as_ref()
                        .map(|w| w.is_met(workspace_path))
                        .unwrap_or(true);
                    if meets_condition {
                        mods.push(rec);
                    }
                }
            }
            Ok(None) => {
                // No workspace recommendations file - that's fine
            }
            Err(e) => {
                tracing::warn!("Failed to load workspace recommendations: {}", e);
            }
        }

        WorkspaceRecommendations { mods }
    }
}

// ============================================================================
// Workspace-specific recommendations
// ============================================================================

/// Recommendations filtered for a specific workspace
#[derive(Debug, Clone, Default)]
pub struct WorkspaceRecommendations {
    pub mods: Vec<Recommendation>,
}

impl WorkspaceRecommendations {
    /// Compare these recommendations against the workspace mods config.
    ///
    /// If the config already matches the recommendations, returns None.
    ///
    /// Otherwise, returns the diff showing what to add and remove.
    pub fn diff_against(&self, config: &WorkspaceModsConfig) -> Option<RecommendationDiff> {
        // Get the set of recommended sources
        let recommended_sources: HashSet<_> = self.mods.iter().map(|r| r.source.clone()).collect();

        // Get the set of configured sources
        let configured_sources: HashSet<_> = config.mods.iter().map(|m| m.source.clone()).collect();

        // New = recommended but not configured
        let mut to_add = Vec::new();
        for m in &self.mods {
            if !configured_sources.contains(&m.source) {
                // New recommendation - add it enabled
                to_add.push(ModConfig {
                    kind: m.kind,
                    source: m.source.clone(),
                    when: m.when.clone().unwrap_or_default(),
                    enabled: true,
                });
            }
        }

        let mut to_remove = vec![];
        for m in &config.mods {
            if !recommended_sources.contains(&m.source) {
                // Stale - remove it
                to_remove.push(m.clone());
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
    /// Sources for mods that were newly recommended
    pub to_add: Vec<ModConfig>,

    /// Configuration for mods that were removed
    pub to_remove: Vec<ModConfig>,
}

impl RecommendationDiff {
    /// True if this diff has no changes
    pub fn is_empty(&self) -> bool {
        self.to_add.is_empty() && self.to_remove.is_empty()
    }

    /// Apply this diff to the given workspace mods config
    pub fn apply(&self, config: &mut WorkspaceModsConfig) {
        if self.is_empty() {
            return;
        }

        // Add new recommendations
        for m in &self.to_add {
            config.mods.push(m.clone());
        }

        // Remove stale recommendations
        config
            .mods
            .retain(|m| !self.to_remove.iter().any(|r| r.source == m.source));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use expect_test::expect;
    use serial_test::serial;
    use std::io::Write;
    use symposium_recommendations::{ComponentSource, ModKind};

    /// Write content to a file and sync to disk to avoid race conditions with cargo metadata
    fn write_synced(path: &Path, content: &str) {
        let mut file = std::fs::File::create(path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file.sync_all().unwrap();
    }

    #[test]
    fn test_load_builtin_recommendations() {
        let recs = Recommendations::load_builtin().expect("Should load builtin recommendations");

        // Should have some mod recommendations
        assert!(!recs.mods.is_empty(), "Should have mod recommendations");

        // Should have sparkle (always recommended) - it's a cargo source
        assert!(
            recs.mods.iter().any(|r| matches!(
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

        // Should only have the "always-on" mod
        assert_eq!(workspace_recs.mods.len(), 1);
        assert_eq!(workspace_recs.mods[0].display_name(), "always-on");

        // Now create Cargo.toml
        std::fs::write(temp_dir.path().join("Cargo.toml"), "[package]").unwrap();
        let workspace_recs = recs.for_workspace(temp_dir.path());

        // Should have both mods
        assert_eq!(workspace_recs.mods.len(), 2);
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
        assert_eq!(workspace_recs.mods.len(), 0);

        // Create Cargo.toml
        std::fs::write(temp_dir.path().join("Cargo.toml"), "[package]").unwrap();
        let workspace_recs = recs.for_workspace(temp_dir.path());
        assert_eq!(workspace_recs.mods.len(), 1);

        // Remove Cargo.toml, create package.json
        std::fs::remove_file(temp_dir.path().join("Cargo.toml")).unwrap();
        std::fs::write(temp_dir.path().join("package.json"), "{}").unwrap();
        let workspace_recs = recs.for_workspace(temp_dir.path());
        assert_eq!(workspace_recs.mods.len(), 1);
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
        assert_eq!(workspace_recs.mods.len(), 0);

        // Only Cargo.toml
        std::fs::write(temp_dir.path().join("Cargo.toml"), "[package]").unwrap();
        let workspace_recs = recs.for_workspace(temp_dir.path());
        assert_eq!(workspace_recs.mods.len(), 0);

        // Both files
        std::fs::create_dir_all(temp_dir.path().join("src")).unwrap();
        std::fs::write(temp_dir.path().join("src/lib.rs"), "").unwrap();
        let workspace_recs = recs.for_workspace(temp_dir.path());
        assert_eq!(workspace_recs.mods.len(), 1);
    }

    // ========================================================================
    // Diff tests
    // ========================================================================

    fn make_workspace_recs(mods: Vec<(&str, Option<When>)>) -> WorkspaceRecommendations {
        WorkspaceRecommendations {
            mods: mods
                .into_iter()
                .map(|(name, when)| Recommendation {
                    kind: ModKind::Proxy,
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
        let config = WorkspaceModsConfig::new(vec![]); // Empty config

        let diff = recs.diff_against(&config).expect("should have changes");

        expect![[r#"
            RecommendationDiff {
                to_add: [
                    ModConfig {
                        kind: Proxy,
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
                    ModConfig {
                        kind: Proxy,
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
    fn test_diff_stale_mods() {
        let recs = make_workspace_recs(vec![]); // No recommendations
        let mut config = WorkspaceModsConfig::new(vec![]);

        // Add a mod that's not recommended
        config.mods.push(ModConfig {
            kind: ModKind::Proxy,
            source: ComponentSource::Builtin("old-mod".to_string()),
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
                    ModConfig {
                        kind: Proxy,
                        source: Builtin(
                            "old-mod",
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
        let mut config = WorkspaceModsConfig::new(vec![]);

        // Add the same mod that's recommended
        config.mods.push(ModConfig {
            kind: ModKind::Proxy,
            source: ComponentSource::Builtin("foo".to_string()),
            enabled: true,
            when: When::default(),
        });

        let diff = recs.diff_against(&config);
        assert!(diff.is_none(), "No changes expected when in sync");
    }

    #[test]
    fn test_diff_disabled_mod_not_new() {
        // If a mod is in config but disabled, it's still "known" - not new
        let recs = make_workspace_recs(vec![("foo", None)]);
        let mut config = WorkspaceModsConfig::new(vec![]);
        config.mods.push(ModConfig {
            kind: ModKind::Proxy,
            source: ComponentSource::Builtin("foo".to_string()),
            enabled: false, // Disabled
            when: When::default(),
        });

        let diff = recs.diff_against(&config);
        // foo is not new because it's already in config (even though disabled)
        assert!(diff.is_none(), "Disabled mod should not count as new");
    }

    #[test]
    fn test_diff_apply() {
        let recs = make_workspace_recs(vec![("foo", None), ("bar", None)]);
        let mut config = WorkspaceModsConfig::new(vec![]);

        // Add a stale mod
        config.mods.push(ModConfig {
            kind: ModKind::Proxy,
            source: ComponentSource::Builtin("old".to_string()),
            enabled: true,
            when: When::default(),
        });

        let diff = recs.diff_against(&config).expect("should have changes");
        diff.apply(&mut config);

        // foo and bar should be added and enabled
        let foo_source = ComponentSource::Builtin("foo".to_string());
        let foo_mod = config.mods.iter().find(|m| m.source == foo_source);
        assert!(foo_mod.is_some() && foo_mod.unwrap().enabled);

        let bar_source = ComponentSource::Builtin("bar".to_string());
        let bar_mod = config.mods.iter().find(|m| m.source == bar_source);
        assert!(bar_mod.is_some() && bar_mod.unwrap().enabled);

        // old should be removed
        let old_source = ComponentSource::Builtin("old".to_string());
        assert!(!config.mods.iter().any(|m| m.source == old_source));
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
        assert_eq!(workspace_recs.mods.len(), 0);

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
        assert_eq!(workspace_recs.mods.len(), 1);
        assert_eq!(workspace_recs.mods[0].display_name(), "serde-helper");
    }

    #[test]
    fn test_for_workspace_merges_workspace_recommendations() {
        // Global recommendations with one mod
        let toml = r#"
[[recommendation]]
source.builtin = "global-mod"
"#;
        let global_recs = Recommendations::from_toml(toml).unwrap();

        // Create workspace with .symposium/recommendations.toml
        let temp_dir = tempfile::tempdir().unwrap();
        let symposium_dir = temp_dir.path().join(".symposium");
        std::fs::create_dir_all(&symposium_dir).unwrap();
        std::fs::write(
            symposium_dir.join("recommendations.toml"),
            r#"
[[recommendation]]
source.builtin = "workspace-mod"
"#,
        )
        .unwrap();

        let workspace_recs = global_recs.for_workspace(temp_dir.path());

        // Should have both global and workspace mods
        assert_eq!(workspace_recs.mods.len(), 2);
        let names: Vec<_> = workspace_recs
            .mods
            .iter()
            .map(|r| r.display_name())
            .collect();
        assert!(names.contains(&"global-mod".to_string()));
        assert!(names.contains(&"workspace-mod".to_string()));
    }

    #[test]
    fn test_for_workspace_filters_workspace_recommendations_by_condition() {
        // Global recommendations
        let toml = r#"
[[recommendation]]
source.builtin = "global-mod"
"#;
        let global_recs = Recommendations::from_toml(toml).unwrap();

        // Create workspace with conditional recommendation
        let temp_dir = tempfile::tempdir().unwrap();
        let symposium_dir = temp_dir.path().join(".symposium");
        std::fs::create_dir_all(&symposium_dir).unwrap();
        std::fs::write(
            symposium_dir.join("recommendations.toml"),
            r#"
[[recommendation]]
source.builtin = "always-mod"

[[recommendation]]
source.builtin = "rust-only-mod"
when.file-exists = "Cargo.toml"
"#,
        )
        .unwrap();

        // Without Cargo.toml
        let workspace_recs = global_recs.for_workspace(temp_dir.path());
        assert_eq!(workspace_recs.mods.len(), 2); // global-mod + always-mod
        let names: Vec<_> = workspace_recs
            .mods
            .iter()
            .map(|r| r.display_name())
            .collect();
        assert!(names.contains(&"global-mod".to_string()));
        assert!(names.contains(&"always-mod".to_string()));
        assert!(!names.contains(&"rust-only-mod".to_string()));

        // With Cargo.toml
        std::fs::write(temp_dir.path().join("Cargo.toml"), "[package]").unwrap();
        let workspace_recs = global_recs.for_workspace(temp_dir.path());
        assert_eq!(workspace_recs.mods.len(), 3); // all three mods
        let names: Vec<_> = workspace_recs
            .mods
            .iter()
            .map(|r| r.display_name())
            .collect();
        assert!(names.contains(&"rust-only-mod".to_string()));
    }
}
