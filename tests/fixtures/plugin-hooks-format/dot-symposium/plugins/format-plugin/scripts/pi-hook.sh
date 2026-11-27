#!/bin/sh
input=$(cat)
case "$input" in
  *'"tool_name":"bash"'*'"tool_input":{"command":"ls"}'*)
    printf '%s\n' '{"decision":"deny","additionalContext":"pi-hook-fired","updatedInput":{"command":"pwd"}}'
    ;;
  *)
    printf '%s\n' "unexpected Pi payload" >&2
    exit 1
    ;;
esac
