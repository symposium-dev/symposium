# `cargo agents init`

Set up Symposium for the current user.

## Usage

```bash
cargo agents init [OPTIONS]
```

## Behavior

Asks which agents you use and where to install hooks, writes `~/.symposium/config.toml`, and registers hooks for each selected agent.

The agents installed on your machine come preselected, so confirming is usually a single Enter. An agent counts as installed when its user config directory exists:

| Agent | Config directory |
|-------|------------------|
| Antigravity CLI | `~/.gemini/antigravity-cli/` |
| Claude Code | `~/.claude/` (or `$CLAUDE_CONFIG_DIR`) |
| Codex CLI | `~/.codex/` |
| GitHub Copilot | `~/.copilot/` |
| Goose | `$XDG_CONFIG_HOME/goose/` (default `~/.config/goose/`) |
| Kiro | `~/.kiro/` |
| OpenCode | `$XDG_CONFIG_HOME/opencode/` (default `~/.config/opencode/`) |

When none is found, `init` says so and the list starts empty, so you pick the ones you use. On a first run, at least one agent must be selected. Without a terminal to prompt on, `init` configures the detected agents, or fails asking for `--add-agent` when none is found.

On later runs, the list again preselects what is installed: an agent installed since comes checked (`Detected since last setup: ...`) and a configured agent that is no longer found comes unchecked (`No longer detected: ...`). Deselecting all of them removes Symposium's hooks and MCP servers. Without a terminal, later runs keep the configured agents as they are. `--add-agent` / `--remove-agent` ignore detection.

If a user config already exists, `init` updates it (preserving existing settings not affected by the flags).

## Options

| Flag | Description |
|------|-------------|
| `--add-agent <name>` | Add an agent (e.g., `antigravity`, `claude`, `copilot`). Repeatable. Skips the interactive prompt. |
| `--remove-agent <name>` | Remove an agent. Repeatable. |
| `--hook-scope <scope>` | Where to install hooks: `global` (default, writes to `~/`) or `project` (writes to the project directory). |

## Examples

Interactive setup:

```bash
cargo agents init
```

Non-interactive, specifying agents directly:

```bash
cargo agents init --add-agent claude --add-agent antigravity
```

Adding an agent to an existing config:

```bash
cargo agents init --add-agent copilot
```

Removing an agent:

```bash
cargo agents init --remove-agent antigravity
```
