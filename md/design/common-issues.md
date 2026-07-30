# Common issues

## Known hook implementation gaps

The following issues were identified by auditing our hook implementations against the agent reference docs (`md/design/agent-details/`). They don't cause crashes (the fallback path handles events without agent-specific handlers) but mean some features are incomplete.

### `permissionDecision` dropped (Copilot)

`CopilotPreToolUseOutput::from_hook_output()` never maps `permissionDecision` or `permissionDecisionReason` from the builtin hook output. If a builtin handler wants to deny a tool call, the decision is silently lost in Copilot output.

### Gemini `SessionStart` matcher

`ensure_gemini_hook_entry` uses `"matcher": ".*"` for all events including `SessionStart`. Per the Gemini reference, lifecycle events use exact-string matchers, not regex. Likely harmless in practice since `".*"` matches anything.
