//! Which skills a Claude Code transcript shows reaching the agent (RFD reference,
//! "Skills used").
//!
//! A skill is loaded by a `Skill` call or a `Read` of its installed `SKILL.md` whose
//! result is not an error; an error result is an attempt. `cat SKILL.md` is not
//! detected. Subagent calls count. Pairing calls with results is [`super::calls`]'.

use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use super::calls::{Matcher, Outcome, Target};
use crate::client::{InstalledSkill, Load};

#[derive(Debug, Deserialize)]
struct SkillInput {
    skill: String,
}

#[derive(Debug, Deserialize)]
struct ReadInput {
    file_path: String,
}

/// Matches tool calls to the skills they would load.
pub(super) struct Skills<'a> {
    skills: &'a [InstalledSkill],
    /// Where Claude Code ran, which a relative `Read` path is resolved against.
    project: &'a Path,
}

impl<'a> Skills<'a> {
    pub(super) fn new(skills: &'a [InstalledSkill], project: &'a Path) -> Self {
        Self { skills, project }
    }

    fn position(&self, matches: impl Fn(usize) -> bool) -> Option<usize> {
        (0..self.skills.len()).find(|&index| matches(index))
    }
}

impl Matcher for Skills<'_> {
    fn targets(&self) -> usize {
        self.skills.len()
    }

    fn target(&self, name: &str, input: Value) -> Target {
        let found = match name {
            "Skill" => {
                let Ok(SkillInput { skill: called }) = serde_json::from_value(input) else {
                    return Target::Unreadable;
                };
                // A plugin's skill is named `<plugin>:<skill>`.
                let called = called.rsplit(':').next();
                self.position(|index| called == Some(self.skills[index].name.as_str()))
            }
            "Read" => {
                let Ok(ReadInput { file_path }) = serde_json::from_value(input) else {
                    return Target::Unreadable;
                };
                let read = normalize(&self.project.join(file_path));
                self.position(|index| normalize(&self.skills[index].skill_md) == read)
            }
            _ => None,
        };
        found.map_or(Target::None, Target::Of)
    }
}

impl From<Outcome> for Load {
    fn from(outcome: Outcome) -> Self {
        match outcome {
            Outcome::Succeeded => Load::Loaded,
            Outcome::Failed => Load::Attempted,
            Outcome::NotObserved => Load::NotObserved,
            Outcome::Unknown => Load::Unknown,
        }
    }
}

/// `path` with `.` and `..` resolved by name alone, since the transcript's paths need
/// not exist when it is read.
fn normalize(path: &Path) -> PathBuf {
    let mut normal = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normal.pop();
            }
            other => normal.push(other),
        }
    }
    normal
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::client::claude::transcript::read_text;
    use crate::client::claude::transcript::tests::{
        FINISHED, recorded_session, without_user_sessions,
    };
    use crate::client::claude::version;
    use crate::task::SkillName;

    /// Recorded from Claude Code 2.1.316, with personal details removed: a `Skill`
    /// call, a `Read` of the installed `SKILL.md`, and a `Skill` call for a skill that
    /// does not exist.
    const SKILL_LOADED: &str = include_str!("transcripts/skill_loaded.jsonl");
    const READ_LOADED: &str = include_str!("transcripts/read_loaded.jsonl");
    const SKILL_FAILED: &str = include_str!("transcripts/skill_failed.jsonl");

    const PROJECT: &str = "/trial/project";

    fn skill(name: &str, dir: &str) -> InstalledSkill {
        InstalledSkill {
            name: SkillName::new(name).unwrap(),
            skill_md: Path::new(PROJECT)
                .join(".claude/skills")
                .join(dir)
                .join("SKILL.md"),
        }
    }

    fn greeting() -> InstalledSkill {
        skill("fixture-greeting", "fixture-greeting")
    }

    fn loads_in(transcript: &str, skills: &[InstalledSkill]) -> Vec<Load> {
        read_text(
            transcript,
            Path::new(PROJECT),
            recorded_session(),
            version::MINIMUM,
            skills,
        )
        .loads
    }

    /// The first line of `transcript` that contains `marker`, as another session's.
    fn foreign(transcript: &str, marker: &str) -> String {
        let line = transcript
            .lines()
            .find(|line| line.contains(marker))
            .unwrap();
        line.replace(&recorded_session().to_string(), &Uuid::new_v4().to_string())
    }

    fn load_of(transcript: &str) -> Load {
        loads_in(transcript, &[greeting()])[0]
    }

    /// `transcript` cut just before its first line that contains `marker`.
    fn cut_before(transcript: &str, marker: &str) -> String {
        let end = transcript.find(marker).unwrap();
        let start = transcript[..end].rfind('\n').map_or(0, |at| at + 1);
        transcript[..start].to_owned()
    }

    #[test]
    fn a_successful_skill_call_loads_the_skill() {
        assert_eq!(load_of(SKILL_LOADED), Load::Loaded);
    }

    #[test]
    fn a_successful_read_of_the_installed_skill_md_loads_the_skill() {
        assert_eq!(load_of(READ_LOADED), Load::Loaded);
    }

    #[test]
    fn a_read_finds_a_skill_installed_under_a_hashed_directory() {
        let transcript = READ_LOADED.replace(
            "skills/fixture-greeting/SKILL.md",
            "skills/fixture-greeting-0123abcd/SKILL.md",
        );
        let hashed = skill("fixture-greeting", "fixture-greeting-0123abcd");

        assert_eq!(loads_in(&transcript, &[hashed]), [Load::Loaded]);
        assert_eq!(load_of(&transcript), Load::NotObserved);
    }

    #[test]
    fn a_read_through_dot_dot_reaches_the_same_file() {
        let transcript = READ_LOADED.replace(
            r#""file_path":".claude/skills"#,
            r#""file_path":"src/../.claude/skills"#,
        );

        assert_eq!(load_of(&transcript), Load::Loaded);
    }

    #[test]
    fn a_failed_skill_call_is_an_attempt() {
        let missing = skill("fixture-missing", "fixture-missing");

        assert_eq!(loads_in(SKILL_FAILED, &[missing]), [Load::Attempted]);
    }

    #[test]
    fn a_plugin_skill_is_matched_by_its_last_name() {
        let transcript = SKILL_LOADED.replace(
            r#""skill":"fixture-greeting""#,
            r#""skill":"fixture-plugin:fixture-greeting""#,
        );

        assert_eq!(load_of(&transcript), Load::Loaded);
    }

    #[test]
    fn a_skill_whose_name_only_ends_the_same_is_not_loaded() {
        let transcript = SKILL_LOADED.replace(
            r#""skill":"fixture-greeting""#,
            r#""skill":"other-fixture-greeting""#,
        );

        assert_eq!(load_of(&transcript), Load::NotObserved);
    }

    #[test]
    fn a_complete_session_without_a_load_is_not_observed() {
        assert_eq!(load_of(FINISHED), Load::NotObserved);
    }

    #[test]
    fn each_skill_is_decided_on_its_own() {
        let other = skill("fixture-other", "fixture-other");

        assert_eq!(
            loads_in(SKILL_LOADED, &[other, greeting()]),
            [Load::NotObserved, Load::Loaded]
        );
    }

    #[test]
    fn a_call_cut_off_before_its_result_is_unknown() {
        let transcript = cut_before(SKILL_LOADED, r#""type":"tool_result""#);

        assert_eq!(load_of(&transcript), Load::Unknown);
    }

    #[test]
    fn a_stream_without_its_result_event_is_unknown() {
        let transcript = cut_before(FINISHED, r#""type":"result""#);

        assert_eq!(load_of(&transcript), Load::Unknown);
    }

    #[test]
    fn a_load_stands_when_the_stream_is_then_cut_off() {
        let transcript = format!(
            "{}{{\"type\":\"assist",
            cut_before(SKILL_LOADED, r#""type":"result""#)
        );

        assert_eq!(load_of(&transcript), Load::Loaded);
    }

    #[test]
    fn an_unreadable_line_makes_an_unloaded_skill_unknown() {
        let transcript = format!("not json\n{FINISHED}");

        assert_eq!(load_of(&transcript), Load::Unknown);
    }

    #[test]
    fn a_tool_call_that_does_not_parse_is_unknown() {
        let transcript = SKILL_LOADED.replace(r#""id":"toolu_"#, r#""call":"toolu_"#);

        assert_eq!(load_of(&transcript), Load::Unknown);
    }

    #[test]
    fn another_sessions_skill_load_is_not_this_sessions() {
        let transcript = format!(
            "{}{}\n{}\n{}",
            cut_before(FINISHED, r#""type":"result""#),
            foreign(SKILL_LOADED, r#""type":"tool_use""#),
            foreign(SKILL_LOADED, r#""type":"tool_result""#),
            &FINISHED[FINISHED.find(r#"{"type":"result""#).unwrap()..],
        );

        assert_eq!(load_of(&transcript), Load::Unknown);
    }

    #[test]
    fn another_sessions_result_does_not_complete_this_one() {
        let transcript = format!(
            "{}{}\n",
            cut_before(FINISHED, r#""type":"result""#),
            foreign(FINISHED, r#""type":"result""#),
        );

        assert_eq!(load_of(&transcript), Load::Unknown);
    }

    #[test]
    fn a_tool_with_an_unfamiliar_input_is_ignored() {
        let call = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_future","name":"FutureTool","input":[1,"two"]}]},"session_id":"00000000-0000-4000-8000-000000000000"}"#;
        let answer = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_future","content":"ok"}]},"session_id":"00000000-0000-4000-8000-000000000000"}"#;
        let transcript = format!("{call}\n{answer}\n{FINISHED}");

        assert_eq!(load_of(&transcript), Load::NotObserved);
    }

    #[test]
    fn a_tool_result_without_a_session_answers_this_sessions_call() {
        assert_eq!(load_of(&without_user_sessions(SKILL_LOADED)), Load::Loaded);
    }

    #[test]
    fn a_skill_call_without_a_session_is_unknown() {
        let session = format!(r#","session_id":"{}""#, recorded_session());
        let transcript: String = SKILL_LOADED
            .lines()
            .map(|line| {
                if line.contains(r#""type":"tool_use""#) {
                    format!("{}\n", line.replace(&session, ""))
                } else {
                    format!("{line}\n")
                }
            })
            .collect();

        assert_eq!(load_of(&transcript), Load::Unknown);
    }

    #[test]
    fn a_message_that_cannot_be_decoded_makes_an_unloaded_skill_unknown() {
        let reshaped = FINISHED.replace(
            r#""message":{"model":"claude-haiku-5-5","role":"assistant""#,
            r#""message":"a new shape","old":{"model":"claude-haiku-5-5","role":"assistant""#,
        );

        assert_eq!(load_of(&reshaped), Load::Unknown);
    }

    #[test]
    fn a_load_stands_when_a_later_message_cannot_be_decoded() {
        let result = SKILL_LOADED.find(r#"{"type":"result""#).unwrap();
        let reshaped = r#"{"type":"assistant","message":"a new shape","session_id":"00000000-0000-4000-8000-000000000000"}"#;
        let transcript = format!(
            "{}{reshaped}\n{}",
            &SKILL_LOADED[..result],
            &SKILL_LOADED[result..]
        );

        assert_eq!(load_of(&transcript), Load::Loaded);
    }

    #[test]
    fn an_unknown_content_block_is_ignored() {
        let block = r#"{"type":"assistant","message":{"content":[{"type":"future_block","data":[1]}]},"session_id":"00000000-0000-4000-8000-000000000000"}"#;
        let transcript = format!("{block}\n{FINISHED}");

        assert_eq!(load_of(&transcript), Load::NotObserved);
    }

    #[test]
    fn a_subagents_skill_load_counts() {
        let transcript = SKILL_LOADED.replace(
            r#""parent_tool_use_id":null"#,
            r#""parent_tool_use_id":"toolu_parent""#,
        );
        assert_ne!(transcript, SKILL_LOADED);

        assert_eq!(load_of(&transcript), Load::Loaded);
    }

    #[test]
    fn a_skill_call_without_its_name_is_unknown() {
        let transcript = SKILL_LOADED.replace(r#""skill":"fixture-greeting""#, r#""x":1"#);

        assert_eq!(load_of(&transcript), Load::Unknown);
    }
}
