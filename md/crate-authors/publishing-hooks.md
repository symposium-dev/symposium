# Publishing hooks

Hooks let your plugin respond to events during an AI coding session — for example, checking tool invocations or validating generated code.

## Defining a hook

Hooks are declared in your plugin's TOML manifest:

```toml
[[hooks]]
name = "check-widget-usage"
event = "PreToolUse"
matcher = "Bash"
command = "./scripts/check-widget.sh"
```

When the agent triggers a matching event, Symposium runs the command with the event payload as JSON on stdin.

## Hook fields

| Field | Description |
|-------|-------------|
| `name` | A descriptive name for the hook (used in logs). |
| `event` | The event type to match (e.g., `PreToolUse`). |
| `matcher` | Which tool invocations to match (e.g., `Bash`, or omit for all). |
| `command` | The command to run. Resolved relative to the plugin directory. |

## Example: checking Bash commands

A hook that inspects Bash tool invocations before they run:

```toml
[[hooks]]
name = "inspect-bash"
event = "PreToolUse"
matcher = "Bash"
command = "./scripts/inspect-bash.sh"
```

The script receives the full event JSON on stdin and can:
- Exit 0 to allow the action
- Exit non-zero to block it
- Write guidance to stdout for the agent

## Testing hooks

Use the CLI to test a hook with sample input (specify the agent):

```bash
echo '{"tool": "Bash", "input": "cargo test"}' | symposium hook claude pre-tool-use
```

You can also use `copilot` or `gemini` as the agent name, e.g. `symposium hook copilot pre-tool-use`.
