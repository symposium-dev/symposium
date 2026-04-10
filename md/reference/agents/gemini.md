# Gemini CLI

Config name: `gemini`

## Skills

| Scope | Path |
|-------|------|
| Project | `.agents/skills/<name>/SKILL.md` |
| Global | `~/.gemini/skills/<name>/SKILL.md` |

## Hooks

Symposium merges hook entries into Gemini's `settings.json`.

| Scope | File |
|-------|------|
| Project | `.gemini/settings.json` |
| Global | `~/.gemini/settings.json` |

Events registered: `BeforeTool`, `AfterTool`, `SessionStart` (Gemini's own naming).

Output format: JSON with nested matcher groups. Timeouts in milliseconds.
