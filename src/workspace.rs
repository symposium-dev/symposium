//! Workspace metadata: on-demand computation of deps and available skills.

use std::path::Path;

use anyhow::Result;

use crate::config::Symposium;

/// An available skill for a specific crate in the workspace.
#[derive(Debug, Clone)]
pub struct AvailableSkill {
    /// Crate name (e.g., "tokio").
    pub crate_name: String,
    /// Resolved directory path containing the SKILL.md file.
    pub skill_dir_path: String,
}

/// Compute the available skills for the given workspace directory.
///
/// Scans workspace deps from Cargo metadata, loads plugins, and resolves
/// which skill groups match. Returns the list of available skills in memory
/// (no DB caching — recomputed each time).
pub async fn compute_available_skills(
    sym: &Symposium,
    cwd: &Path,
) -> Result<Vec<AvailableSkill>> {
    let deps = crate::crate_sources::workspace_semver_pairs(cwd);
    let registry = crate::plugins::load_registry(sym);
    let skills = crate::skills::list_output_raw(sym, &registry, &deps).await;

    let mut available = Vec::new();
    for entry in &skills {
        let skill_dir = entry
            .skill
            .path
            .parent()
            .unwrap_or(&entry.skill.path)
            .to_string_lossy()
            .to_string();

        for crate_name in entry.effective_crate_names() {
            available.push(AvailableSkill {
                crate_name,
                skill_dir_path: skill_dir.clone(),
            });
        }
    }

    Ok(available)
}
