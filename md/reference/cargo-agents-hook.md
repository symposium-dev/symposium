# `symposium hook`

Entry point invoked by your agent's hook system. This is an internal command — you generally don't need to run it yourself.

## Usage

```bash
symposium hook <EVENT> [ARGS...]
```

## Behavior

When your agent starts, the registered hook calls `symposium hook` with the appropriate event. The hook does two things, in order:

1. **Syncs agent config** — reads the project configuration and installs enabled extensions into the agent's expected locations (equivalent to `symposium sync --agent`).
2. **Dispatches to plugin hooks** — runs any hook handlers defined by enabled plugins for the given event.

## Events

The specific events and arguments depend on which agent you are using. `symposium init --user` configures the hook registration appropriate for your agent.

## When is the hook invoked?

The hook is registered globally during `symposium init --user`. It runs automatically when your agent starts a session in a project that has a `.symposium/` directory.
