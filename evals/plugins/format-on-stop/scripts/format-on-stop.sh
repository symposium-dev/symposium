#!/bin/sh
# Runs when the agent's turn ends, in its working directory: records whether the code
# was already formatted, then formats it.
if cargo fmt --check >/dev/null 2>&1; then
    already_formatted=true
else
    already_formatted=false
fi
cargo fmt
status=$?
# In an evaluation, leave a record of the run.
if [ -n "${EVAL_EVIDENCE_DIR:-}" ]; then
    printf '{"hook":"format-on-stop","already_formatted":%s,"exit_code":%d}\n' \
        "$already_formatted" "$status" >> "$EVAL_EVIDENCE_DIR/hook-records.jsonl"
fi
exit "$status"
