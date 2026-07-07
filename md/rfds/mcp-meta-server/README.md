# MCP meta-server for progressive tool disclosure

## TL;DR

- Instead of writing plugin MCP servers directly into agent config, Symposium runs a single "meta" MCP server that gates access to all plugin-provided servers.
- The meta-server exposes two tools — `list_tools` and `call_tool` — following the progressive disclosure / bounded context pack pattern.
- This avoids context bloat, keeps `.claude/` (and equivalents) clean, and gives Symposium a persistent in-process channel to the agent.

## Motivation

Today, `[[mcp_servers]]` entries in plugins are registered directly into the agent's MCP configuration during `sync`. This has three problems:

1. **Context bloat.** Every registered MCP server's tools are loaded into the agent's context window at startup. A workspace with many plugins could inject dozens of tool schemas the agent never uses.

2. **Config pollution.** Writing entries into `.claude/settings.local.json` (or equivalent) leaves artifacts that are visible to the user, hard to `.gitignore` cleanly, and create merge friction in shared repos.

3. **No return channel.** Once tools are registered, Symposium has no way to communicate with the agent session — for elicitation, status updates, or dynamic capability changes.

A single Symposium-owned MCP server solves all three: one config entry, progressive tool loading, and an always-available communication channel.

## Change in a nutshell

At `cargo agents init` (or `sync`), Symposium registers exactly one MCP server — itself — into the agent's config:

```jsonc
// .claude/settings.local.json (Claude Code example)
{
  "mcpServers": {
    "symposium": {
      "command": "cargo-agents",
      "args": ["mcp-serve"]
    }
  }
}
```

When the agent starts a session, it connects to this server and sees two tools:

```
symposium__list_tools  — List available tools, optionally filtered by domain
symposium__call_tool   — Execute a tool by name
```

The `list_tools` description contains a capability index (the "menu") built from all applicable plugin MCP servers. The agent requests schemas on demand and executes through `call_tool`. Plugin servers are started lazily on first use.

## Detailed plans

### Meta-server architecture

The meta-server is a stdio MCP server implemented in `cargo-agents mcp-serve`. It:

1. Resolves the workspace (same `WorkspaceDeps` logic as sync/hooks).
2. Collects all applicable `[[mcp_servers]]` from the plugin registry.
3. Exposes a two-tool interface following the bounded context pack pattern.

#### The two tools

**`list_tools`**

```
description: |
  List available tools from Symposium plugins.
  
  Domains:
    sqlx: [query, explain, migrate_status]
    sea-orm: [generate_entity, diff_schema]
    ...

parameters:
  domain: string (optional) — filter to a specific domain
  tools: array of strings (optional) — return full schemas for these tools
```

The two parameters are independent:

- `domain` filters the capability index to one domain (returns tool names only).
- `tools` returns full JSON schemas for the named tools (regardless of domain).

When called with no arguments, returns the full structured index. Both can be combined (e.g., "show me the sqlx domain and also give me the schema for `sea-orm__generate_entity`").

**`call_tool`**

```
parameters:
  tool: string — fully qualified tool name (e.g., "sqlx__query")
  input: object — tool-specific parameters
```

Dispatches to the appropriate plugin MCP server. If the backing server isn't running, starts it first (lazy initialization).

#### Lazy server lifecycle

Plugin MCP servers are not started until the agent requests their tool schemas (via `list_tools`) or invokes one of their tools (via `call_tool`). The meta-server maintains a process table:

- **Cold** — server not running, schema loaded from plugin manifest.
- **Starting** — server process spawning, calls queue.
- **Ready** — server running, calls dispatched directly.
- **Dead** — server exited unexpectedly, restart on next call.

Servers are shut down when the meta-server exits (agent session ends).

### Capability index in the description

The `list_tools` description is dynamically generated from applicable plugins. It follows the pattern from the bounded context packs literature:

```
Domains:
  sqlx: [query, explain, migrate_status]
  sea-orm: [generate_entity, diff_schema]
  tokio-console: [list_tasks, task_detail]
```

This means the agent sees the full capability menu just by reading the tool description — no discovery call needed for basic orientation. The model only calls `list_tools` when it needs parameter schemas.

If the index exceeds a reasonable size (TBD, likely ~2000 chars), the description is truncated with a note to call `list_tools` for the full listing.

### Registration mechanics

During `init`/`sync`, Symposium writes a single MCP entry named `"symposium"` pointing to `cargo-agents mcp-serve`. The entry is identified by its well-known name — no additional ownership markers are needed. Individual plugin server entries are never written to agent config.

### Agent compatibility

| Agent | MCP config location | Transport |
|-------|-------------------|-----------|
| Claude Code | `.claude/settings.local.json` | stdio |
| Gemini CLI | `.gemini/settings.json` | stdio |
| Copilot | `.github/copilot-mcp.json` | stdio |
| Codex CLI | `codex.json` | stdio |
| Kiro | `.kiro/mcp.json` | stdio |
| OpenCode | `.opencode/config.json` | stdio |
| Goose | `.goose/mcp.json` | stdio |

All supported agents use stdio transport for local servers, so one implementation covers all.

### Future: elicitation and notifications

Because the meta-server is an always-connected channel, it can also:

- Surface notifications (e.g., "new plugin version available").
- Provide a `symposium__status` resource with sync state.
- Act as an elicitation endpoint if MCP gains that capability, or use sampling requests.

These are out of scope for the initial implementation but inform the architecture.

## Related work

The progressive disclosure pattern for MCP tools is well-established. Our design draws on and is compatible with this landscape.

### Anthropic guidance

- [Advanced Tool Use](https://www.anthropic.com/engineering/advanced-tool-use) — recommends keeping 3–5 tools always loaded, deferring the rest. Reports 85% token reduction and accuracy improvements.
- [Effective Context Engineering for AI Agents](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents) — introduces "just-in-time retrieval": agents maintain lightweight identifiers and load data at runtime via tools.
- [Tool Search Tool](https://platform.claude.com/docs/en/agents-and-tools/tool-use/tool-search-tool) — Anthropic's API-level implementation of progressive disclosure (`defer_loading: true`, BM25/regex search over up to 10,000 tools). Only works via the Claude API, not for MCP-based agent sessions.

### Community MCP aggregators

Several projects have converged on the same "2–4 meta-tools" pattern:

- [mcp-gateway (ViperJuice)](https://github.com/ViperJuice/mcp-gateway) — 26 meta-tools including `catalog_search`, `describe`, `invoke`. 4-step progressive disclosure with on-demand server provisioning.
- [mcp-gateway (MikkoParkkola)](https://github.com/MikkoParkkola/mcp-gateway) — Rust. 4 meta-tools: `gateway_list_servers`, `gateway_list_tools`, `gateway_search_tools` (TF-IDF), `gateway_invoke`. Claims 89% token savings.
- [1MCP](https://github.com/1mcp-app/agent) — unified runtime with 3-step CLI: `instructions`, `inspect`, `run`.
- [NCP](https://github.com/portel-dev/ncp) — 2–3 meta-tools: `find` (vector similarity), `code`, `run`. Claims 97% fewer tokens.
- [MCPProxy-Go](https://github.com/smart-mcp-proxy/mcpproxy-go) — Go proxy with BM25 `retrieve_tools` filtering. Claims 99% token reduction.

### Bounded context packs literature

- [The Meta-Tool Pattern](https://blog.synapticlabs.ai/bounded-context-packs-meta-tool-pattern) — articulates the two-tool discovery+execution pattern and three-layer architecture (meta-tools, domain agents, atomic tools).
- [From Theory to Production](https://blog.synapticlabs.ai/bounded-context-packs-from-theory-to-production) — production walkthrough via the "Nexus" Obsidian plugin. `getTools`/`useTools`, dynamic description-embedded index, schema stripping.

### MCP spec primitives

The [MCP 2025-03-26 spec](https://modelcontextprotocol.io/specification/2025-03-26/server/tools) provides building blocks but no built-in discovery/search mechanism:

- `tools/list` supports cursor-based pagination (for large result sets, not progressive disclosure).
- `notifications/tools/list_changed` lets servers signal that their tool set changed mid-session.

Progressive disclosure must be implemented at the application layer — which is what the meta-server does.

### How Symposium differs

The key differentiator from existing aggregators: the meta-server is **workspace-aware**. It uses crate predicates to determine which plugin servers are applicable, starts them lazily, and integrates with the existing Symposium config/sync lifecycle. No manual server configuration is needed — plugins declare `[[mcp_servers]]` and the meta-server handles the rest.

## Frequently asked questions

### Why not just register plugin servers directly with a `.gitignore`?

Three reasons. First, `.gitignore` patterns for agent config directories (`.claude/`, `.github/`) are coarse — you'd either ignore too much or need per-file patterns that users have to maintain. Second, direct registration means every plugin's tools land in context at startup regardless of whether the agent needs them. Third, direct registration gives Symposium no way to communicate with the agent after initialization.

### Why two tools instead of exposing each plugin tool directly?

Exposing each tool directly through the meta-server would still bloat the context — the agent would see N tool schemas at connection time. The two-tool pattern means the agent sees exactly 2 schemas at startup (~600 tokens) regardless of how many plugin tools exist. It requests schemas on demand.

### What about tool namespacing and conflicts?

Tool names are prefixed with the plugin server name: `sqlx__query`, `sea-orm__generate_entity`. If two plugins declare a server with the same qualified tool name, the first-registered wins and a warning is emitted. The conflicting tool from the later plugin is skipped.

### How does this affect latency?

First call to a cold server pays startup cost (process spawn + MCP handshake). Subsequent calls go directly. For most plugin servers (small Rust binaries), startup is <100ms.

### What about HTTP/SSE backing servers?

The meta-server acts as an MCP client to each backing server using whatever transport that server declares (stdio, HTTP, or SSE). From the agent's perspective it's always stdio — the meta-server bridges the transport gap.

### What if a backing server crashes mid-call?

The meta-server surfaces the error to the agent as-is (the MCP error response from the backing server, or a connection-lost error if it crashed). The server transitions to `Dead` state and is restarted on the next call.

### What if `Cargo.toml` changes mid-session?

The meta-server re-resolves the workspace on `list_tools` calls when `Cargo.lock` mtime has changed (same freshness gate as hooks). This is gated behind the user's `auto-sync` configuration setting — if auto-sync is disabled, the index stays static until the next manual `cargo agents sync`.

### What about the `sync --agent` flow?

`sync --agent` currently writes MCP entries directly. With this change, it writes only the single meta-server entry. The `--agent` flag remains for agents that need explicit sync, but the MCP section of the output shrinks to one entry.

## Implementation plan and status

Each step is independently mergeable and leaves the codebase green.

### Step 1: Register the meta-server entry during `sync` (refactor, new tests)

Change `sync` to write a single `"symposium"` MCP entry (pointing to `cargo-agents mcp-serve`) instead of per-plugin entries. The existing `mcp_server_registration.rs` infrastructure handles the per-agent format differences — we just change the input from the collected plugin servers to one fixed entry. The `mcp-serve` subcommand doesn't exist yet, but registration is just writing config JSON.

This is partly a refactor (removing the per-plugin write path) and partly new behavior (the fixed entry). The existing `sync_filters_mcp_servers_by_crates` test and friends update to assert a single `"symposium"` entry rather than per-plugin entries.

- [ ] Replace per-plugin MCP registration in `sync.rs` with a single `"symposium"` entry
- [ ] Update existing MCP integration tests to expect the new behavior
- [ ] Verify: `cargo test` passes, `.claude/settings.json` contains only `"symposium"` after sync

### Step 2: `mcp-serve` subcommand with hardcoded two-tool skeleton (new tests)

Add `cargo agents mcp-serve` as a new `Commands` variant. It starts a stdio MCP server (via `rmcp`) that advertises `list_tools` and `call_tool` with hardcoded descriptions and returns empty/stub responses. Exits cleanly on stdin EOF.

Integration test: spawn `cargo-agents mcp-serve` as a child process, send MCP `initialize` + `tools/list` JSON-RPC requests over stdin, assert the response contains exactly the two tools with expected names. This validates the transport layer end-to-end.

- [ ] Add `rmcp` dependency
- [ ] Add `McpServe` variant to `Commands`, wire handler
- [ ] Implement stdio MCP server with `list_tools` and `call_tool` stubs
- [ ] Integration test: spawn process, verify JSON-RPC handshake and tool listing

### Step 3: Plugin-driven capability index (new tests)

Wire `list_tools` to the real plugin registry. At startup, the meta-server resolves `WorkspaceDeps` and collects applicable `[[mcp_servers]]` to build the capability index. The `list_tools` description is generated dynamically; calling it with `tools: [...]` returns schemas (which requires starting the backing server).

For this step, only the *index* (tool names per domain) needs to work — schema retrieval can return an error for now.

Integration test: use an existing fixture (e.g., `mcp-filtering0` + `workspace0`), spawn `mcp-serve` in that workspace, call `list_tools` with no args, assert the index contains `always-server` tools and `serde-server` tools but not `missing-crate-server` tools.

- [ ] Resolve workspace and plugin registry at meta-server startup
- [ ] Build capability index from applicable plugin MCP server declarations
- [ ] Generate dynamic `list_tools` description with domain listing
- [ ] Handle `domain` parameter filtering
- [ ] Integration test: verify index reflects workspace-filtered plugins

### Step 4: Lazy server dispatch and `call_tool` (new tests)

Implement the process table and `call_tool` dispatch. When the agent calls `call_tool` (or `list_tools` with `tools: [...]`), the meta-server starts the backing server if cold, waits for MCP handshake, then proxies the request. Support stdio transport first; HTTP/SSE can follow.

Integration test: create a minimal mock MCP server (a small script or binary in the test fixture that responds to `tools/list` and `tools/call`). Spawn `mcp-serve`, call `call_tool` targeting the mock, assert the response passes through correctly. Also test the `Dead → restart` path by having the mock exit after one call.

- [ ] Process table (Cold/Starting/Ready/Dead state machine)
- [ ] MCP client connection to backing servers (stdio first)
- [ ] `call_tool` dispatch: start if cold, proxy request, return response
- [ ] `list_tools` with `tools` parameter: start server, fetch schemas, return
- [ ] Error propagation: surface backing server errors to agent
- [ ] Integration tests: successful dispatch, cold start, crash recovery

### Step 5: Freshness and `auto-sync` gating (new tests)

Re-resolve the workspace on `list_tools` when `Cargo.lock` mtime has changed since last resolution, gated by the `auto-sync` config setting. When auto-sync is off, the index stays static for the session lifetime.

- [ ] Track `Cargo.lock` mtime at startup
- [ ] On `list_tools`, check mtime; if changed and auto-sync enabled, re-resolve
- [ ] Integration test: modify fixture's `Cargo.lock` mid-session, verify index updates
