//! Which skills a Claude Code transcript shows reaching the agent (RFD reference,
//! "Skills used").
//!
//! A skill is loaded by a `Skill` call or a `Read` of its installed `SKILL.md` whose
//! result is not an error; an error result is an attempt. `cat SKILL.md` is not
//! detected. Subagent calls count. Pairing calls with results is [`super::calls`]'.

use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use super::calls::{Matcher, Outcome, Target, ToolCall};
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
    /// Each skill's `SKILL.md`, in every form a `Read` could name it by.
    files: Vec<Vec<PathBuf>>,
}

impl<'a> Skills<'a> {
    pub(super) fn new(skills: &'a [InstalledSkill]) -> Self {
        Self {
            skills,
            files: skills.iter().map(|skill| forms(&skill.skill_md)).collect(),
        }
    }

    fn position(&self, matches: impl Fn(usize) -> bool) -> Option<usize> {
        (0..self.skills.len()).find(|&index| matches(index))
    }
}

impl Matcher for Skills<'_> {
    fn targets(&self) -> usize {
        self.skills.len()
    }

    fn target(&self, call: &ToolCall) -> Target {
        let found = match call.name.as_str() {
            "Skill" => {
                let Ok(SkillInput { skill: called }) = SkillInput::deserialize(&*call.input) else {
                    return Target::Unreadable;
                };
                // A plugin's skill is named `<plugin>:<skill>`.
                let called = called.rsplit(':').next();
                self.position(|index| called == Some(self.skills[index].name.as_str()))
            }
            "Read" => {
                let Ok(ReadInput { file_path }) = ReadInput::deserialize(&*call.input) else {
                    return Target::Unreadable;
                };
                let read = forms(&call.cwd.join(file_path));
                self.position(|index| self.files[index].iter().any(|file| read.contains(file)))
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

/// The forms `path` can be named by: with `.` and `..` resolved by name, and, when it
/// exists, with links resolved too (macOS keeps temporary files under `/var`, a link to
/// `/private/var`).
fn forms(path: &Path) -> Vec<PathBuf> {
    let mut forms = vec![normalize(path)];
    if let Ok(resolved) = path.canonicalize()
        && !forms.contains(&resolved)
    {
        forms.push(resolved);
    }
    forms
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
    use crate::client::claude::fixtures::{
        FINISHED, READ_LOADED, SKILL_FAILED, SKILL_LOADED, as_session, before, before_line_with,
        editing, first, from, line_with, recorded_session, replaced, reshaped,
        without_user_sessions,
    };
    use crate::client::claude::transcript::read_text;
    use crate::client::claude::version;
    use crate::task::SkillName;

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
        as_session(line_with(transcript, marker), Uuid::new_v4())
    }

    /// The installed skills, as the recorded `Read` names them.
    const READ_PATH: &str = r#""file_path":".claude/skills"#;

    /// The recorded `Read`, naming the skills directory as `skills`.
    fn reading(skills: &str) -> String {
        replaced(READ_LOADED, READ_PATH, &format!(r#""file_path":"{skills}"#))
    }

    fn load_of(transcript: &str) -> Load {
        loads_in(transcript, &[greeting()])[0]
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
        let transcript = replaced(
            READ_LOADED,
            "skills/fixture-greeting/SKILL.md",
            "skills/fixture-greeting-0123abcd/SKILL.md",
        );
        let hashed = skill("fixture-greeting", "fixture-greeting-0123abcd");

        assert_eq!(loads_in(&transcript, &[hashed]), [Load::Loaded]);
        assert_eq!(load_of(&transcript), Load::NotObserved);
    }

    #[test]
    fn a_read_by_absolute_path_loads_the_skill() {
        let transcript = reading("/trial/project/.claude/skills");

        assert_eq!(load_of(&transcript), Load::Loaded);
    }

    #[test]
    fn a_read_through_dot_dot_reaches_the_same_file() {
        let transcript = reading("src/../.claude/skills");

        assert_eq!(load_of(&transcript), Load::Loaded);
    }

    #[test]
    fn a_relative_read_is_resolved_where_claude_code_ran() {
        let transcript = replaced(
            &reading("../.claude/skills"),
            r#""cwd":"/trial/project""#,
            r#""cwd":"/trial/project/src""#,
        );

        assert_eq!(load_of(&transcript), Load::Loaded);
    }

    #[test]
    fn each_relative_read_resolves_where_claude_code_ran_when_it_was_made() {
        let call = line_with(READ_LOADED, r#""name":"Read""#);
        let result = line_with(READ_LOADED, r#""type":"tool_result""#);
        let init = line_with(READ_LOADED, r#""subtype":"init""#);
        let moved = replaced(
            init,
            r#""cwd":"/trial/project""#,
            r#""cwd":"/trial/elsewhere""#,
        );
        let id = "toolu_01516sTbHU5oWta7zdRLvjd5";
        let (later_call, later_result) = (
            replaced(call, id, "toolu_later"),
            replaced(result, id, "toolu_later"),
        );
        // One `Read` before an `init` that moves the session, one after it.
        let mut lines = Vec::new();
        for line in READ_LOADED.lines() {
            if line == init {
                lines.extend([call, result, &moved, &later_call, &later_result]);
            } else if line != call && line != result {
                lines.push(line);
            }
        }
        let transcript = lines.join("\n");
        let elsewhere = InstalledSkill {
            skill_md: "/trial/elsewhere/.claude/skills/fixture-greeting/SKILL.md".into(),
            ..greeting()
        };

        assert_eq!(
            loads_in(&transcript, &[greeting(), elsewhere]),
            [Load::Loaded, Load::Loaded]
        );
    }

    #[test]
    fn a_read_from_elsewhere_does_not_load_the_skill() {
        let transcript = replaced(
            READ_LOADED,
            r#""cwd":"/trial/project""#,
            r#""cwd":"/trial/elsewhere""#,
        );

        assert_eq!(load_of(&transcript), Load::NotObserved);
    }

    #[cfg(unix)]
    #[test]
    fn a_read_through_a_linked_directory_reaches_the_same_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let real = dir.path().join("real");
        let installed = real.join(".claude/skills/fixture-greeting");
        std::fs::create_dir_all(&installed).unwrap();
        std::fs::write(installed.join("SKILL.md"), "").unwrap();
        let linked = dir.path().join("linked");
        std::os::unix::fs::symlink(&real, &linked).unwrap();
        let held = InstalledSkill {
            name: SkillName::new("fixture-greeting").unwrap(),
            skill_md: linked.join(".claude/skills/fixture-greeting/SKILL.md"),
        };
        let transcript = reading(&format!("{}/.claude/skills", real.display()));

        assert_eq!(loads_in(&transcript, &[held]), [Load::Loaded]);
    }

    #[test]
    fn a_last_line_cut_off_mid_write_makes_an_unloaded_skill_unknown() {
        let transcript = format!("{FINISHED}{{\"type\":\"assist");

        assert_eq!(load_of(&transcript), Load::Unknown);
    }

    #[test]
    fn a_failed_skill_call_is_an_attempt() {
        let missing = skill("fixture-missing", "fixture-missing");

        assert_eq!(loads_in(SKILL_FAILED, &[missing]), [Load::Attempted]);
    }

    #[test]
    fn a_plugin_skill_is_matched_by_its_last_name() {
        let transcript = replaced(
            SKILL_LOADED,
            r#""skill":"fixture-greeting""#,
            r#""skill":"fixture-plugin:fixture-greeting""#,
        );

        assert_eq!(load_of(&transcript), Load::Loaded);
    }

    #[test]
    fn a_skill_whose_name_only_ends_the_same_is_not_loaded() {
        let transcript = replaced(
            SKILL_LOADED,
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
        let transcript = before_line_with(SKILL_LOADED, r#""type":"tool_result""#);

        assert_eq!(load_of(&transcript), Load::Unknown);
    }

    #[test]
    fn a_stream_without_its_result_event_is_unknown() {
        let transcript = before(FINISHED, "result");

        assert_eq!(load_of(&transcript), Load::Unknown);
    }

    #[test]
    fn a_load_stands_when_the_stream_is_then_cut_off() {
        let transcript = format!("{}{{\"type\":\"assist", before(SKILL_LOADED, "result"));

        assert_eq!(load_of(&transcript), Load::Loaded);
    }

    #[test]
    fn an_unreadable_line_makes_an_unloaded_skill_unknown() {
        let transcript = format!("not json\n{FINISHED}");

        assert_eq!(load_of(&transcript), Load::Unknown);
    }

    #[test]
    fn a_tool_call_that_does_not_parse_is_unknown() {
        let transcript = replaced(SKILL_LOADED, r#""id":"toolu_"#, r#""call":"toolu_"#);

        assert_eq!(load_of(&transcript), Load::Unknown);
    }

    #[test]
    fn another_sessions_skill_load_is_not_this_sessions() {
        let transcript = format!(
            "{}{}\n{}\n{}",
            before(FINISHED, "result"),
            foreign(SKILL_LOADED, r#""type":"tool_use""#),
            foreign(SKILL_LOADED, r#""type":"tool_result""#),
            from(FINISHED, "result"),
        );

        assert_eq!(load_of(&transcript), Load::Unknown);
    }

    #[test]
    fn another_sessions_result_does_not_complete_this_one() {
        let transcript = format!(
            "{}{}\n",
            before(FINISHED, "result"),
            as_session(first(FINISHED, "result"), Uuid::new_v4()),
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
        let transcript = editing(
            SKILL_LOADED,
            |line| line.contains(r#""type":"tool_use""#),
            |event| {
                event.remove("session_id").unwrap();
            },
        );

        assert_eq!(load_of(&transcript), Load::Unknown);
    }

    #[test]
    fn a_message_that_cannot_be_decoded_makes_an_unloaded_skill_unknown() {
        assert_eq!(load_of(&reshaped(FINISHED, "assistant")), Load::Unknown);
    }

    #[test]
    fn a_load_stands_when_a_later_message_cannot_be_decoded() {
        let reshaped = r#"{"type":"assistant","message":"a new shape","session_id":"00000000-0000-4000-8000-000000000000"}"#;
        let transcript = format!(
            "{}{reshaped}\n{}",
            before(SKILL_LOADED, "result"),
            from(SKILL_LOADED, "result")
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
        let transcript = replaced(
            SKILL_LOADED,
            r#""parent_tool_use_id":null"#,
            r#""parent_tool_use_id":"toolu_parent""#,
        );

        assert_eq!(load_of(&transcript), Load::Loaded);
    }

    #[test]
    fn a_skill_call_without_its_name_is_unknown() {
        let transcript = replaced(SKILL_LOADED, r#""skill":"fixture-greeting""#, r#""x":1"#);

        assert_eq!(load_of(&transcript), Load::Unknown);
    }
}
