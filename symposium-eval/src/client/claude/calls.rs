//! Tool calls in a Claude Code transcript, each paired with its result by tool use ID.
//!
//! Each message is decoded once, into [`Calls`]: every call with its result, and whether
//! anything could hide one. What a call is evidence of is decided afterwards, by a
//! [`Matcher`] over that record. Without full evidence, a target not seen succeeding is
//! unknown; a success, once seen, stands.

use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::path::Path;
use std::rc::Rc;

use serde::Deserialize;
use serde_json::value::RawValue;

use crate::client::ToolResult;

/// Whose a message is, as far as the transcript says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Scope {
    /// It names this session.
    Session,
    /// It names no session, which the SDK allows for user messages. Its tool results can
    /// still answer this session's calls, which only a message of this session registers,
    /// but its calls count for nothing.
    Unscoped,
}

/// Says which of its targets, if any, a tool call is evidence of.
pub(super) trait Matcher {
    /// How many targets there are; [`Target::Of`] indexes them.
    fn targets(&self) -> usize;

    /// What `call` is evidence of.
    fn target(&self, call: &ToolCall) -> Target;
}

pub(super) enum Target {
    Of(usize),
    /// A call to a relevant tool without the input that says what it is for.
    Unreadable,
    None,
}

/// What the transcript showed for one target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Outcome {
    Succeeded,
    /// Only calls that returned an error, which is also how a denial arrives.
    Failed,
    NotObserved,
    Unknown,
}

/// One tool call as the transcript shows it.
#[derive(Debug)]
pub(super) struct ToolCall {
    pub(super) name: String,
    /// Left to the matchers to decode, so another tool's input, in whatever shape,
    /// cannot make the stream unreadable.
    pub(super) input: Box<RawValue>,
    /// Where the session was running when it made the call, which a relative path in
    /// `input` is resolved against.
    pub(super) cwd: Rc<Path>,
    pub(super) scope: Scope,
    pub(super) result: ToolResult,
}

/// A message, borrowed from its line. Its content is plain text or a list of blocks;
/// only blocks hold tool calls and results.
#[derive(Debug, Deserialize)]
struct Message<'a> {
    #[serde(borrow)]
    content: &'a RawValue,
}

/// A content block, read flat rather than as a tagged enum, which would buffer every
/// block whole: a tool result's body is skipped unread. Fields are kept raw, so a block
/// of another kind cannot fail on them.
#[derive(Debug, Deserialize)]
struct RawBlock<'a> {
    #[serde(rename = "type", borrow)]
    kind: Cow<'a, str>,
    #[serde(borrow)]
    id: Option<&'a RawValue>,
    #[serde(borrow)]
    name: Option<&'a RawValue>,
    #[serde(borrow)]
    input: Option<&'a RawValue>,
    #[serde(borrow)]
    tool_use_id: Option<&'a RawValue>,
    /// Omitted means success; `null` is present, so it reads as unknown.
    #[serde(default, borrow, deserialize_with = "present")]
    is_error: Option<&'a RawValue>,
}

/// A field that is present, even as `null`, which `Option` alone would read as absent.
fn present<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<&'de RawValue>, D::Error> {
    <&RawValue>::deserialize(deserializer).map(Some)
}

/// The blocks that matter, decoded from a [`RawBlock`].
enum Block<'a> {
    ToolUse {
        id: String,
        name: String,
        input: &'a RawValue,
    },
    ToolResult {
        tool_use_id: String,
        is_error: bool,
    },
    Other,
}

impl<'a> RawBlock<'a> {
    /// The block, or `None` when a call or result lacks a field it needs.
    fn decode(&self) -> Option<Block<'a>> {
        let text = |raw: Option<&RawValue>| serde_json::from_str::<String>(raw?.get()).ok();
        Some(match &*self.kind {
            "tool_use" => Block::ToolUse {
                id: text(self.id)?,
                name: text(self.name)?,
                input: self.input?,
            },
            "tool_result" => Block::ToolResult {
                tool_use_id: text(self.tool_use_id)?,
                is_error: match self.is_error {
                    None => false,
                    Some(raw) => serde_json::from_str(raw.get()).ok()?,
                },
            },
            _ => Block::Other,
        })
    }
}

/// The blocks of `payload`, `Ok(None)` for plain text, and an error for anything else.
fn blocks(payload: &RawValue) -> Result<Option<Vec<Block<'_>>>, ()> {
    let message: Message<'_> = serde_json::from_str(payload.get()).map_err(drop)?;
    if message.content.get().starts_with('"') {
        return Ok(None);
    }
    let raw: Vec<RawBlock<'_>> = serde_json::from_str(message.content.get()).map_err(drop)?;
    raw.iter()
        .map(RawBlock::decode)
        .collect::<Option<_>>()
        .map(Some)
        .ok_or(())
}

/// Larger than the tool calls of any session; past it, calls are no longer kept.
const MAX_KEPT: usize = 64 * 1024 * 1024;

/// What a tool use ID of this session pairs with.
#[derive(Debug)]
enum Id {
    /// The call at this index in `calls`, awaiting its result.
    Open(usize),
    Answered(usize),
    /// Used by two calls, or answered twice, so no result can be attributed.
    Ambiguous,
}

impl Id {
    /// Makes the ID ambiguous, taking back any result its call was given.
    fn lose(&mut self, calls: &mut [ToolCall]) {
        if let Self::Open(earlier) | Self::Answered(earlier) = *self {
            calls[earlier].result = ToolResult::Missing;
        }
        *self = Self::Ambiguous;
    }
}

/// Every tool call a transcript showed, with its result.
#[derive(Debug)]
pub(super) struct Calls {
    calls: Vec<ToolCall>,
    ids: HashMap<String, Id>,
    /// Where the session is running, as far as the transcript has said.
    cwd: Rc<Path>,
    /// The bytes of the messages whose calls are kept, at most `limit`.
    kept: usize,
    limit: usize,
    /// Whether some record that could hide a call was unreadable, not this session's,
    /// or too much to keep.
    damaged: bool,
    finished: bool,
}

impl Calls {
    /// Calls of a session that starts in `project`.
    pub(super) fn new(project: &Path) -> Self {
        Self::keeping(project, MAX_KEPT)
    }

    fn keeping(project: &Path, limit: usize) -> Self {
        Self {
            calls: Vec::new(),
            ids: HashMap::new(),
            cwd: project.into(),
            kept: 0,
            limit,
            damaged: false,
            finished: false,
        }
    }

    /// Reads a message's `payload`. One that cannot be decoded could hide a call.
    pub(super) fn message(&mut self, payload: &RawValue, scope: Scope) {
        let blocks = match blocks(payload) {
            Ok(Some(blocks)) => blocks,
            Ok(None) => return,
            Err(()) => {
                self.damaged = true;
                return;
            }
        };
        let has_calls = blocks
            .iter()
            .any(|block| matches!(block, Block::ToolUse { .. }));
        let keep = !has_calls || self.kept + payload.get().len() <= self.limit;
        if has_calls && keep {
            self.kept += payload.get().len();
        }
        if !keep {
            self.damaged = true;
        }
        for block in blocks {
            match block {
                Block::ToolUse { id, name, input } if keep => {
                    self.call(id, name, input.to_owned(), scope);
                }
                // Not kept, but its ID still counts, so a result cannot reach another
                // call that reused it.
                Block::ToolUse { id, .. } if scope == Scope::Session => {
                    let slot = self.ids.entry(id).or_insert(Id::Ambiguous);
                    slot.lose(&mut self.calls);
                }
                Block::ToolResult {
                    tool_use_id,
                    is_error,
                } => self.result(&tool_use_id, is_error),
                Block::ToolUse { .. } | Block::Other => {}
            }
        }
    }

    /// Claude Code reported running in `cwd`, for the calls that follow.
    pub(super) fn ran_in(&mut self, cwd: &Path) {
        self.cwd = cwd.into();
    }

    /// This session's `result` arrived.
    pub(super) fn finish(&mut self) {
        self.finished = true;
    }

    /// Something in the transcript could hide a call.
    pub(super) fn damage(&mut self) {
        self.damaged = true;
    }

    fn call(&mut self, id: String, name: String, input: Box<RawValue>, scope: Scope) {
        let index = self.calls.len();
        self.calls.push(ToolCall {
            name,
            input,
            cwd: Rc::clone(&self.cwd),
            scope,
            result: ToolResult::Missing,
        });
        if scope != Scope::Session {
            return;
        }
        match self.ids.entry(id) {
            Entry::Vacant(slot) => {
                slot.insert(Id::Open(index));
            }
            Entry::Occupied(mut slot) => slot.get_mut().lose(&mut self.calls),
        }
    }

    fn result(&mut self, id: &str, is_error: bool) {
        let Some(slot) = self.ids.get_mut(id) else {
            return;
        };
        match *slot {
            Id::Open(index) => {
                self.calls[index].result = if is_error {
                    ToolResult::Failed
                } else {
                    ToolResult::Succeeded
                };
                *slot = Id::Answered(index);
            }
            Id::Answered(_) => slot.lose(&mut self.calls),
            Id::Ambiguous => {}
        }
    }

    /// Every call, in transcript order.
    pub(super) fn iter(&self) -> impl Iterator<Item = &ToolCall> {
        self.calls.iter()
    }

    /// Whether nothing could have hidden a call: no damage, and the session ended.
    pub(super) fn complete(&self) -> bool {
        self.finished && !self.damaged
    }

    /// What the calls show for each of `matcher`'s targets.
    pub(super) fn outcomes(&self, matcher: &impl Matcher) -> Vec<Outcome> {
        let mut succeeded = vec![false; matcher.targets()];
        let mut failed = vec![false; matcher.targets()];
        let mut unresolved = vec![false; matcher.targets()];
        let mut damaged = self.damaged;
        for call in &self.calls {
            match (matcher.target(call), call.scope) {
                (Target::Of(index), Scope::Session) => {
                    let seen = match call.result {
                        ToolResult::Succeeded => &mut succeeded,
                        ToolResult::Failed => &mut failed,
                        ToolResult::Missing => &mut unresolved,
                    };
                    seen[index] = true;
                }
                // A call no session vouches for could hide an outcome.
                (Target::Of(_), Scope::Unscoped) | (Target::Unreadable, _) => damaged = true,
                (Target::None, _) => {}
            }
        }
        let incomplete = damaged || !self.finished;
        (0..matcher.targets())
            .map(|index| {
                if succeeded[index] {
                    Outcome::Succeeded
                } else if incomplete || unresolved[index] {
                    Outcome::Unknown
                } else if failed[index] {
                    Outcome::Failed
                } else {
                    Outcome::NotObserved
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::value::to_raw_value;
    use serde_json::{Value, json};

    use super::*;

    /// Targets a call by its tool's name: `A` and `B`; `Bad` lacks its input.
    struct ByName;

    impl Matcher for ByName {
        fn targets(&self) -> usize {
            2
        }

        fn target(&self, call: &ToolCall) -> Target {
            match call.name.as_str() {
                "A" => Target::Of(0),
                "B" => Target::Of(1),
                "Bad" => Target::Unreadable,
                _ => Target::None,
            }
        }
    }

    fn call(name: &str, id: &str) -> Value {
        json!({ "type": "tool_use", "id": id, "name": name, "input": {} })
    }

    fn result(id: &str, is_error: bool) -> Value {
        json!({ "type": "tool_result", "tool_use_id": id, "is_error": is_error })
    }

    fn message(calls: &mut Calls, blocks: &[Value], scope: Scope) {
        let payload = to_raw_value(&json!({ "content": blocks })).unwrap();
        calls.message(&payload, scope);
    }

    /// The outcomes of `blocks`, sent as one message each, then finished.
    fn outcomes(blocks: &[Value]) -> Vec<Outcome> {
        let mut calls = Calls::new(Path::new("/project"));
        for block in blocks {
            message(&mut calls, std::slice::from_ref(block), Scope::Session);
        }
        calls.finish();
        calls.outcomes(&ByName)
    }

    #[test]
    fn each_target_follows_its_calls_results() {
        let blocks = [
            call("A", "1"),
            result("1", false),
            call("B", "2"),
            result("2", true),
        ];

        assert_eq!(outcomes(&blocks), [Outcome::Succeeded, Outcome::Failed]);
        assert_eq!(outcomes(&[]), [Outcome::NotObserved, Outcome::NotObserved]);
    }

    #[test]
    fn a_call_without_its_result_is_unknown() {
        assert_eq!(outcomes(&[call("A", "1")])[0], Outcome::Unknown);
    }

    #[test]
    fn an_explicit_null_error_flag_does_not_claim_success() {
        use crate::client::claude::fixtures::{SKILL_FAILED, recorded_session, replaced};
        use crate::client::claude::{transcript::read_text, version};
        use crate::client::{InstalledSkill, Load};
        use crate::task::SkillName;

        let skill = InstalledSkill {
            name: SkillName::new("fixture-missing").unwrap(),
            skill_md: "/trial/project/.claude/skills/fixture-missing/SKILL.md".into(),
        };
        let load = |text| {
            read_text(
                text,
                Path::new("/trial/project"),
                recorded_session(),
                version::MINIMUM,
                std::slice::from_ref(&skill),
            )
            .loads[0]
        };
        assert_eq!(load(SKILL_FAILED), Load::Attempted);
        let malformed = replaced(SKILL_FAILED, r#""is_error":true"#, r#""is_error":null"#);

        assert_eq!(load(&malformed), Load::Unknown);
    }

    #[test]
    fn a_transcript_without_its_ending_cannot_show_a_target_unobserved() {
        let mut calls = Calls::new(Path::new("/project"));
        message(
            &mut calls,
            &[call("A", "1"), result("1", false)],
            Scope::Session,
        );

        assert_eq!(
            calls.outcomes(&ByName),
            [Outcome::Succeeded, Outcome::Unknown]
        );
    }

    #[test]
    fn two_calls_with_one_id_get_no_result() {
        let blocks = [call("A", "1"), call("B", "1"), result("1", false)];

        assert_eq!(outcomes(&blocks), [Outcome::Unknown, Outcome::Unknown]);
    }

    #[test]
    fn a_call_answered_twice_gets_no_result() {
        let blocks = [call("A", "1"), result("1", false), result("1", true)];

        assert_eq!(outcomes(&blocks)[0], Outcome::Unknown);
    }

    #[test]
    fn an_unscoped_result_answers_a_call_of_this_session() {
        let mut calls = Calls::new(Path::new("/project"));
        message(&mut calls, &[call("A", "1")], Scope::Session);
        message(&mut calls, &[result("1", false)], Scope::Unscoped);
        calls.finish();

        assert_eq!(calls.outcomes(&ByName)[0], Outcome::Succeeded);
    }

    #[test]
    fn an_unscoped_or_unreadable_call_could_hide_an_outcome() {
        let mut unscoped = Calls::new(Path::new("/project"));
        message(
            &mut unscoped,
            &[call("A", "1"), result("1", false)],
            Scope::Unscoped,
        );
        unscoped.finish();

        assert_eq!(
            unscoped.outcomes(&ByName),
            [Outcome::Unknown, Outcome::Unknown]
        );
        assert_eq!(
            outcomes(&[call("Bad", "1")]),
            [Outcome::Unknown, Outcome::Unknown]
        );
    }

    #[test]
    fn a_success_stands_beside_damage() {
        let mut calls = Calls::new(Path::new("/project"));
        message(
            &mut calls,
            &[call("A", "1"), result("1", false)],
            Scope::Session,
        );
        calls.damage();
        calls.finish();

        assert_eq!(
            calls.outcomes(&ByName),
            [Outcome::Succeeded, Outcome::Unknown]
        );
    }

    #[test]
    fn a_dropped_call_still_makes_its_id_ambiguous() {
        let first = to_raw_value(&json!({ "content": [call("A", "1")] })).unwrap();
        let mut calls = Calls::keeping(Path::new("/project"), first.get().len());
        calls.message(&first, Scope::Session);
        message(&mut calls, &[call("B", "1")], Scope::Session);
        message(&mut calls, &[result("1", false)], Scope::Session);
        calls.finish();

        assert_eq!(calls.outcomes(&ByName)[0], Outcome::Unknown);
    }

    #[test]
    fn each_call_keeps_where_the_session_ran_when_it_was_made() {
        let mut calls = Calls::new(Path::new("/project"));
        message(&mut calls, &[call("A", "1")], Scope::Session);
        calls.ran_in(Path::new("/elsewhere"));
        message(&mut calls, &[call("A", "2")], Scope::Session);

        let cwds: Vec<&Path> = calls.calls.iter().map(|call| &*call.cwd).collect();
        assert_eq!(cwds, [Path::new("/project"), Path::new("/elsewhere")]);
    }

    #[test]
    fn a_block_of_another_kind_is_skipped_whatever_its_fields() {
        let other = json!({ "type": "text", "id": 1, "name": [], "is_error": "no" });

        let blocks = [other, call("A", "1"), result("1", false)];

        assert_eq!(outcomes(&blocks)[0], Outcome::Succeeded);
    }

    #[test]
    fn plain_text_content_holds_no_calls() {
        let mut calls = Calls::new(Path::new("/project"));
        let text = to_raw_value(&json!({ "content": "hello" })).unwrap();
        calls.message(&text, Scope::Session);
        calls.finish();

        assert_eq!(
            calls.outcomes(&ByName),
            [Outcome::NotObserved, Outcome::NotObserved]
        );
    }

    #[test]
    fn a_call_without_its_input_is_damage() {
        let call = json!({ "type": "tool_use", "id": "1", "name": "A" });

        assert_eq!(outcomes(&[call]), [Outcome::Unknown, Outcome::Unknown]);
    }

    #[test]
    fn a_message_that_cannot_be_decoded_is_damage() {
        let mut calls = Calls::new(Path::new("/project"));
        calls.message(
            &to_raw_value(&json!({ "content": 1 })).unwrap(),
            Scope::Session,
        );
        calls.finish();

        assert_eq!(
            calls.outcomes(&ByName),
            [Outcome::Unknown, Outcome::Unknown]
        );
    }

    #[test]
    fn calls_past_the_limit_are_not_kept() {
        let first = to_raw_value(&json!({ "content": [call("A", "1")] })).unwrap();
        let mut calls = Calls::keeping(Path::new("/project"), first.get().len());
        calls.message(&first, Scope::Session);
        message(
            &mut calls,
            &[call("B", "2"), result("1", false)],
            Scope::Session,
        );
        message(&mut calls, &[result("2", false)], Scope::Session);
        calls.finish();

        // The result still reaches the kept call; the call past the limit is lost.
        assert_eq!(
            calls.outcomes(&ByName),
            [Outcome::Succeeded, Outcome::Unknown]
        );
    }
}
