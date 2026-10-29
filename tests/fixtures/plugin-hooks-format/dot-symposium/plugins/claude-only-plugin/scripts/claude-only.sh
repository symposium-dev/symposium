#!/bin/sh
root=$(printf '%s' "$CLAUDE_PLUGIN_ROOT" | tr '\\' '/')
echo "{\"hookSpecificOutput\":{\"hookEventName\":\"PreToolUse\",\"additionalContext\":\"claude-only-fired from $root\"}}"
