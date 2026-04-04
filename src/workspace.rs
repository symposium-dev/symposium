//! Workspace metadata: deps resolution with mtime-based caching.
//!
//! Provides the "ensure DB is fresh" step that hooks call on entry.
//! Checks Cargo.lock mtime against `WorkspaceCache` in the DB.
//! If stale, re-scans workspace deps, loads plugins, resolves skill
//! directories, and upserts `AvailableSkill` rows.

use std::path::Path;

use anyhow::Result;

use crate::config::Symposium;
use crate::state;

/// Cached workspace information, returned by `ensure_fresh`.
pub struct WorkspaceInfo {
    /// Workspace dependencies as (name, version) pairs for predicate evaluation.
    pub deps: Vec<(String, semver::Version)>,
}

/// Ensure the DB has up-to-date workspace data for `cwd`.
///
/// Checks Cargo.lock mtime. If unchanged from the cached value, returns
/// cached deps. If changed (or first run), re-scans the workspace,
/// loads plugins, resolves skill directories, and upserts `AvailableSkill` rows.
pub async fn ensure_fresh(
    sym: &Symposium,
    db: &mut toasty::Db,
    cwd: &Path,
) -> Result<WorkspaceInfo> {
    let cwd_str = cwd.to_string_lossy().to_string();

    // Check Cargo.lock mtime
    let lock_mtime = cargo_lock_mtime(cwd);

    // Check DB cache
    if let Ok(cached) = state::WorkspaceCache::get_by_cwd(db, &cwd_str).await {
        if cached.cargo_lock_mtime == lock_mtime {
            // Cache hit — parse deps from JSON
            let deps = parse_deps_json(&cached.deps_json);
            return Ok(WorkspaceInfo { deps });
        }
    }

    // Cache miss — re-scan
    let deps = crate::crate_sources::workspace_semver_pairs(cwd);

    // Serialize deps to JSON for caching
    let deps_json = serialize_deps_json(&deps);

    // Upsert workspace cache
    // Delete old entry if exists, then insert new one
    let _ = state::WorkspaceCache::filter_by_cwd(&cwd_str)
        .delete()
        .exec(db)
        .await;

    toasty::create!(state::WorkspaceCache {
        cwd: cwd_str.clone(),
        cargo_lock_mtime: lock_mtime,
        deps_json: deps_json,
    })
    .exec(db)
    .await
    .map_err(|e| anyhow::anyhow!("failed to cache workspace deps: {e}"))?;

    // Refresh available skills
    refresh_available_skills(sym, db, &cwd_str, &deps).await?;

    Ok(WorkspaceInfo { deps })
}

/// Refresh `AvailableSkill` rows for the given cwd.
///
/// Loads plugins, resolves skill directories for matching groups,
/// and upserts rows into the DB.
async fn refresh_available_skills(
    sym: &Symposium,
    db: &mut toasty::Db,
    cwd: &str,
    deps: &[(String, semver::Version)],
) -> Result<()> {
    // Delete old available skills for this cwd
    let _ = state::AvailableSkill::filter_by_cwd(cwd)
        .delete()
        .exec(db)
        .await;

    let registry = crate::plugins::load_registry(sym);

    // Collect available skills from plugin skill groups
    let skills = crate::skills::list_output_raw(sym, &registry, deps).await;

    for entry in &skills {
        let skill_dir = entry
            .skill
            .path
            .parent()
            .unwrap_or(&entry.skill.path)
            .to_string_lossy()
            .to_string();

        for crate_name in entry.effective_crate_names() {
            toasty::create!(state::AvailableSkill {
                cwd: cwd.to_string(),
                crate_name: crate_name,
                skill_dir_path: skill_dir.clone(),
            })
            .exec(db)
            .await
            .map_err(|e| anyhow::anyhow!("failed to insert available skill: {e}"))?;
        }
    }

    Ok(())
}

/// Get Cargo.lock mtime as seconds since epoch, or 0 if not found.
fn cargo_lock_mtime(cwd: &Path) -> i64 {
    let lock_path = cwd.join("Cargo.lock");
    match std::fs::metadata(&lock_path) {
        Ok(meta) => meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
        Err(_) => 0,
    }
}

fn serialize_deps_json(deps: &[(String, semver::Version)]) -> String {
    let entries: Vec<serde_json::Value> = deps
        .iter()
        .map(|(name, version)| {
            serde_json::json!({
                "name": name,
                "version": version.to_string(),
            })
        })
        .collect();
    serde_json::to_string(&entries).unwrap_or_else(|_| "[]".to_string())
}

fn parse_deps_json(json: &str) -> Vec<(String, semver::Version)> {
    let entries: Vec<serde_json::Value> = serde_json::from_str(json).unwrap_or_default();
    entries
        .into_iter()
        .filter_map(|v| {
            let name = v.get("name")?.as_str()?.to_string();
            let version_str = v.get("version")?.as_str()?;
            let version = semver::Version::parse(version_str).ok()?;
            Some((name, version))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_and_parse_deps() {
        let deps = vec![
            ("serde".to_string(), semver::Version::new(1, 0, 210)),
            ("tokio".to_string(), semver::Version::new(1, 40, 0)),
        ];
        let json = serialize_deps_json(&deps);
        let parsed = parse_deps_json(&json);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].0, "serde");
        assert_eq!(parsed[0].1, semver::Version::new(1, 0, 210));
    }

    #[test]
    fn parse_empty_deps_json() {
        assert!(parse_deps_json("").is_empty());
        assert!(parse_deps_json("[]").is_empty());
    }
}
