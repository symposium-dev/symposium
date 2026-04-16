#!/usr/bin/env python3
"""Harness for running a Claude Agent SDK session in symposium integration tests.

Invoked via:
    uv run --with claude-agent-sdk \
        tests/agent_harness/run_scenario.py \
        --prompt "..." \
        --cwd /tmp/test-xxx \
        --trace /tmp/test-xxx/hook-trace.jsonl

Sets SYMPOSIUM_HOOK_TRACE in the agent subprocess environment so the
symposium CLI appends JSONL entries for each hook invocation.
"""

import argparse
import asyncio
import os
import sys

from claude_agent_sdk import ClaudeAgentOptions, ResultMessage, query


async def run(prompt: str, cwd: str, trace: str) -> None:
    # Build env: inherit current env, add SYMPOSIUM_HOOK_TRACE, and prepend
    # the cargo target dir to PATH so the freshly-built symposium binary is found.
    env = {"SYMPOSIUM_HOOK_TRACE": trace}
    cargo_bin = os.environ.get("CARGO_BIN_DIR")
    if cargo_bin:
        env["PATH"] = cargo_bin + os.pathsep + os.environ.get("PATH", "")

    options = ClaudeAgentOptions(
        cwd=cwd,
        setting_sources=["project"],
        permission_mode="bypassPermissions",
        max_turns=5,
        env=env,
    )

    async for message in query(prompt=prompt, options=options):
        if isinstance(message, ResultMessage):
            if message.subtype == "error_during_execution":
                print(f"Agent error: {message.result}", file=sys.stderr)
                sys.exit(1)


def main() -> None:
    parser = argparse.ArgumentParser(description="Run a Claude Agent SDK scenario")
    parser.add_argument("--prompt", required=True, help="Prompt to send to the agent")
    parser.add_argument("--cwd", required=True, help="Working directory for the agent")
    parser.add_argument("--trace", required=True, help="Path to JSONL trace file")
    args = parser.parse_args()

    asyncio.run(run(args.prompt, args.cwd, args.trace))


if __name__ == "__main__":
    main()
