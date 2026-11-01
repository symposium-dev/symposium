//! Tool calls in a Claude Code transcript, each paired with its result by tool use ID.
//!
//! What a call is evidence of is the [`Matcher`]'s; this module only says whether the
//! transcript showed it succeed, fail, or not settle. Without full evidence, a target not
//! seen succeeding is unknown; a success, once seen, stands.

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;
use serde_json::value::RawValue;

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

    /// What a call to the tool `name` with `input` is evidence of.
    fn target(&self, name: &str, input: Value) -> Target;
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
        /// Left to the matcher, so another tool's input, in whatever shape, cannot make
        /// the stream unreadable.
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

#[derive(Debug, Default, Clone, Copy)]
struct Seen {
    succeeded: bool,
    failed: bool,
}

/// What a transcript has shown so far about the matcher's targets.
pub(super) struct Calls<M> {
    matcher: M,
    seen: Vec<Seen>,
    /// Calls awaiting their result, by tool use ID.
    pending: HashMap<String, usize>,
    /// Whether some record that could hide an outcome was unreadable or not this
    /// session's.
    damaged: bool,
    finished: bool,
}

impl<M: Matcher> Calls<M> {
    pub(super) fn new(matcher: M) -> Self {
        Self {
            seen: vec![Seen::default(); matcher.targets()],
            matcher,
            pending: HashMap::new(),
            damaged: false,
            finished: false,
        }
    }

    pub(super) fn matcher_mut(&mut self) -> &mut M {
        &mut self.matcher
    }

    /// Reads a message's `payload`. One that cannot be decoded could hide a call.
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
            Block::ToolUse { id, name, input } => match self.matcher.target(&name, input) {
                Target::Of(index) if scope == Scope::Session => {
                    self.pending.insert(id, index);
                }
                // A call no session vouches for could hide an outcome.
                Target::Of(_) | Target::Unreadable => self.damaged = true,
                Target::None => {}
            },
            Block::ToolResult {
                tool_use_id,
                is_error,
            } => {
                if let Some(index) = self.pending.remove(&tool_use_id) {
                    let seen = &mut self.seen[index];
                    if is_error {
                        seen.failed = true;
                    } else {
                        seen.succeeded = true;
                    }
                }
            }
            Block::Other => {}
        }
    }

    pub(super) fn outcomes(self) -> Vec<Outcome> {
        let incomplete = self.damaged || !self.finished;
        let unresolved: Vec<usize> = self.pending.into_values().collect();
        self.seen
            .iter()
            .enumerate()
            .map(|(index, seen)| {
                if seen.succeeded {
                    Outcome::Succeeded
                } else if incomplete || unresolved.contains(&index) {
                    Outcome::Unknown
                } else if seen.failed {
                    Outcome::Failed
                } else {
                    Outcome::NotObserved
                }
            })
            .collect()
    }
}
