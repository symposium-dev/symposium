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

On later runs, explicitly selected agents stay checked even if their config directory disappears. `--add-agent` and checking an initially unchecked box make a choice explicit; accepting an auto-selected checkbox leaves it managed by detection. Previously configured agents migrate as explicit choices when upgrading. To make an automatically selected agent explicit, use `--add-agent <name>`.

Detection-managed agents are unchecked when no longer found (`No longer detected: ...`); newly detected agents are checked. Deselecting or removing an agent clears its explicit status and preserves that deselection while it remains detected. Deselecting every agent removes Symposium's hooks and MCP servers. Without a terminal, later runs keep the configured agents as they are.

If a user config already exists, `init` updates it (preserving existing settings not affected by the flags).

Removing an agent also runs [`cargo agents sync`](./cargo-agents-sync.md) when `init` is run inside a Rust workspace, which removes the skills Symposium installed there for that agent. Copies in other workspaces stay until `cargo agents sync` runs in each of them.

## Options

| Flag | Description |
|------|-------------|
| `--add-agent <name>` | Add an agent (e.g., `antigravity`, `claude`, `copilot`). Repeatable. Skips the interactive prompt. |
| `--remove-agent <name>` | Remove an agent, including one this version no longer knows. Repeatable. |
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
