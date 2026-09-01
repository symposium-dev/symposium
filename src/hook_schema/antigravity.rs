//! Antigravity CLI (`agy`) hook wire format.
//!
//! Three things here differ from every other agent and are load-bearing:
//!
//! 1. **A `PreToolUse` hook that writes `{}` denies the tool call.** Exit codes
//!    are ignored entirely; only stdout decides. So [`AntigravityPreToolUseOutput`]
//!    always serializes `decision`, and its default is `allow`.
//! 2. **The workspace comes from the payload, not the process.** A hook's working
//!    directory is the directory holding its own `hooks.json`, which for a global
//!    registration is `~/.gemini/config`. `workspacePaths[0]` is the only way to
//!    learn the project, so it maps to the canonical `cwd`.
//! 3. **`PreInvocation` stands in for `user-prompt-submit`.** Antigravity has no
//!    prompt event and `PreInvocation` fires before *every* model call, so
//!    dispatch gates it on `invocationNum == 0` (see `hook.rs`).
//!
//! Context is returned as an `injectSteps` entry rather than an
//! `additionalContext` field. All keys are camelCase (protojson).

use serde::{Deserialize, Serialize};

use crate::hook_schema::{
    Agent, AgentHookEvent, AgentHookInput, AgentHookOutput, erase_agent_hook_event, symposium,
};

pub struct Antigravity;
impl Agent for Antigravity {
    fn event(&self, event: super::HookEvent) -> Option<Box<dyn super::ErasedAgentHookEvent>> {
        match event {
            super::HookEvent::PreToolUse => {
                Some(erase_agent_hook_event(AntigravityPreToolUseEvent))
            }
            super::HookEvent::PostToolUse => {
                Some(erase_agent_hook_event(AntigravityPostToolUseEvent))
            }
            super::HookEvent::UserPromptSubmit => {
                Some(erase_agent_hook_event(AntigravityUserPromptSubmitEvent))
            }
            super::HookEvent::SessionStart => {
                Some(erase_agent_hook_event(AntigravitySessionStartEvent))
            }
            super::HookEvent::Stop => Some(erase_agent_hook_event(AntigravityStopEvent)),
            _ => None,
        }
    }
}

macro_rules! antigravity_event {
    ($event:ident, $input:ident, $output:ident) => {
        pub struct $event;
        impl AgentHookEvent for $event {
            type Input = $input;
            type Output = $output;
        }
    };
}

antigravity_event!(
    AntigravityPreToolUseEvent,
    AntigravityPreToolUseInput,
    AntigravityPreToolUseOutput
);
antigravity_event!(
    AntigravityPostToolUseEvent,
    AntigravityPostToolUseInput,
    AntigravityPostToolUseOutput
);
antigravity_event!(
    AntigravityUserPromptSubmitEvent,
    AntigravityInvocationInput,
    AntigravityInjectStepsOutput
);
antigravity_event!(
    AntigravitySessionStartEvent,
    AntigravitySessionStartInput,
    AntigravityInjectStepsOutput
);
antigravity_event!(
    AntigravityStopEvent,
    AntigravityStopInput,
    AntigravityStopOutput
);

// ── Common ────────────────────────────────────────────────────────────

/// Fields present on every Antigravity hook payload.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AntigravityCommon {
    #[serde(
        rename = "conversationId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub conversation_id: Option<String>,
    /// Empty when no workspace was adopted (headless `agy -p` without
    /// `--add-dir`), so callers must tolerate an absent project.
    #[serde(
        rename = "workspacePaths",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub workspace_paths: Vec<String>,
    #[serde(
        rename = "transcriptPath",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub transcript_path: Option<String>,
    #[serde(
        rename = "artifactDirectoryPath",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub artifact_directory_path: Option<String>,
    #[serde(rename = "modelName", default, skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
}

impl AntigravityCommon {
    fn cwd(&self) -> Option<String> {
        self.workspace_paths.first().cloned()
    }

    fn from_symposium(session_id: Option<String>, cwd: Option<String>) -> Self {
        Self {
            conversation_id: session_id,
            workspace_paths: cwd.into_iter().collect(),
            ..Default::default()
        }
    }
}

/// A tool call as Antigravity reports it: name and arguments nested under
/// `toolCall`, rather than the flat `tool_name` / `tool_input` other agents use.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AntigravityToolCall {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub args: serde_json::Value,
}

/// Steps injected back into the conversation. `ephemeralMessage` is the
/// equivalent of Claude Code's `additionalContext`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AntigravityInjectedStep {
    #[serde(
        rename = "ephemeralMessage",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub ephemeral_message: Option<String>,
    #[serde(
        rename = "userMessage",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub user_message: Option<String>,
    #[serde(rename = "toolCall", default, skip_serializing_if = "Option::is_none")]
    pub tool_call: Option<serde_json::Value>,
}

// ── PreToolUse ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AntigravityPreToolUseInput {
    #[serde(rename = "toolCall", default)]
    pub tool_call: AntigravityToolCall,
    #[serde(rename = "stepIdx", default, skip_serializing_if = "Option::is_none")]
    pub step_idx: Option<i64>,
    #[serde(flatten)]
    pub common: AntigravityCommon,
}

/// `decision` is deliberately **not** optional and never skipped: an object
/// without it — `{}` included — is treated by Antigravity as a denial.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntigravityPreToolUseOutput {
    pub decision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Shallow-merged into the tool call's arguments before it runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overwrite: Option<serde_json::Value>,
}

impl Default for AntigravityPreToolUseOutput {
    fn default() -> Self {
        Self {
            decision: "allow".into(),
            reason: None,
            overwrite: None,
        }
    }
}

impl AgentHookInput for AntigravityPreToolUseInput {
    fn parse_input(payload: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }
    fn to_symposium(&self) -> symposium::InputEvent {
        symposium::InputEvent::PreToolUse(symposium::PreToolUseInput::new(
            self.tool_call.name.clone(),
            self.tool_call.args.clone(),
            self.common.conversation_id.clone(),
            self.common.cwd(),
        ))
    }
    fn from_symposium(event: &symposium::InputEvent) -> Self {
        let symposium::InputEvent::PreToolUse(p) = event else {
            panic!("wrong event")
        };
        Self {
            tool_call: AntigravityToolCall {
                name: p.tool_name.clone(),
                args: p.tool_input.clone(),
            },
            step_idx: None,
            common: AntigravityCommon::from_symposium(p.session_id.clone(), p.cwd.clone()),
        }
    }
    fn to_string(&self) -> anyhow::Result<String> {
        serde_json::to_string(self).map_err(Into::into)
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

impl AgentHookOutput for AntigravityPreToolUseOutput {
    fn parse_output(output: &[u8]) -> anyhow::Result<Self> {
        if output.is_empty() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_slice(output)?)
    }
    fn from_symposium(event: &symposium::OutputEvent) -> Self {
        let symposium::OutputEvent::PreToolUse(o) = event else {
            return Self::default();
        };
        let denied = matches!(o.decision, symposium_sdk::hook::Decision::Deny);
        Self {
            decision: if denied {
                "deny".into()
            } else {
                "allow".into()
            },
            // A denial's explanation is the reason; otherwise context is
            // dropped, since PreToolUse has nowhere to put it.
            reason: o.additional_context.clone(),
            overwrite: o.updated_input.clone(),
        }
    }
    fn to_symposium(&self) -> symposium::OutputEvent {
        let decision = match self.decision.as_str() {
            "deny" | "deny_unless_prior_grant" => symposium_sdk::hook::Decision::Deny,
            _ => symposium_sdk::hook::Decision::Allow,
        };
        symposium::OutputEvent::PreToolUse(symposium::PreToolUseOutput::new(
            decision,
            self.reason.clone(),
            self.overwrite.clone(),
        ))
    }
    fn to_hook_output(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap()
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

// ── PostToolUse ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AntigravityPostToolUseInput {
    #[serde(rename = "toolCall", default)]
    pub tool_call: AntigravityToolCall,
    #[serde(rename = "stepIdx", default, skip_serializing_if = "Option::is_none")]
    pub step_idx: Option<i64>,
    /// Set when the tool failed; empty string otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(flatten)]
    pub common: AntigravityCommon,
}

/// Antigravity expects an empty object here; there is nothing to return.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AntigravityPostToolUseOutput {}

impl AgentHookInput for AntigravityPostToolUseInput {
    fn parse_input(payload: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }
    fn to_symposium(&self) -> symposium::InputEvent {
        symposium::InputEvent::PostToolUse(symposium::PostToolUseInput::new(
            self.tool_call.name.clone(),
            self.tool_call.args.clone(),
            self.error
                .clone()
                .filter(|e| !e.is_empty())
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null),
            self.common.conversation_id.clone(),
            self.common.cwd(),
        ))
    }
    fn from_symposium(event: &symposium::InputEvent) -> Self {
        let symposium::InputEvent::PostToolUse(p) = event else {
            panic!("wrong event")
        };
        Self {
            tool_call: AntigravityToolCall {
                name: p.tool_name.clone(),
                args: p.tool_input.clone(),
            },
            step_idx: None,
            error: p.tool_response.as_str().map(str::to_string),
            common: AntigravityCommon::from_symposium(p.session_id.clone(), p.cwd.clone()),
        }
    }
    fn to_string(&self) -> anyhow::Result<String> {
        serde_json::to_string(self).map_err(Into::into)
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

impl AgentHookOutput for AntigravityPostToolUseOutput {
    fn parse_output(_output: &[u8]) -> anyhow::Result<Self> {
        Ok(Self::default())
    }
    fn from_symposium(_event: &symposium::OutputEvent) -> Self {
        Self::default()
    }
    fn to_symposium(&self) -> symposium::OutputEvent {
        symposium::OutputEvent::PostToolUse(symposium::PostToolUseOutput::new(None))
    }
    fn to_hook_output(&self) -> serde_json::Value {
        serde_json::json!({})
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

// ── PreInvocation (user-prompt-submit) and SessionStart ────────────────

/// `PreInvocation` / `PostInvocation` payload.
///
/// `invocationNum` restarts at 0 on every turn, which is what makes it usable
/// as a "first call of this turn" gate but useless as a session marker.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AntigravityInvocationInput {
    #[serde(rename = "invocationNum", default)]
    pub invocation_num: i64,
    #[serde(
        rename = "initialNumSteps",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub initial_num_steps: Option<i64>,
    #[serde(flatten)]
    pub common: AntigravityCommon,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AntigravitySessionStartInput {
    #[serde(flatten)]
    pub common: AntigravityCommon,
}

/// Shared by the two events that can inject context back into the conversation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AntigravityInjectStepsOutput {
    #[serde(rename = "injectSteps", default)]
    pub inject_steps: Vec<AntigravityInjectedStep>,
}

impl AntigravityInjectStepsOutput {
    fn from_context(context: Option<&str>) -> Self {
        Self {
            inject_steps: context
                .map(|c| AntigravityInjectedStep {
                    ephemeral_message: Some(c.to_string()),
                    ..Default::default()
                })
                .into_iter()
                .collect(),
        }
    }

    fn context(&self) -> Option<String> {
        let joined: Vec<&str> = self
            .inject_steps
            .iter()
            .filter_map(|s| s.ephemeral_message.as_deref())
            .collect();
        (!joined.is_empty()).then(|| joined.join("\n"))
    }
}

impl AgentHookInput for AntigravityInvocationInput {
    fn parse_input(payload: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }
    fn to_symposium(&self) -> symposium::InputEvent {
        // Antigravity never sends the prompt text, so the canonical prompt is
        // empty; plugins keyed on prompt content cannot fire on this agent.
        symposium::InputEvent::UserPromptSubmit(symposium::UserPromptSubmitInput::new(
            String::new(),
            self.common.conversation_id.clone(),
            self.common.cwd(),
        ))
    }
    fn from_symposium(event: &symposium::InputEvent) -> Self {
        let symposium::InputEvent::UserPromptSubmit(p) = event else {
            panic!("wrong event")
        };
        Self {
            invocation_num: 0,
            initial_num_steps: None,
            common: AntigravityCommon::from_symposium(p.session_id.clone(), p.cwd.clone()),
        }
    }
    fn to_string(&self) -> anyhow::Result<String> {
        serde_json::to_string(self).map_err(Into::into)
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

impl AgentHookInput for AntigravitySessionStartInput {
    fn parse_input(payload: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }
    fn to_symposium(&self) -> symposium::InputEvent {
        symposium::InputEvent::SessionStart(symposium::SessionStartInput::new(
            self.common.conversation_id.clone(),
            self.common.cwd(),
        ))
    }
    fn from_symposium(event: &symposium::InputEvent) -> Self {
        let symposium::InputEvent::SessionStart(p) = event else {
            panic!("wrong event")
        };
        Self {
            common: AntigravityCommon::from_symposium(p.session_id.clone(), p.cwd.clone()),
        }
    }
    fn to_string(&self) -> anyhow::Result<String> {
        serde_json::to_string(self).map_err(Into::into)
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

impl AgentHookOutput for AntigravityInjectStepsOutput {
    fn parse_output(output: &[u8]) -> anyhow::Result<Self> {
        if output.is_empty() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_slice(output)?)
    }
    fn from_symposium(event: &symposium::OutputEvent) -> Self {
        Self::from_context(event.additional_context())
    }
    fn to_symposium(&self) -> symposium::OutputEvent {
        symposium::OutputEvent::SessionStart(symposium::SessionStartOutput::new(self.context()))
    }
    fn to_hook_output(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap()
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

// ── Stop ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AntigravityStopInput {
    #[serde(
        rename = "executionNum",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub execution_num: Option<i64>,
    #[serde(
        rename = "terminationReason",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub termination_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(rename = "fullyIdle", default, skip_serializing_if = "Option::is_none")]
    pub fully_idle: Option<bool>,
    #[serde(flatten)]
    pub common: AntigravityCommon,
}

/// `decision: "continue"` blocks the stop and re-enters the loop; any other
/// value lets the agent stop, so the field is omitted when there is nothing to say.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AntigravityStopOutput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl AgentHookInput for AntigravityStopInput {
    fn parse_input(payload: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(payload)?)
    }
    fn to_symposium(&self) -> symposium::InputEvent {
        symposium::InputEvent::Stop(symposium::StopInput::new(
            self.common.conversation_id.clone(),
            self.common.cwd(),
        ))
    }
    fn from_symposium(event: &symposium::InputEvent) -> Self {
        let symposium::InputEvent::Stop(p) = event else {
            panic!("wrong event")
        };
        Self {
            common: AntigravityCommon::from_symposium(p.session_id.clone(), p.cwd.clone()),
            ..Default::default()
        }
    }
    fn to_string(&self) -> anyhow::Result<String> {
        serde_json::to_string(self).map_err(Into::into)
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

impl AgentHookOutput for AntigravityStopOutput {
    fn parse_output(output: &[u8]) -> anyhow::Result<Self> {
        if output.is_empty() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_slice(output)?)
    }
    fn from_symposium(event: &symposium::OutputEvent) -> Self {
        // Context on Stop is only deliverable as the reason for continuing.
        match event.additional_context() {
            Some(ctx) => Self {
                decision: Some("continue".into()),
                reason: Some(ctx.to_string()),
            },
            None => Self::default(),
        }
    }
    fn to_symposium(&self) -> symposium::OutputEvent {
        symposium::OutputEvent::Stop(symposium::StopOutput::new(self.reason.clone()))
    }
    fn to_hook_output(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap()
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

/// Whether a `PreInvocation` payload is the first model call of its turn.
///
/// `PreInvocation` stands in for `user-prompt-submit`, but it fires before every
/// model call — several times in a turn that uses tools — so dispatch runs the
/// prompt event only on invocation 0. A payload that will not parse counts as
/// first, so a wire change makes the hook fire too often rather than never.
pub fn is_first_invocation(payload: &str) -> bool {
    serde_json::from_str::<AntigravityInvocationInput>(payload)
        .map(|p| p.invocation_num == 0)
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of the type: symposium contributes nothing on most tool
    /// calls, and an object without `decision` — `{}` included — is a denial.
    #[test]
    fn a_default_pre_tool_use_output_allows_rather_than_denying() {
        let json = AntigravityPreToolUseOutput::default().to_hook_output();
        assert_eq!(json["decision"], "allow");
        assert_ne!(json.to_string(), "{}");
    }

    #[test]
    fn a_no_op_symposium_output_still_serializes_an_allow() {
        let sym = symposium::OutputEvent::PreToolUse(symposium::PreToolUseOutput::default());
        let out = AntigravityPreToolUseOutput::from_symposium(&sym);
        assert_eq!(out.to_hook_output()["decision"], "allow");
    }

    #[test]
    fn a_denial_carries_its_reason() {
        let sym = symposium::OutputEvent::PreToolUse(symposium::PreToolUseOutput::deny("nope"));
        let out = AntigravityPreToolUseOutput::from_symposium(&sym);
        assert_eq!(out.decision, "deny");
        assert_eq!(out.reason.as_deref(), Some("nope"));
    }

    /// The hook's process cwd is wherever `hooks.json` lives, so the workspace
    /// has to come from the payload or sync targets the wrong directory.
    #[test]
    fn workspace_paths_become_the_canonical_cwd() {
        let input: AntigravityPreToolUseInput = serde_json::from_str(
            r#"{"toolCall":{"name":"run_command","args":{"CommandLine":"ls"}},
                "conversationId":"abc","workspacePaths":["/repo"],"stepIdx":2}"#,
        )
        .unwrap();
        let sym = input.to_symposium();
        assert_eq!(sym.cwd(), Some("/repo"));
        assert_eq!(sym.session_id(), Some("abc"));
        let symposium::InputEvent::PreToolUse(p) = &sym else {
            panic!()
        };
        assert_eq!(p.tool_name, "run_command");
        assert_eq!(p.tool_input["CommandLine"], "ls");
    }

    /// Headless `agy -p` without `--add-dir` sends no workspace at all.
    #[test]
    fn an_empty_workspace_list_yields_no_cwd() {
        let input: AntigravitySessionStartInput =
            serde_json::from_str(r#"{"conversationId":"abc","workspacePaths":[]}"#).unwrap();
        assert_eq!(input.to_symposium().cwd(), None);
    }

    #[test]
    fn context_round_trips_through_inject_steps() {
        let sym = symposium::OutputEvent::SessionStart(symposium::SessionStartOutput::new(Some(
            "hello".into(),
        )));
        let out = AntigravityInjectStepsOutput::from_symposium(&sym);
        assert_eq!(
            out.to_hook_output()["injectSteps"][0]["ephemeralMessage"],
            "hello"
        );
        assert_eq!(out.context().as_deref(), Some("hello"));
    }

    #[test]
    fn no_context_injects_no_steps() {
        let sym = symposium::OutputEvent::SessionStart(symposium::SessionStartOutput::new(None));
        let out = AntigravityInjectStepsOutput::from_symposium(&sym);
        assert!(out.inject_steps.is_empty());
    }

    #[test]
    fn post_tool_use_reports_an_error_and_returns_an_empty_object() {
        let input: AntigravityPostToolUseInput = serde_json::from_str(
            r#"{"toolCall":{"name":"run_command","args":{}},"error":"exit status 1",
                "conversationId":"c","workspacePaths":["/repo"]}"#,
        )
        .unwrap();
        let symposium::InputEvent::PostToolUse(p) = input.to_symposium() else {
            panic!()
        };
        assert_eq!(p.tool_response, serde_json::json!("exit status 1"));
        assert_eq!(
            AntigravityPostToolUseOutput::default().to_hook_output(),
            serde_json::json!({})
        );
    }

    #[test]
    fn stop_only_sets_continue_when_there_is_something_to_say() {
        let quiet = symposium::OutputEvent::Stop(symposium::StopOutput::new(None));
        assert!(
            AntigravityStopOutput::from_symposium(&quiet)
                .decision
                .is_none()
        );

        let loud = symposium::OutputEvent::Stop(symposium::StopOutput::new(Some("wait".into())));
        let out = AntigravityStopOutput::from_symposium(&loud);
        assert_eq!(out.decision.as_deref(), Some("continue"));
        assert_eq!(out.reason.as_deref(), Some("wait"));
    }

    #[test]
    fn only_the_first_invocation_of_a_turn_is_a_prompt() {
        assert!(is_first_invocation(r#"{"invocationNum":0}"#));
        assert!(!is_first_invocation(r#"{"invocationNum":1}"#));
        assert!(!is_first_invocation(r#"{"invocationNum":7}"#));
    }

    #[test]
    fn an_unparseable_invocation_payload_fires_rather_than_disappears() {
        assert!(is_first_invocation("not json"));
    }
}
