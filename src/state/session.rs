//! Per-session state: prompt counting, skill activations, and nudges.
//!
//! State is stored as a JSON file per session under `<config_dir>/sessions/`.
//! All operations are in-memory on `SessionData`; the caller is responsible
//! for loading before and saving after mutations.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// All state for a single session, serialized to/from JSON.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SessionData {
    /// Running prompt count, incremented on each UserPromptSubmit.
    pub prompt_count: i64,

    /// Crate names whose skills have been loaded in this session.
    pub activations: BTreeSet<String>,

    /// Nudge history: crate name → prompt count at which the last nudge was sent.
    pub nudges: BTreeMap<String, i64>,
}

impl SessionData {
    /// Record that the agent loaded a skill for `crate_name`.
    pub fn record_activation(&mut self, crate_name: &str) {
        self.activations.insert(crate_name.to_string());
        tracing::info!(crate_name, "recorded skill activation");
    }

    /// Increment the prompt count, returning the new value.
    pub fn increment_prompt_count(&mut self) -> i64 {
        self.prompt_count += 1;
        self.prompt_count
    }

    /// Determine which of the `mentioned` crates should be nudged about,
    /// record the nudges, and return the list of crate names to include
    /// in the hook output.
    ///
    /// A crate is nudged when:
    /// - It has not been activated in this session, AND
    /// - It has never been nudged, OR enough prompts have elapsed since the last nudge.
    pub fn compute_nudges(&mut self, mentioned: &[String], nudge_interval: i64) -> Vec<String> {
        let prompt_count = self.prompt_count;
        let mut nudge_crates = Vec::new();

        for crate_name in mentioned {
            if self.activations.contains(crate_name) {
                continue;
            }

            let should_nudge = match self.nudges.get(crate_name) {
                None => true,
                Some(&last_prompt) => prompt_count - last_prompt >= nudge_interval,
            };

            if should_nudge {
                nudge_crates.push(crate_name.clone());
                self.nudges.insert(crate_name.clone(), prompt_count);
            }
        }

        nudge_crates
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_activation_and_check() {
        let mut session = SessionData::default();
        session.record_activation("tokio");
        session.record_activation("serde");
        assert!(session.activations.contains("tokio"));
        assert!(session.activations.contains("serde"));
        assert_eq!(session.activations.len(), 2);
    }

    #[test]
    fn record_activation_is_idempotent() {
        let mut session = SessionData::default();
        session.record_activation("tokio");
        session.record_activation("tokio");
        assert_eq!(session.activations.len(), 1);
    }

    #[test]
    fn increment_prompt_count_works() {
        let mut session = SessionData::default();
        assert_eq!(session.increment_prompt_count(), 1);
        assert_eq!(session.increment_prompt_count(), 2);
        assert_eq!(session.increment_prompt_count(), 3);
    }

    #[test]
    fn compute_nudges_first_mention() {
        let mut session = SessionData::default();
        session.prompt_count = 1;

        let result = session.compute_nudges(&["tokio".to_string()], 50);
        assert_eq!(result, vec!["tokio"]);
        assert_eq!(session.nudges["tokio"], 1);
    }

    #[test]
    fn compute_nudges_skips_activated() {
        let mut session = SessionData::default();
        session.record_activation("tokio");
        session.prompt_count = 1;

        let result = session.compute_nudges(&["tokio".to_string()], 50);
        assert!(result.is_empty());
    }

    #[test]
    fn compute_nudges_respects_interval() {
        let mut session = SessionData::default();

        // First nudge at prompt 1
        session.prompt_count = 1;
        let result = session.compute_nudges(&["tokio".to_string()], 50);
        assert_eq!(result, vec!["tokio"]);

        // Too soon at prompt 10
        session.prompt_count = 10;
        let result = session.compute_nudges(&["tokio".to_string()], 50);
        assert!(result.is_empty());

        // Enough elapsed at prompt 51
        session.prompt_count = 51;
        let result = session.compute_nudges(&["tokio".to_string()], 50);
        assert_eq!(result, vec!["tokio"]);
    }

    #[test]
    fn serialization_roundtrip() {
        let mut session = SessionData::default();
        session.prompt_count = 5;
        session.record_activation("tokio");
        session.nudges.insert("serde".to_string(), 3);

        let json = serde_json::to_string(&session).unwrap();
        let loaded: SessionData = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.prompt_count, 5);
        assert!(loaded.activations.contains("tokio"));
        assert_eq!(loaded.nudges["serde"], 3);
    }
}
