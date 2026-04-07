# Cargo workflow

The cargo workflow monitors tools invoked to reduce token usage and (perhaps in the future) trigger other actions.


## Hook flow

Hooks let Symposium react to what the agent is doing. The Claude Code plugin registers three hook events:

```mermaid
sequenceDiagram
    participant Agent
    participant Symposium
    participant Session as Session State
    
    Note over Agent: Agent is about to use a tool
    Agent->>Symposium: symposium hook <agent> pre-tool-use (JSON on stdin)
    Symposium->>Symposium: dispatch to plugin-defined hook commands
    Note over Symposium: plugin hooks can allow (exit 0) or block (exit non-zero)
    Symposium-->>Agent: allow/block + optional guidance
```

**PreToolUse** has no built-in logic — it dispatches to plugin-defined hook commands, which receive the event JSON on stdin and can allow or block the action.
