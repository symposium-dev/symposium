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
echo '{"tool": "Bash", "input": "cargo test"}' | cargo agents hook claude pre-tool-use
```

You can also use `copilot` or `gemini` as the agent name, e.g. `cargo agents hook copilot pre-tool-use`.

## Supported hooks

| Hook event | Description | CLI usage |
|------------|-------------|-----------|
| `PreToolUse` | Triggered before a tool (for example, `Bash`) is invoked by the agent. The hook receives the event payload on stdin. | `pre-tool-use` |

## Agent → hook name mapping

The table below lists tool events as rows and agents as columns. Each cell is the hook event name the agent uses for that tool event.

| Tool / Event | Claude (`claude`) | Copilot (`copilot`) | Gemini (`gemini`) |
|--------------|------------------------------------:|-------------------:|------------------:|
| `PreToolUse` | `PreToolUse` | `PreToolUse` | `BeforeTool` |

## Hook semantics

- Exit codes:
	- `0` — success: the hook's stdout is parsed as JSON and merged into the overall hook result.
	- `2` (or no reported exit code) — treated as a failure: dispatch stops immediately and the hook's stderr is returned to the caller.
	- any other non-zero code — treated as success for dispatching purposes; stdout is still parsed and merged when possible.

- Stdout handling:
	- Hooks SHOULD write a JSON object to stdout to contribute structured data back to the caller. Valid JSON objects are merged together across successful hooks; keys from later hooks overwrite earlier keys.
	- If a hook writes non-JSON to stdout, the output will be ignored and a warning is logged.

- Stderr handling:
	- If a hook exits with code `2` (or no exit code), dispatch returns immediately with the hook's stderr as the error message. Otherwise stderr is captured but not returned on success.
