#!/bin/sh
root=$(printf '%s' "$CLAUDE_PLUGIN_ROOT" | tr '\\' '/')
echo "{\"hookSpecificOutput\":{\"hookEventName\":\"SessionStart\",\"additionalContext\":\"demo-tools from $root\"}}"
