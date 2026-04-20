//! List workspace crates with available guidance

use std::path::Path;

use anyhow::Result;
use cargo_metadata::{CargoOpt, MetadataCommand};

/// A crate in the workspace's dependency graph
pub struct WorkspaceCrate {
    pub name: String,
    pub version: String,
}

/// Load workspace crates and return as `(name, semver::Version)` pairs
/// for predicate evaluation. Returns an empty list on failure.
pub fn workspace_semver_pairs(cwd: &Path) -> Vec<(String, semver::Version)> {
    list_all_workspace_crates(cwd)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|c| semver::Version::parse(&c.version).ok().map(|v| (c.name, v)))
        .collect()
}

/// List direct dependencies of workspace members.
///
/// Uses the resolve graph to identify direct dependencies rather than
/// `metadata.packages` which includes the entire transitive closure.
fn list_all_workspace_crates(cwd: &Path) -> Result<Vec<WorkspaceCrate>> {
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

    // Map package IDs to name/version, excluding workspace members themselves.
    let mut crates: Vec<_> = metadata
        .packages
        .iter()
        .filter(|p| direct_dep_ids.contains(&p.id) && !ws_members.contains(&p.id))
        .map(|p| WorkspaceCrate {
            name: p.name.to_string(),
            version: p.version.to_string(),
        })
        .collect();

    crates.sort_by(|a, b| a.name.cmp(&b.name));
    crates.dedup_by(|a, b| a.name == b.name);

    Ok(crates)
}
