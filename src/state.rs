//! SQLite state layer via toasty.
//!
//! Tracks skill activations, nudges, session state, and workspace cache
//! for the hook system. Database location: `<config_dir>/state.0.sqlite`.
//!
//! The filename encodes the schema version (`.0.`). On breaking schema changes,
//! bump to `.1.sqlite` etc. — no migration code needed.

use std::path::Path;

use anyhow::Result;

/// Schema version embedded in the DB filename.
const SCHEMA_VERSION: u32 = 0;

/// Records that the agent loaded a skill for a specific crate in this session.
#[derive(Debug, toasty::Model)]
pub struct SkillActivation {
    #[key]
    #[auto]
    pub id: u64,

    #[index]
    pub session_id: String,

    pub crate_name: String,

    pub activated_at: String,
}

/// Records that we nudged the agent about a crate skill, and at which prompt count.
#[derive(Debug, toasty::Model)]
pub struct SkillNudge {
    #[key]
    #[auto]
    pub id: u64,

    #[index]
    pub session_id: String,

    pub crate_name: String,

    /// The prompt count at which this nudge was sent.
    pub at_prompt: i64,
}

/// Per-session prompt counter.
#[derive(Debug, toasty::Model)]
pub struct SessionState {
    #[key]
    pub session_id: String,

    /// Incremented on each UserPromptSubmit.
    pub prompt_count: i64,
}

/// Cached workspace deps, keyed by cwd. Refreshed when Cargo.lock mtime changes.
#[derive(Debug, toasty::Model)]
pub struct WorkspaceCache {
    #[key]
    pub cwd: String,

    /// Cargo.lock mtime (seconds since epoch) at time of caching.
    pub cargo_lock_mtime: i64,

    /// JSON array of {name, version} for workspace deps with available skills.
    pub deps_json: String,
}

/// Available skills for a workspace directory, populated during the
/// Cargo.lock mtime refresh step at hook entry.
#[derive(Debug, toasty::Model)]
pub struct AvailableSkill {
    #[key]
    #[auto]
    pub id: u64,

    /// The cwd this skill is relevant to.
    #[index]
    pub cwd: String,

    /// Crate name (e.g., "tokio").
    pub crate_name: String,

    /// Resolved directory path containing the SKILL.md file.
    pub skill_dir_path: String,
}

/// Open (or create) the state database.
pub async fn open_db(config_dir: &Path) -> Result<toasty::Db> {
    let db_path = config_dir.join(format!("state.{SCHEMA_VERSION}.sqlite"));
    let url = format!("sqlite://{}", db_path.display());

    let db = toasty::Db::builder()
        .models(toasty::models!(
            SkillActivation,
            SkillNudge,
            SessionState,
            WorkspaceCache,
            AvailableSkill,
        ))
        .connect(&url)
        .await
        .map_err(|e| anyhow::anyhow!("failed to open state DB at {}: {e}", db_path.display()))?;

    db.push_schema()
        .await
        .map_err(|e| anyhow::anyhow!("failed to create state DB schema: {e}"))?;

    Ok(db)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn open_db_creates_file() {
        let tmp = tempfile::tempdir().unwrap();
        let mut db = open_db(tmp.path()).await.unwrap();

        // Verify the DB file was created
        let db_path = tmp.path().join("state.0.sqlite");
        assert!(db_path.exists());

        // Verify we can do a basic operation
        toasty::create!(SkillActivation {
            session_id: "test-session",
            crate_name: "tokio",
            activated_at: "2024-01-01T00:00:00Z",
        })
        .exec(&mut db)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn session_state_crud() {
        let tmp = tempfile::tempdir().unwrap();
        let mut db = open_db(tmp.path()).await.unwrap();

        // Create
        toasty::create!(SessionState {
            session_id: "s1",
            prompt_count: 0,
        })
        .exec(&mut db)
        .await
        .unwrap();

        // Read
        let state = SessionState::get_by_session_id(&mut db, "s1").await.unwrap();
        assert_eq!(state.prompt_count, 0);
    }

    #[tokio::test]
    async fn skill_activation_insert_and_query() {
        let tmp = tempfile::tempdir().unwrap();
        let mut db = open_db(tmp.path()).await.unwrap();

        toasty::create!(SkillActivation {
            session_id: "s1",
            crate_name: "tokio",
            activated_at: "2024-01-01",
        })
        .exec(&mut db)
        .await
        .unwrap();

        toasty::create!(SkillActivation {
            session_id: "s1",
            crate_name: "serde",
            activated_at: "2024-01-01",
        })
        .exec(&mut db)
        .await
        .unwrap();

        // Query all activations for session s1 via the indexed field
        let activations: Vec<SkillActivation> =
            SkillActivation::filter_by_session_id("s1")
                .exec(&mut db)
                .await
                .unwrap();
        assert_eq!(activations.len(), 2);
    }
}
