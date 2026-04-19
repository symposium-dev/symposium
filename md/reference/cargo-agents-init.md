# `symposium init`

Set up `symposium` for the current user.

## Usage

```bash
symposium init [OPTIONS]
```

## Behavior

Prompts for which agents you use (e.g., Claude Code, Copilot, Gemini) and where to install hooks, writes `~/.symposium/config.toml`, and registers hooks for each selected agent.

If a user config already exists, `init` updates it (preserving existing settings not affected by the flags).

## Options

| Flag | Description |
|------|-------------|
| `--add-agent <name>` | Add an agent (e.g., `claude`, `copilot`, `gemini`). Repeatable. Skips the interactive prompt. |
| `--remove-agent <name>` | Remove an agent. Repeatable. |
| `--hook-scope <scope>` | Where to install hooks: `global` (default, writes to `~/`) or `project` (writes to the project directory). |

## Examples

Interactive setup:

```bash
symposium init
```

Non-interactive, specifying agents directly:

```bash
symposium init --add-agent claude --add-agent gemini
```

Adding an agent to an existing config:

```bash
symposium init --add-agent copilot
```

Removing an agent:

```bash
symposium init --remove-agent gemini
```
