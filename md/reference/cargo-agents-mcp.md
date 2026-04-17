# `symposium mcp`

Run symposium as an MCP (Model Context Protocol) server using stdio transport.

## Usage

```bash
symposium mcp
```

## Behavior

Starts an MCP server that exposes the `start` and `crate` commands as MCP tools. The server communicates over stdin/stdout using the MCP protocol.

This allows agents that support MCP to access symposium's functionality directly without shell command invocation.
