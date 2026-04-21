# Common issues

## Known hook implementation gaps

The following issues were identified by auditing our hook implementations against the agent reference docs (`md/design/agent-details/`). They don't cause crashes (the fallback path handles events without agent-specific handlers) but mean some features are incomplete.

### `toolArgs` not parsed (Copilot)

Copilot sends `toolArgs` as a JSON *string* (not an object). Our `CopilotPreToolUsePayload` declares it as `serde_json::Value` and passes it through as-is in `to_hook_payload()`. Downstream code expecting structured tool args will get a raw string. Should parse the JSON string into a `Value` during conversion.

### `permissionDecision` dropped (Copilot)

`CopilotPreToolUseOutput::from_hook_output()` never maps `permissionDecision` or `permissionDecisionReason` from the builtin hook output. If a builtin handler wants to deny a tool call, the decision is silently lost in Copilot output.

### `matches_matcher` uses substring instead of regex (Claude Code / Gemini)

`HookSubPayload::matches_matcher()` uses `matcher.contains(&tool_name)` — a substring check. The Claude Code and Gemini references specify regex matching for tool events. Patterns like `"mcp__.*"` or `"^Bash$"` would not work correctly. Also the operand order is inverted (checks if matcher contains tool name, not if tool name matches the matcher regex).

### Gemini `SessionStart` matcher

`ensure_gemini_hook_entry` uses `"matcher": ".*"` for all events including `SessionStart`. Per the Gemini reference, lifecycle events use exact-string matchers, not regex. Likely harmless in practice since `".*"` matches anything.
