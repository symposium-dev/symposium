//! List workspace crates with available guidance

use std::path::{Path, PathBuf};

use anyhow::Result;
use cargo_metadata::{CargoOpt, MetadataCommand};

/// A crate in the workspace's dependency graph.
#[derive(Debug, Clone)]
pub struct WorkspaceCrate {
    pub name: String,
    pub version: semver::Version,
    /// Local source path for path dependencies (from `cargo metadata`).
    /// `None` for registry crates.
    pub path: Option<PathBuf>,
}

/// Load workspace crates with parsed versions and local path overrides.
///
/// Combines dependency resolution and path-override detection into a single
/// `cargo metadata` call. Path dependencies (those with no registry source)
/// get their `path` field populated so callers can resolve them locally.
pub fn workspace_crates(cwd: &Path) -> Vec<WorkspaceCrate> {
    load_workspace_crates(cwd).unwrap_or_default()
}

fn load_workspace_crates(cwd: &Path) -> Result<Vec<WorkspaceCrate>> {
    let metadata = MetadataCommand::new()
        .features(CargoOpt::AllFeatures)
        .current_dir(cwd)
        .exec()?;

    let resolve = match &metadata.resolve {
        Some(r) => r,
        None => return Ok(Vec::new()),
    };

    // Collect direct dependency package IDs from workspace member nodes.
    let ws_members: std::collections::HashSet<_> = metadata.workspace_members.iter().collect();
    let mut direct_dep_ids: std::collections::HashSet<&cargo_metadata::PackageId> =
        std::collections::HashSet::new();

    for node in &resolve.nodes {
        if ws_members.contains(&node.id) {
            for dep in &node.deps {
                direct_dep_ids.insert(&dep.pkg);
            }
        }
    }

    // Build a map from package name to local source path for path deps
    // (packages with no registry source).
    let path_overrides: std::collections::HashMap<String, PathBuf> = metadata
        .packages
        .iter()
        .filter(|p| p.source.is_none())
        .filter_map(|p| {
            p.manifest_path
                .parent()
                .map(|dir| (p.name.clone(), dir.into()))
        })
        .collect();

    // Map package IDs to WorkspaceCrate, excluding workspace members themselves.
    let mut crates: Vec<_> = metadata
        .packages
        .iter()
        .filter(|p| direct_dep_ids.contains(&p.id) && !ws_members.contains(&p.id))
        .filter_map(|p| {
            semver::Version::parse(&p.version.to_string())
                .ok()
                .map(|v| WorkspaceCrate {
                    path: path_overrides.get(&p.name).cloned(),
                    name: p.name.to_string(),
                    version: v,
                })
        })
        .collect();

    crates.sort_by(|a, b| a.name.cmp(&b.name));
    crates.dedup_by(|a, b| a.name == b.name);

    Ok(crates)
}
