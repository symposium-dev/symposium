# `cargo agents hook`

Entry point invoked by your agent's hook system. This is an internal command — you generally don't need to run it yourself.

## Usage

```bash
cargo agents hook <AGENT> <EVENT>
```

## Behavior

When your agent triggers a hook event, it calls `cargo agents hook` with the agent name and event type. The hook does two things:

1. **Auto-sync** (if enabled) — when `auto-sync = true` in the user config, runs [`cargo agents sync`](./cargo-agents-sync.md) to ensure skills are current for the workspace. The workspace root is resolved from the hook payload's `cwd` field; if the payload does not include a working directory, the process's current working directory is used as a fallback. Failures are logged but don't block hook dispatch.

2. **Dispatches to plugin hooks** — runs any hook handlers defined by [plugins](./plugin-definition.md#hooks) for the given event.

## Events

The specific events depend on which agent you are using. `cargo agents init` configures the hook registration appropriate for your agents.

## When is the hook invoked?

The hook is registered globally during `cargo agents init`. It runs automatically when your agent triggers supported events (e.g., session start, tool use).
