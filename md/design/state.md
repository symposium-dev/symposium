# State

Documents the kind of state maintained by the Symposium agent.

## `state.toml`

`state.toml` in the config directory (`~/.symposium/`) holds what Symposium needs to remember between runs, as opposed to what the user configures. It records the version of the binary that last wrote it, the last update check, and the MCP server ledger below. A file that cannot be parsed loads as the defaults.

## MCP server ledger

Every MCP server entry Symposium writes into an agent's config is recorded as one `[[mcp-server]]` table:

```toml
[[mcp-server]]
agent = "claude"
file = "/Users/me/.claude.json"
name = "demo"
```

`file` is the path `Agent::mcp_config_path` returned when the entry was written. Removing MCP servers goes through this ledger only: an entry comes out of the file it was written to, and leaves the ledger once that succeeds (a failure keeps it, so the next run retries).

The ledger exists because a server name is not proof of ownership. A user, or their team in a committed project file, may have their own server named like a plugin's, in an agent Symposium does not manage. Removing by name deleted those. Nothing can be marked inside the agent files instead: several agents reject unknown keys, and JSON has no comments.

The ledger is per machine. An entry Symposium did not write on this machine, including one written by an earlier version that had no ledger, is never removed automatically.

## Agent choices

`detected-agents` records the last confirmed detection snapshot. `explicit-agents`
records agents selected with `--add-agent` or by checking an initially unchecked
box. These remain selected even when their config directory disappears. Accepting
an automatically checked box does not make it explicit. Deselecting or removing an
agent clears its explicit status; removing a currently detected agent also records
it in detection history so the next prompt does not immediately re-add it.

An absent `explicit-agents` field means the state predates choice tracking: the
configured agents migrate as explicit choices. An empty array means migration has
already happened and all selections are detection-managed. Updates preserve the
MCP ledger and update-check timestamp.
