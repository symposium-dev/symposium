# `cargo agents sync`

Scans workspace dependencies, installs applicable skills into agent directories, and cleans up stale skills.

## Flow

1. **Find workspace root** — run `cargo metadata` to locate the workspace manifest directory.

2. **Load plugin sources** — read the user config's `[[plugin-source]]` entries and load their plugin manifests. For git sources, fetch/update as needed.

3. **Scan dependencies** — read the full dependency graph from the workspace.

4. **Match skills to dependencies** — for each plugin, evaluate skill group crate predicates and individual skill `crates` frontmatter against the workspace dependencies.

5. **Install skills per agent** — for each configured agent:
   - Copy applicable `SKILL.md` files into the agent's expected skill directory.
   - Write a `.symposium.toml` manifest in the agent's skill directory tracking which skills were installed by symposium.
   - Remove skills that are in the old manifest but no longer applicable.
   - Leave skills not in the manifest (user-managed) untouched.

6. **Register hooks** — ensure global hooks and MCP servers are registered for all configured agents. Unregister hooks for agents no longer in the config.

## Skill manifest

Each agent's skill directory contains a `.symposium.toml` file tracking what symposium installed:

```toml
installed = [
    "serde-guidance",
    "tokio-guidance",
]
```

This allows symposium to clean up stale skills without touching user-managed skill files.

## Auto-sync

When `auto-sync = true` is set in the user config, the hook handler runs `sync` automatically during agent sessions. This keeps skills in sync as dependencies change.
