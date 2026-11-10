# `symposium init`

Set up Symposium for the current user.

## Usage

```bash
symposium init [OPTIONS]
```

## Behavior

Prompts for which agents you use (e.g., Antigravity CLI, Claude Code, Copilot) and where to install hooks, writes `~/.symposium/config.toml`, and registers hooks for each selected agent.

If a user config already exists, `init` updates it (preserving existing settings not affected by the flags).

Without a terminal and without `--add-agent`, `init` keeps the configured agents, or configures Claude Code when there are none.

Removing an agent also runs [`symposium sync`](./symposium-sync.md) when `init` is run inside a Rust workspace, which removes the skills Symposium installed there for that agent. Copies in other workspaces stay until `symposium sync` runs in each of them.

## Options

| Flag | Description |
|------|-------------|
| `--add-agent <name>` | Add an agent (e.g., `antigravity`, `claude`, `copilot`). Repeatable. Skips the interactive prompt. |
| `--remove-agent <name>` | Remove an agent, including one this version no longer knows. Repeatable. |
| `--hook-scope <scope>` | Where to install hooks: `global` (default, writes to `~/`) or `project` (writes to the project directory). |

## Examples

Interactive setup:

```bash
symposium init
```

Non-interactive, specifying agents directly:

```bash
symposium init --add-agent claude --add-agent antigravity
```

Adding an agent to an existing config:

```bash
symposium init --add-agent copilot
```

Removing an agent:

```bash
symposium init --remove-agent antigravity
```
