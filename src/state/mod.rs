//! SQLite state layer via toasty.
//!
//! Tracks skill activations, nudges, and session state for the hook system.
//! Database location: `<config_dir>/state.0.sqlite`.
//!
//! The filename encodes the schema version (`.0.`). On breaking schema changes,
//! bump to `.1.sqlite` etc. -- no migration code needed.

pub mod session;

use std::path::Path;

use anyhow::Result;

/// Schema version embedded in the DB filename.
const SCHEMA_VERSION: u32 = 0;

/// Open (or create) the state database.
pub async fn open_db(config_dir: &Path) -> Result<toasty::Db> {
    let db_path = config_dir.join(format!("state.{SCHEMA_VERSION}.sqlite"));
    let url = format!("sqlite://{}", db_path.display());

    let db = toasty::Db::builder()
        .models(toasty::models!(
            session::SkillActivation,
            session::SkillNudge,
            session::SessionState,
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
        toasty::create!(session::SkillActivation {
            session_id: "test-session",
            crate_name: "tokio",
            activated_at: "2024-01-01T00:00:00Z",
        })
        .exec(&mut db)
        .await
        .unwrap();
    }
}
