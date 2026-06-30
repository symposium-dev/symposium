# Workspace plugins

Your workspace itself is a plugin source. Symposium scans the workspace root and each member crate using the same discovery rules as any installed crate — so you can add project-specific skills, hooks, and MCP servers just by placing files in the right spots.

## Default skill locations

Every plugin (including the implicit workspace plugin) has two default skill sources:

| Directory | When installed | Use case |
| --- | --- | --- |
| `skills/` | Always (unconditional) | Maintaining or using your crate |
| `.agents/skills/` | Your crate is in the current workspace | Maintaining your crate |

Just create a directory with a `SKILL.md` file in either directory and they'll be picked up on the next sync.

Both directories are searched recursively, so you can organize skills into subdirectories freely.

### Flattening into agent skill directories

Agents expect skills at a flat depth (e.g., `.claude/skills/<name>/SKILL.md`). When Symposium finds skills in nested subdirectories, it hoists them to the flat layout using the skill name. For example, given:

```
.agents/skills/
  guides/
    error-handling/
      SKILL.md
```

Symposium installs it as `.claude/skills/error-handling/SKILL.md` (with `.symposium` marker and `.gitignore`).

If two skills from different subdirectories have the same name, Symposium disambiguates with a hash suffix:

```
.agents/skills/
  guides/
    deploy/
      SKILL.md       ← skill named "deploy"
  runbooks/
    deploy/
      SKILL.md       ← also named "deploy"
```

Result:

```
.claude/skills/
  deploy/
    SKILL.md         ← from guides/deploy
  deploy-a1b2c3d4/
    SKILL.md         ← from runbooks/deploy (disambiguated)
```

User-managed skills (those without the `.symposium` marker) are never overwritten — if a collision occurs with a user-managed skill, Symposium always takes the suffixed path.

## Propagation to agent-specific directories

Symposium copies skills into each configured agent's own skill directory (e.g., `.claude/skills/`, `.kiro/skills/`). This means you author skills once in a central location and all agents see them, regardless of which ones your team uses.

## Adding a `SYMPOSIUM.toml`

For more control, add a `SYMPOSIUM.toml` at your workspace root or in a member crate. This lets you:

- Gate skills with predicates (`crates`, `env`, `path_exists`, etc.)
- Add hooks and MCP servers
- Declare `[[plugins]]` for companion plugin crates
- Disable defaults like `skills/` and `.agents/skills/`

Adding a `SYMPOSIUM.toml` is purely additive — the default `skills/` and `.agents/skills/` sources continue to work unless explicitly suppressed with `defaults.skills = false`.

## Pre-existing files

Symposium never touches skills in `.claude/skills/`, `.kiro/skills/` etc. that it did not put there itself. If a name collision occurs with a user-managed skill, Symposium installs to a suffixed directory (e.g., `my-skill-a1b2c3d4/`) so both coexist.
