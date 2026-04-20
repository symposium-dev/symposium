# Kiro

Config name: `kiro`

## Skills

| Scope | Path |
|-------|------|
| Project | `.kiro/skills/<name>/SKILL.md` |
| Global | `~/.kiro/skills/<name>/SKILL.md` |

Kiro uses its own skill path, not the vendor-neutral `.agents/skills/`.

## Hooks

Kiro requires hooks to be registered with a named agent. We create a `symposium` agent by creating a `symposium.json` agent definition file in `.kiro/agents/`. This registers Symposium as a Kiro agent with hooks attached. If you use a different agent, you won't benefit from hook-based features like token reduction unless you manually add the hooks into your agent definition.

| Scope | File |
|-------|------|
| Project | `.kiro/agents/symposium.json` |
| Global | `~/.kiro/agents/symposium.json` |

Events registered: `preToolUse`, `postToolUse`, `userPromptSubmit`, `agentSpawn` (camelCase; `agentSpawn` maps to session-start internally).

Output format: plain text on stdout (not JSON). Exit code 2 blocks `preToolUse` only.

The generated agent file includes `"tools": ["*"]` (all tools available) and `"resources": ["skill://.kiro/skills/**/SKILL.md"]` (auto-discover skills). Without `tools`, a Kiro custom agent has zero tools.

**Caveat:** Kiro uses a flat hook entry format (`{ "command": "..." }`) unlike the nested format used by Claude/Gemini/Codex. Unregistration deletes the `symposium.json` file entirely.

## MCP servers

| Scope | File | Key |
|-------|------|-----|
| Project | `.kiro/settings/mcp.json` | `mcpServers.<name>` |
| Global | `~/.kiro/settings/mcp.json` | `mcpServers.<name>` |
