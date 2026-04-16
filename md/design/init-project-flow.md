# `cargo agents init --project`

Sets up project-level configuration for the current workspace.

## Flow

1. **Find workspace root** — run `cargo metadata` to locate the workspace manifest directory.

2. **Prompt for agent override** — ask whether to set a project-level agent (default: use each developer's own user-wide preference).

3. **Create project config** — create the `.symposium/` directory and an empty `.symposium/config.toml`. If an agent override was selected, write the `[agent]` section.

4. **Run `sync --workspace`** — delegates to the [`sync --workspace` flow](./sync-workspace-flow.md) to scan dependencies, discover available extensions, and populate the config file.

5. **Run `sync --agent`** — delegates to the [`sync --agent` flow](./sync-agent-flow.md) to install the discovered extensions into the agent's expected locations and ensure hooks are in place.

> **Combined init**: When `init` runs both user and project setup (either by default or with both flags), user setup completes first (including its own `sync --agent` for global hooks), then project setup runs. The `sync --agent` at this step sees the full context — user config plus project config — and places hooks accordingly.
