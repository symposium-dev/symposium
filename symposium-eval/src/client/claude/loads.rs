//! Which skills a Claude Code transcript shows reaching the agent (RFD reference,
//! "Skills used"; research notes, Step 4, load evidence).
//!
//! Two routes count: a `Skill` call naming the skill, or a `Read` of its installed
//! `SKILL.md`, each paired with its result by tool use ID. A result that is not an error
//! loads the skill; an error, which is also how a denial arrives, is an attempt. A shell
//! command such as `cat SKILL.md` is not detected: reading arbitrary commands reliably is
//! outside the adapter. Calls made inside a subagent count like any other, since the
//! skill's content still reached an agent working on the task.
//!
//! Only this session's events count. Another session's message or `result` gives no
//! evidence, and like a line that does not parse, it could hide an outcome. A skill not
//! yet loaded is then unknown, as it is after a call without its result or with no
//! `result` event at the end. Once a skill is loaded, nothing later undoes that, so a
//! timeout can still report a load.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;
use serde_json::value::RawValue;

use crate::client::{InstalledSkill, Load};

/// Whose a message is, as far as the transcript says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Scope {
    /// It names this session.
    Session,
    /// It names no session. Its tool results can still answer this session's calls,
    /// which only a message of this session registers, but its calls count for nothing.
    Unscoped,
}

#[derive(Debug, Deserialize)]
struct Message {
    content: Content,
}

/// A prompt's content can be plain text; only blocks hold tool calls and results. Text
/// must be a string, so a list of blocks that does not parse is not taken for text.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Content {
    Blocks(Vec<Block>),
    Text(String),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Block {
    ToolUse {
        id: String,
        name: String,
        /// Read only for the tools that can load a skill, so another tool's input, in
        /// whatever shape, cannot make the stream unreadable.
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        #[serde(default)]
        is_error: bool,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
struct SkillInput {
    skill: String,
}

#[derive(Debug, Deserialize)]
struct ReadInput {
    file_path: String,
}

/// What the transcript showed for one skill.
#[derive(Debug, Default, Clone, Copy)]
struct Seen {
    loaded: bool,
    attempted: bool,
}

/// What a transcript has shown so far about `skills`.
pub(super) struct Observer<'a> {
    skills: &'a [InstalledSkill],
    /// Where Claude Code ran, which a relative `Read` path is resolved against.
    project: &'a Path,
    seen: Vec<Seen>,
    /// Calls that may load a skill, by tool use ID, awaiting their result.
    pending: HashMap<String, usize>,
    /// Whether some record that could hide an outcome was unreadable or not this
    /// session's.
    damaged: bool,
    finished: bool,
}

impl<'a> Observer<'a> {
    pub(super) fn new(skills: &'a [InstalledSkill], project: &'a Path) -> Self {
        Self {
            skills,
            project,
            seen: vec![Seen::default(); skills.len()],
            pending: HashMap::new(),
            damaged: false,
            finished: false,
        }
    }

    /// Reads a message's `payload`. One this observer cannot decode could hide a load.
    pub(super) fn message(&mut self, payload: &RawValue, scope: Scope) {
        match serde_json::from_str::<Message>(payload.get()) {
            Ok(Message {
                content: Content::Blocks(blocks),
            }) => blocks
                .into_iter()
                .for_each(|block| self.block(block, scope)),
            Ok(_) => {}
            Err(_) => self.damaged = true,
        }
    }

    /// This session's `result` arrived.
    pub(super) fn finish(&mut self) {
        self.finished = true;
    }

    /// Something in the transcript could hide an outcome.
    pub(super) fn damage(&mut self) {
        self.damaged = true;
    }

    fn block(&mut self, block: Block, scope: Scope) {
        match block {
            Block::ToolUse { id, name, input } => match self.target(&name, input) {
                Target::Skill(index) if scope == Scope::Session => {
                    self.pending.insert(id, index);
                }
                // A call no session vouches for could hide a load.
                Target::Skill(_) => self.damaged = true,
                Target::Unreadable => self.damaged = true,
                Target::None => {}
            },
            Block::ToolResult {
                tool_use_id,
                is_error,
            } => {
                if let Some(index) = self.pending.remove(&tool_use_id) {
                    let seen = &mut self.seen[index];
                    if is_error {
                        seen.attempted = true;
                    } else {
                        seen.loaded = true;
                    }
                }
            }
            Block::Other => {}
        }
    }

    /// Which skill, if any, a call to the tool `name` with `input` would load.
    fn target(&self, name: &str, input: Value) -> Target {
        let found = match name {
            "Skill" => {
                let Ok(SkillInput { skill: called }) = serde_json::from_value(input) else {
                    return Target::Unreadable;
                };
                // A plugin's skill is named `<plugin>:<skill>`.
                self.position(|skill| called.rsplit(':').next() == Some(skill.name.as_str()))
            }
            "Read" => {
                let Ok(ReadInput { file_path }) = serde_json::from_value(input) else {
                    return Target::Unreadable;
                };
                let read = normalize(&self.project.join(file_path));
                self.position(|skill| normalize(&skill.skill_md) == read)
            }
            _ => None,
        };
        found.map_or(Target::None, Target::Skill)
    }

    fn position(&self, matches: impl Fn(&InstalledSkill) -> bool) -> Option<usize> {
        self.skills.iter().position(matches)
    }

    pub(super) fn loads(self) -> Vec<Load> {
        let unresolved: Vec<usize> = self.pending.values().copied().collect();
        let incomplete = self.damaged || !self.finished;
        self.seen
            .iter()
            .enumerate()
            .map(|(index, seen)| {
                if seen.loaded {
                    Load::Loaded
                } else if incomplete || unresolved.contains(&index) {
                    Load::Unknown
                } else if seen.attempted {
                    Load::Attempted
                } else {
                    Load::NotObserved
                }
            })
            .collect()
    }
}

enum Target {
    Skill(usize),
    /// A call to a loading tool without the input that says what it loads.
    Unreadable,
    None,
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
