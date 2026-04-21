# Common issues

## Known hook implementation gaps

The following issues were identified by auditing our hook implementations against the agent reference docs (`md/design/agent-details/`). They don't cause crashes (the fallback path handles events without agent-specific handlers) but mean some features are incomplete.

### `updatedInput` type mismatch (Claude Code)

`HookSpecificOutput.updated_input` and Claude's `ClaudeHookSpecificOutput.updated_input` are typed as `Option<String>`, but per the Claude Code reference, `updatedInput` is a JSON object (e.g., `{"command": "safe-cmd"}`). Should be `Option<serde_json::Value>`.

### `toolArgs` not parsed (Copilot)

Copilot sends `toolArgs` as a JSON *string* (not an object). Our `CopilotPreToolUsePayload` declares it as `serde_json::Value` and passes it through as-is in `to_hook_payload()`. Downstream code expecting structured tool args will get a raw string. Should parse the JSON string into a `Value` during conversion.

### `permissionDecision` dropped (Copilot)

`CopilotPreToolUseOutput::from_hook_output()` never maps `permissionDecision` or `permissionDecisionReason` from the builtin hook output. If a builtin handler wants to deny a tool call, the decision is silently lost in Copilot output.

### Gemini `SessionStart` matcher

`ensure_gemini_hook_entry` uses `"matcher": ".*"` for all events including `SessionStart`. Per the Gemini reference, lifecycle events use exact-string matchers, not regex. Likely harmless in practice since `".*"` matches anything.
