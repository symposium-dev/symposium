//! Workspace metadata: on-demand computation of deps and applicable skills.
//!
//! Terminology:
//! - **available**: a skill found in a plugin source.
//! - **applicable**: a skill whose crate predicate matches something.
//! - **applicable to workspace**: matches a crate in the workspace deps.
//! - **applicable to request**: matches the specific crate being queried.

use std::path::Path;

use anyhow::Result;

use crate::config::Symposium;

/// A skill applicable to a specific crate in the workspace.
#[derive(Debug, Clone)]
pub struct ApplicableSkill {
    /// Crate name (e.g., "tokio").
    pub crate_name: String,
    /// Resolved directory path containing the SKILL.md file.
    pub skill_dir_path: String,
}

/// Compute the skills applicable to the given workspace directory.
///
/// Scans workspace deps from Cargo metadata, loads plugins, and resolves
/// which skill groups match workspace crates. Returns one entry per
/// (crate_name, skill_dir) pair — a single skill group that covers multiple
/// crates produces multiple entries.
pub async fn compute_skills_applicable_to_workspace(
    sym: &Symposium,
    cwd: &Path,
) -> Result<Vec<ApplicableSkill>> {
    let deps = crate::crate_sources::workspace_semver_pairs(cwd);
    let registry = crate::plugins::load_registry(sym);
    let skills = crate::skills::skills_applicable_to(sym, &registry, &deps).await;

    let mut applicable = Vec::new();
    for entry in &skills {
        let skill_dir = entry
            .skill
            .path
            .parent()
            .unwrap_or(&entry.skill.path)
            .to_string_lossy()
            .to_string();

        // A single skill group may cover multiple crates (e.g., "serde | serde_json").
        // Produce one ApplicableSkill per crate name so callers can look up by crate.
        for crate_name in entry.effective_crate_names() {
            applicable.push(ApplicableSkill {
                crate_name,
                skill_dir_path: skill_dir.clone(),
            });
        }
    }

    Ok(applicable)
}
