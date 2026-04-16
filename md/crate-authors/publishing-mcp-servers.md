# Publishing MCP servers

MCP servers let your plugin expose tools and resources to AI agents via the [Model Context Protocol](https://modelcontextprotocol.io/). When a user syncs their project, Symposium registers your MCP server into the agent's configuration automatically.

## Declaring an MCP server

MCP servers are declared in your plugin's TOML manifest with `[[mcp_servers]]` entries.

### Builtin servers

If your MCP server is built into the `symposium` binary, use the `builtin` form:

```toml
[[mcp_servers]]
name = "symposium"
type = "builtin"
args = ["mcp"]
```

The `args` array becomes the arguments passed to the `symposium` binary. Symposium resolves the binary path at sync time, so users don't need to hardcode it.

### Custom servers

For a standalone MCP server, declare it with the appropriate transport:

```toml
# Stdio transport (no type field needed)
[[mcp_servers]]
name = "widgetlib-mcp"
command = "/usr/local/bin/widgetlib-mcp"
args = ["--stdio"]

# HTTP transport
[[mcp_servers]]
type = "http"
name = "widgetlib-remote"
url = "http://localhost:8080/mcp"

# SSE transport
[[mcp_servers]]
type = "sse"
name = "widgetlib-sse"
url = "http://localhost:8080/sse"
```

HTTP and SSE entries require a `type` field to distinguish them. Stdio entries don't need one.

## How it works

When a user runs `symposium sync` (or the hook triggers it automatically), Symposium:

1. Collects `[[mcp_servers]]` entries from all enabled plugins.
2. Resolves builtin entries to the local `symposium` binary path.
3. Writes each server into the agent's MCP configuration file.

Registration is idempotent. If the entry already exists with the correct values, it's left untouched. Stale entries are updated in place.

## Agent support

All supported agents have MCP server configuration. Symposium handles the format differences — you declare the server once and it works across agents.

See the [per-agent registration table](../reference/plugin-definition.md#how-registration-works) for where each agent stores MCP config.

## Example: plugin with skills and an MCP server

```toml
name = "widgetlib"

[[skills]]
crates = ["widgetlib"]
source.path = "skills"

[[mcp_servers]]
name = "widgetlib-mcp"
command = "widgetlib-mcp"
args = ["--stdio"]
```

## Reference

See the [`[[mcp_servers]]` section](../reference/plugin-definition.md#mcp_servers) in the plugin definition reference for the full field listing.
