# Workspace plugins

Your workspace itself is a plugin source. Symposium scans the workspace root and each member crate using the same discovery rules as any installed crate — so you can add project-specific skills, hooks, and MCP servers just by placing files in the right spots.

## Default skill locations

Every plugin (including the implicit workspace plugin) has two default skill sources:

| Directory | Search depth | When installed | Use case |
| --- | --- | --- | --- |
| `skills/` | Recursive | Always (unconditional) | Skills for users of your crate |
| `.agents/skills/` | Recursive | Only when `workspace()` is true | Skills for developers of this project |

Just drop `SKILL.md` files in either directory and they'll be picked up on the next sync. No `SYMPOSIUM.toml` needed.

Both directories are searched recursively, so you can organize skills into subdirectories freely. Nested `.agents/skills/` entries are hoisted to the flat skill name when copied to agents that require flat skill directories.

## Propagation to agent-specific directories

Symposium copies skills from `.agents/skills/` into each configured agent's own skill directory (e.g., `.claude/skills/`, `.kiro/skills/`). This means you author skills once in a central location and all agents see them, regardless of which ones your team uses.

## Adding a `SYMPOSIUM.toml`

For more control, add a `SYMPOSIUM.toml` at your workspace root or in a member crate. This lets you:

- Gate skills with predicates (`crates`, `env`, `path_exists`, etc.)
- Add hooks and MCP servers
- Declare `[[plugins]] source.crate` for companion plugin crates
- Disable defaults like `skills/` and `.agents/skills/`

Adding a `SYMPOSIUM.toml` is purely additive — the default `skills/` and `.agents/skills/` sources continue to work. See the [plugin definition reference](./reference/plugin-definition.md) for details on suppressing defaults.

## Recommended git setup

We recommend you commit your `.agents/skills/` or `skills/` directories into the repository. Symposium installs a `.gitignore` file into every skill that it copies, so automatically installed skills should not dirty your git status.

## Pre-existing files

Symposium never touches skills in `.claude/skills/`, `.kiro/skills/` etc. that it did not put there itself. If a name collision occurs with a user-managed skill, Symposium installs to a suffixed directory (e.g., `my-skill-a1b2c3d4/`) so both coexist.
