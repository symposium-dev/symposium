#!/bin/sh
cat >/dev/null
if [ -f symposium-cwd-marker ]; then where=workspace; else where=elsewhere; fi
echo "{\"UserPromptSubmit\":{\"additionalContext\":\"hook ran in the $where\"}}"
