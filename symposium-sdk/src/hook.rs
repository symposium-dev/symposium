//! SDK for writing symposium hook handlers in Rust.
//!
//! A symposium hook is a program that reads a JSON event on stdin and writes a
//! JSON response to stdout. This crate provides the types and a `run()` harness
//! so you can focus on the logic.
//!
//! # Example
//!
//! ```no_run
//! use std::process::ExitCode;
//! use symposium_sdk::hook::{HookHandler, PreToolUseInput, PreToolUseOutput, run};
//!
//! struct MyHook;
//!
//! impl HookHandler for MyHook {
//!     async fn pre_tool_use(&self, event: &PreToolUseInput) -> anyhow::Result<PreToolUseOutput> {
//!         if event.tool_name == "Bash" {
//!             Ok(PreToolUseOutput::context("Remember: prefer non-destructive commands"))
//!         } else {
//!             Ok(PreToolUseOutput::default())
//!         }
//!     }
//! }
//!
//! fn main() -> ExitCode {
//!     run(MyHook)
//! }
//! ```

pub use anyhow;

use serde::{Deserialize, Serialize};
use std::io::Read as _;
use std::process::ExitCode;

// ── Event types ─────────────────────────────────────────────────────────

/// Hook event types supported by Symposium.
#[non_exhaustive]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
pub enum HookEvent {
    #[cfg_attr(feature = "clap", value(name = "pre-tool-use"))]
    #[serde(rename = "PreToolUse")]
    PreToolUse,

    #[cfg_attr(feature = "clap", value(name = "post-tool-use"))]
    #[serde(rename = "PostToolUse")]
    PostToolUse,

    #[cfg_attr(feature = "clap", value(name = "user-prompt-submit"))]
    #[serde(rename = "UserPromptSubmit")]
    UserPromptSubmit,

    #[cfg_attr(feature = "clap", value(name = "session-start"))]
    #[serde(rename = "SessionStart")]
    SessionStart,
}

// ── Input types ─────────────────────────────────────────────────────────

/// Input event received on stdin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Input {
    PreToolUse(PreToolUseInput),
    PostToolUse(PostToolUseInput),
    UserPromptSubmit(UserPromptSubmitInput),
    SessionStart(SessionStartInput),
}

impl Input {
    /// Returns the event type for this input.
    pub fn event(&self) -> HookEvent {
        match self {
            Input::PreToolUse(_) => HookEvent::PreToolUse,
            Input::PostToolUse(_) => HookEvent::PostToolUse,
            Input::UserPromptSubmit(_) => HookEvent::UserPromptSubmit,
            Input::SessionStart(_) => HookEvent::SessionStart,
        }
    }

    /// Extract the working directory from any event variant.
    pub fn cwd(&self) -> Option<&str> {
        match self {
            Input::PreToolUse(p) => p.cwd.as_deref(),
            Input::PostToolUse(p) => p.cwd.as_deref(),
            Input::UserPromptSubmit(p) => p.cwd.as_deref(),
            Input::SessionStart(p) => p.cwd.as_deref(),
        }
    }

    /// Check whether this event matches the given matcher string.
    ///
    /// For tool events (`PreToolUse`, `PostToolUse`), the matcher is a regex
    /// tested against the tool name. For other events, all matchers match.
    /// The wildcard `"*"` matches everything.
    pub fn matches_matcher(&self, matcher: &str) -> bool {
        if matcher == "*" {
            return true;
        }

        let tool_name = match self {
            Input::PreToolUse(p) => &p.tool_name,
            Input::PostToolUse(p) => &p.tool_name,
            Input::UserPromptSubmit(_) | Input::SessionStart(_) => return true,
        };

        regex::Regex::new(matcher).is_ok_and(|re| re.is_match(tool_name))
    }
}

/// Input for a `PreToolUse` event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PreToolUseInput {
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: serde_json::Value,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

impl PreToolUseInput {
    pub fn new(
        tool_name: String,
        tool_input: serde_json::Value,
        session_id: Option<String>,
        cwd: Option<String>,
    ) -> Self {
        Self {
            tool_name,
            tool_input,
            session_id,
            cwd,
        }
    }
}

/// Input for a `PostToolUse` event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PostToolUseInput {
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: serde_json::Value,
    #[serde(default)]
    pub tool_response: serde_json::Value,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

impl PostToolUseInput {
    pub fn new(
        tool_name: String,
        tool_input: serde_json::Value,
        tool_response: serde_json::Value,
        session_id: Option<String>,
        cwd: Option<String>,
    ) -> Self {
        Self {
            tool_name,
            tool_input,
            tool_response,
            session_id,
            cwd,
        }
    }
}

/// Input for a `UserPromptSubmit` event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UserPromptSubmitInput {
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

impl UserPromptSubmitInput {
    pub fn new(prompt: String, session_id: Option<String>, cwd: Option<String>) -> Self {
        Self {
            prompt,
            session_id,
            cwd,
        }
    }
}

/// Input for a `SessionStart` event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SessionStartInput {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

impl SessionStartInput {
    pub fn new(session_id: Option<String>, cwd: Option<String>) -> Self {
        Self { session_id, cwd }
    }
}

// ── Output types ────────────────────────────────────────────────────────

/// Output event written to stdout.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Output {
    PreToolUse(PreToolUseOutput),
    PostToolUse(PostToolUseOutput),
    UserPromptSubmit(UserPromptSubmitOutput),
    SessionStart(SessionStartOutput),
}

impl Output {
    /// Create an empty output for the given event type.
    pub fn empty_for(event: HookEvent) -> Self {
        match event {
            HookEvent::PreToolUse => Output::PreToolUse(PreToolUseOutput::default()),
            HookEvent::PostToolUse => Output::PostToolUse(PostToolUseOutput::default()),
            HookEvent::UserPromptSubmit => {
                Output::UserPromptSubmit(UserPromptSubmitOutput::default())
            }
            HookEvent::SessionStart => Output::SessionStart(SessionStartOutput::default()),
        }
    }

    /// Create an output with additional context for the given event type.
    pub fn with_context(event: HookEvent, context: String) -> Self {
        match event {
            HookEvent::PreToolUse => Output::PreToolUse(PreToolUseOutput::context(context)),
            HookEvent::PostToolUse => Output::PostToolUse(PostToolUseOutput::context(context)),
            HookEvent::UserPromptSubmit => {
                Output::UserPromptSubmit(UserPromptSubmitOutput::context(context))
            }
            HookEvent::SessionStart => Output::SessionStart(SessionStartOutput::context(context)),
        }
    }

    /// Extract additional context from any variant.
    pub fn additional_context(&self) -> Option<&str> {
        match self {
            Output::PreToolUse(o) => o.additional_context.as_deref(),
            Output::PostToolUse(o) => o.additional_context.as_deref(),
            Output::UserPromptSubmit(o) => o.additional_context.as_deref(),
            Output::SessionStart(o) => o.additional_context.as_deref(),
        }
    }
}

/// Output for a `PreToolUse` event.
/// Decision for a `PreToolUse` hook.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    /// Allow the tool call to proceed (default).
    #[default]
    Allow,
    /// Block the tool call.
    Deny,
}

#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PreToolUseOutput {
    #[serde(default, skip_serializing_if = "Decision::is_allow")]
    pub decision: Decision,
    #[serde(
        rename = "additionalContext",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_context: Option<String>,
    #[serde(
        rename = "updatedInput",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub updated_input: Option<serde_json::Value>,
}

impl Decision {
    fn is_allow(&self) -> bool {
        *self == Decision::Allow
    }
}

impl PreToolUseOutput {
    /// Create an output with all fields specified.
    pub fn new(
        decision: Decision,
        additional_context: Option<String>,
        updated_input: Option<serde_json::Value>,
    ) -> Self {
        Self {
            decision,
            additional_context,
            updated_input,
        }
    }

    /// Create an output that injects additional context.
    pub fn context(text: impl Into<String>) -> Self {
        Self {
            additional_context: Some(text.into()),
            ..Default::default()
        }
    }

    /// Create an output that replaces the tool input.
    pub fn with_updated_input(input: serde_json::Value) -> Self {
        Self {
            updated_input: Some(input),
            ..Default::default()
        }
    }

    /// Deny the tool call with a reason.
    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            decision: Decision::Deny,
            additional_context: Some(reason.into()),
            ..Default::default()
        }
    }
}

/// Output for a `PostToolUse` event.
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PostToolUseOutput {
    #[serde(
        rename = "additionalContext",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_context: Option<String>,
}

impl PostToolUseOutput {
    /// Create an output with all fields specified.
    pub fn new(additional_context: Option<String>) -> Self {
        Self { additional_context }
    }

    /// Create an output that injects additional context.
    pub fn context(text: impl Into<String>) -> Self {
        Self::new(Some(text.into()))
    }
}

/// Output for a `UserPromptSubmit` event.
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserPromptSubmitOutput {
    #[serde(
        rename = "additionalContext",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_context: Option<String>,
}

impl UserPromptSubmitOutput {
    /// Create an output with all fields specified.
    pub fn new(additional_context: Option<String>) -> Self {
        Self { additional_context }
    }

    /// Create an output that injects additional context.
    pub fn context(text: impl Into<String>) -> Self {
        Self::new(Some(text.into()))
    }
}

/// Output for a `SessionStart` event.
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionStartOutput {
    #[serde(
        rename = "additionalContext",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_context: Option<String>,
}

impl SessionStartOutput {
    /// Create an output with all fields specified.
    pub fn new(additional_context: Option<String>) -> Self {
        Self { additional_context }
    }

    /// Create an output that injects additional context.
    pub fn context(text: impl Into<String>) -> Self {
        Self::new(Some(text.into()))
    }
}

// ── Handler trait ───────────────────────────────────────────────────────

/// Default dispatch logic: matches on the event variant and calls the
/// corresponding method on the handler, wrapping the result in the appropriate
/// `Output` variant.
pub async fn default_handle_event(
    handler: &(impl HookHandler + ?Sized),
    input: &Input,
) -> anyhow::Result<Output> {
    match input {
        Input::PreToolUse(event) => Ok(Output::PreToolUse(handler.pre_tool_use(event).await?)),
        Input::PostToolUse(event) => Ok(Output::PostToolUse(handler.post_tool_use(event).await?)),
        Input::UserPromptSubmit(event) => Ok(Output::UserPromptSubmit(
            handler.user_prompt_submit(event).await?,
        )),
        Input::SessionStart(event) => Ok(Output::SessionStart(handler.session_start(event).await?)),
    }
}

/// Trait for implementing a symposium hook handler.
///
/// Override the methods for the events you care about. The defaults return
/// the default (empty) output for each event type.
#[allow(async_fn_in_trait)] // Hook handlers run on a single-threaded runtime; Send is not needed.
pub trait HookHandler {
    /// Dispatch an input event to the appropriate handler method.
    ///
    /// The default implementation calls [`default_handle_event`], which matches
    /// on the event variant, calls the corresponding method, and wraps the
    /// result in the appropriate `Output` variant.
    async fn handle_event(&self, input: &Input) -> anyhow::Result<Output> {
        default_handle_event(self, input).await
    }

    /// Called before the agent invokes a tool.
    async fn pre_tool_use(&self, _event: &PreToolUseInput) -> anyhow::Result<PreToolUseOutput> {
        Ok(PreToolUseOutput::default())
    }

    /// Called after a tool completes.
    async fn post_tool_use(&self, _event: &PostToolUseInput) -> anyhow::Result<PostToolUseOutput> {
        Ok(PostToolUseOutput::default())
    }

    /// Called when the user submits a prompt.
    async fn user_prompt_submit(
        &self,
        _event: &UserPromptSubmitInput,
    ) -> anyhow::Result<UserPromptSubmitOutput> {
        Ok(UserPromptSubmitOutput::default())
    }

    /// Called when an agent session begins.
    async fn session_start(
        &self,
        _event: &SessionStartInput,
    ) -> anyhow::Result<SessionStartOutput> {
        Ok(SessionStartOutput::default())
    }
}

// ── Harness ─────────────────────────────────────────────────────────────

/// Run a hook handler. Reads the input event from stdin, dispatches to the
/// handler, and writes the output to stdout with the correct exit code.
///
/// - Empty output (all fields `None`) → exit 0, no stdout.
/// - Non-empty output → exit 0, JSON on stdout.
/// - `Err(...)` → exit 1, error message on stderr.
pub fn run(handler: impl HookHandler) -> ExitCode {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to create tokio runtime");
    rt.block_on(run_async(handler))
}

async fn run_async(handler: impl HookHandler) -> ExitCode {
    let mut input_str = String::new();
    if std::io::stdin().read_to_string(&mut input_str).is_err() {
        return ExitCode::FAILURE;
    }

    let input: Input = match serde_json::from_str(&input_str) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("symposium-hook: failed to parse input: {e}");
            return ExitCode::FAILURE;
        }
    };

    let output = match handler.handle_event(&input).await {
        Ok(o) => o,
        Err(e) => {
            eprintln!("symposium-hook: handler error: {e}");
            return ExitCode::FAILURE;
        }
    };

    if is_empty_output(&output) {
        return ExitCode::SUCCESS;
    }

    match serde_json::to_string(&output) {
        Ok(json) => println!("{json}"),
        Err(e) => {
            eprintln!("symposium-hook: failed to serialize output: {e}");
            return ExitCode::FAILURE;
        }
    }

    ExitCode::SUCCESS
}

fn is_empty_output(output: &Output) -> bool {
    match output {
        Output::PreToolUse(o) => o.additional_context.is_none() && o.updated_input.is_none(),
        Output::PostToolUse(o) => o.additional_context.is_none(),
        Output::UserPromptSubmit(o) => o.additional_context.is_none(),
        Output::SessionStart(o) => o.additional_context.is_none(),
    }
}
