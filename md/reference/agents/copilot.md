# GitHub Copilot

Config name: `copilot`

## Skills

| Scope | Path |
|-------|------|
| Project | `.agents/skills/<name>/SKILL.md` |
| Global | *(none)* |

Copilot has no global skills path.

## Plugins

A plugin enabled with `cargo agents use --global` is installed for you as a Copilot CLI plugin:

| Scope | File |
|-------|------|
| Global | `~/.copilot/settings.json` (`$COPILOT_HOME/settings.json` when set) |

Symposium registers `~/.symposium/installed/` there as the plugin marketplace `symposium` and enables `<plugin>@symposium`. Copilot loads the plugin from that directory in every project, and its skills keep their own names. `copilot plugin disable <plugin>@symposium` (or `uninstall`) turns it off, and symposium leaves that choice alone. A plugin installed while a session runs appears after `/restart` or in the next session. This needs Copilot CLI 1.0.81 or later.

**Caveat:** symposium reads `settings.json` as plain JSON. If yours contains comments, symposium leaves it untouched and installs the plugin's skills in each project instead.

**Caveat:** a skill with the same name in `.agents/skills/` wins over the plugin's. When another configured agent (OpenCode, for one) still receives a copy of the skill there, Copilot uses that copy.

## Hooks

Symposium creates a `symposium.json` file in the project hooks directory, and merges entries into the `hooks` key of the global settings.

| Scope | File |
|-------|------|
| Project | `.github/hooks/symposium.json` |
| Global | `~/.copilot/settings.json` |

Events registered: `preToolUse`, `postToolUse`, `userPromptSubmitted`, `sessionStart` (camelCase).

Output format: JSON. Uses `"bash"` key instead of `"command"` for platform-specific dispatch. Any non-zero exit code denies (not just exit 2).

**Caveat:** Copilot loads a project's hooks only once you trust the folder (it asks the first time you open it).

**Caveat:** Copilot also runs the hooks in `.claude/settings.json`. In a project where Claude Code is configured too, each Copilot event runs `cargo agents hook` twice, once as each agent.

## MCP servers

| Scope | File | Key |
|-------|------|-----|
| Global | `~/.copilot/mcp-config.json` | `mcpServers.<name>` |

Copilot reads no project-level MCP file, so a project-scoped registration goes to the global file.
