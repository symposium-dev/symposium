//! Per-session state: prompt counting, skill activations, and nudges.
//!
//! All session-scoped DB operations live here so that hook handlers
//! remain thin (parse payload -> call helpers -> format output).

/// Records that the agent loaded a skill for a specific crate in this session.
#[derive(Debug, toasty::Model)]
pub struct SkillActivation {
    #[key]
    #[auto]
    pub id: u64,

    /// Identifies the Claude Code session (from hook payload).
    #[index]
    pub session_id: String,

    /// Crate whose skill was loaded (e.g. "tokio").
    pub crate_name: String,

    /// RFC 3339 timestamp of when the activation was recorded.
    pub activated_at: String,
}

/// Records that we nudged the agent about a crate skill, and at which prompt count.
///
/// Rows are append-only: the latest `at_prompt` per crate determines re-nudge timing.
#[derive(Debug, toasty::Model)]
pub struct SkillNudge {
    #[key]
    #[auto]
    pub id: u64,

    /// Identifies the Claude Code session.
    #[index]
    pub session_id: String,

    /// Crate we nudged about.
    pub crate_name: String,

    /// The session prompt count at which this nudge was sent.
    pub at_prompt: i64,
}

/// Per-session prompt counter, incremented on each UserPromptSubmit.
#[derive(Debug, toasty::Model)]
pub struct SessionState {
    /// Session identifier (primary key).
    #[key]
    pub session_id: String,

    /// Running prompt count for this session.
    pub prompt_count: i64,
}

// ---------------------------------------------------------------------------
// DB helpers
// ---------------------------------------------------------------------------

/// Record a skill activation for `crate_name` in the given session.
pub async fn record_activation(db: &mut toasty::Db, session_id: &str, crate_name: &str) {
    let now = chrono::Utc::now().to_rfc3339();
    if let Err(e) = toasty::create!(SkillActivation {
        session_id: session_id.to_string(),
        crate_name: crate_name.to_string(),
        activated_at: now,
    })
    .exec(db)
    .await
    {
        tracing::warn!(
            session_id,
            crate_name,
            error = %e,
            "failed to record skill activation"
        );
    } else {
        tracing::info!(session_id, crate_name, "recorded skill activation");
    }
}

/// Increment the session prompt count, returning the new count.
pub async fn increment_prompt_count(db: &mut toasty::Db, session_id: &str) -> i64 {
    match SessionState::get_by_session_id(db, session_id).await {
        Ok(state) => {
            let new_count = state.prompt_count + 1;
            // toasty doesn't track in-place field mutations, so delete+re-insert.
            let _ = SessionState::filter_by_session_id(session_id)
                .delete()
                .exec(db)
                .await;
            let _ = toasty::create!(SessionState {
                session_id: session_id.to_string(),
                prompt_count: new_count,
            })
            .exec(db)
            .await;
            new_count
        }
        Err(_) => {
            // First prompt in this session
            let _ = toasty::create!(SessionState {
                session_id: session_id.to_string(),
                prompt_count: 1,
            })
            .exec(db)
            .await;
            1
        }
    }
}

/// Determine which of the `mentioned` crates should be nudged about,
/// record the nudges, and return the list of crate names to include
/// in the hook output.
///
/// A crate is nudged when:
/// - It has not been activated in this session, AND
/// - It has never been nudged, OR enough prompts have elapsed since the last nudge.
pub async fn compute_nudges(
    db: &mut toasty::Db,
    session_id: &str,
    mentioned: &[String],
    nudge_interval: i64,
    prompt_count: i64,
) -> Vec<String> {
    // Load activations for this session
    let activations: Vec<SkillActivation> =
        match SkillActivation::filter_by_session_id(session_id)
            .exec(db)
            .await
        {
            Ok(a) => a,
            Err(_) => Vec::new(),
        };
    let activated_crates: std::collections::HashSet<&str> =
        activations.iter().map(|a| a.crate_name.as_str()).collect();

    // Load nudges for this session
    let nudges: Vec<SkillNudge> = match SkillNudge::filter_by_session_id(session_id)
        .exec(db)
        .await
    {
        Ok(n) => n,
        Err(_) => Vec::new(),
    };

    let mut nudge_crates = Vec::new();

    for crate_name in mentioned {
        if activated_crates.contains(crate_name.as_str()) {
            continue;
        }

        let existing_nudge = nudges
            .iter()
            .filter(|n| n.crate_name == *crate_name)
            .max_by_key(|n| n.at_prompt);

        let should_nudge = match existing_nudge {
            None => true,
            Some(nudge) => prompt_count - nudge.at_prompt >= nudge_interval,
        };

        if should_nudge {
            nudge_crates.push(crate_name.clone());
            record_nudge(db, session_id, crate_name, prompt_count).await;
        }
    }

    nudge_crates
}

/// Record a nudge in the DB (append-only).
async fn record_nudge(db: &mut toasty::Db, session_id: &str, crate_name: &str, at_prompt: i64) {
    let _ = toasty::create!(SkillNudge {
        session_id: session_id.to_string(),
        crate_name: crate_name.to_string(),
        at_prompt: at_prompt,
    })
    .exec(db)
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_db() -> (tempfile::TempDir, toasty::Db) {
        let tmp = tempfile::tempdir().unwrap();
        let db = crate::state::open_db(tmp.path()).await.unwrap();
        (tmp, db)
    }

    #[tokio::test]
    async fn session_state_crud() {
        let (_tmp, mut db) = test_db().await;

        toasty::create!(SessionState {
            session_id: "s1",
            prompt_count: 0,
        })
        .exec(&mut db)
        .await
        .unwrap();

        let state = SessionState::get_by_session_id(&mut db, "s1")
            .await
            .unwrap();
        assert_eq!(state.prompt_count, 0);
    }

    #[tokio::test]
    async fn skill_activation_insert_and_query() {
        let (_tmp, mut db) = test_db().await;

        record_activation(&mut db, "s1", "tokio").await;
        record_activation(&mut db, "s1", "serde").await;

        let activations: Vec<SkillActivation> = SkillActivation::filter_by_session_id("s1")
            .exec(&mut db)
            .await
            .unwrap();
        assert_eq!(activations.len(), 2);
    }

    #[tokio::test]
    async fn increment_prompt_count_works() {
        let (_tmp, mut db) = test_db().await;

        assert_eq!(increment_prompt_count(&mut db, "s1").await, 1);
        assert_eq!(increment_prompt_count(&mut db, "s1").await, 2);
        assert_eq!(increment_prompt_count(&mut db, "s1").await, 3);
    }

    #[tokio::test]
    async fn compute_nudges_first_mention() {
        let (_tmp, mut db) = test_db().await;
        let _ = increment_prompt_count(&mut db, "s1").await;

        let result = compute_nudges(
            &mut db,
            "s1",
            &["tokio".to_string()],
            50,
            1,
        )
        .await;
        assert_eq!(result, vec!["tokio"]);
    }

    #[tokio::test]
    async fn compute_nudges_skips_activated() {
        let (_tmp, mut db) = test_db().await;

        record_activation(&mut db, "s1", "tokio").await;

        let result = compute_nudges(
            &mut db,
            "s1",
            &["tokio".to_string()],
            50,
            1,
        )
        .await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn compute_nudges_respects_interval() {
        let (_tmp, mut db) = test_db().await;

        // First nudge at prompt 1
        let result = compute_nudges(&mut db, "s1", &["tokio".to_string()], 50, 1).await;
        assert_eq!(result, vec!["tokio"]);

        // Too soon at prompt 10
        let result = compute_nudges(&mut db, "s1", &["tokio".to_string()], 50, 10).await;
        assert!(result.is_empty());

        // Enough elapsed at prompt 51
        let result = compute_nudges(&mut db, "s1", &["tokio".to_string()], 50, 51).await;
        assert_eq!(result, vec!["tokio"]);
    }
}
