# MCP meta-server for progressive tool disclosure

## TL;DR

- Instead of writing plugin MCP servers directly into agent config, Symposium runs a single "meta" MCP server that gates access to all plugin-provided servers.
- The meta-server exposes two tools — `list_tools` and `execute` — following the progressive disclosure pattern.
- `list_tools` returns TypeScript declarations describing available tools. `execute` runs a JavaScript program with those tools available as global functions.
- This avoids context bloat, keeps `.claude/` (and equivalents) clean, gives Symposium a persistent in-process channel to the agent, and lets the agent compose multi-step tool workflows in a single round trip.

## Motivation

Today, `[[mcp_servers]]` entries in plugins are registered directly into the agent's MCP configuration during `sync`. This has three problems:

1. **Context bloat.** Every registered MCP server's tools are loaded into the agent's context window at startup. A workspace with many plugins could inject dozens of tool schemas the agent never uses.

2. **Config pollution.** Writing entries into `.claude/settings.local.json` (or equivalent) leaves artifacts that are visible to the user, hard to `.gitignore` cleanly, and create merge friction in shared repos.

3. **No return channel.** Once tools are registered, Symposium has no way to communicate with the agent session — for elicitation, status updates, or dynamic capability changes.

A single Symposium-owned MCP server solves all three: one config entry, progressive tool loading, and an always-available communication channel. Adding code execution on top eliminates the "one round trip per tool call" bottleneck.

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
symposium__list_tools  — Show available tools as TypeScript declarations
symposium__execute     — Run a JavaScript program with tools as globals
```

The `list_tools` description contains a capability index (the "menu") built from all applicable plugin MCP servers. When the agent needs details, it calls `list_tools` and receives TypeScript declarations. It then writes a JavaScript program that calls those functions and passes it to `execute`.

### Example flow

The agent sees this in the `list_tools` description:

```
Available servers: sqlx, sea_orm
Call list_tools for full declarations.
```

It calls `list_tools({ servers: ["sqlx"] })` and gets:

```typescript
declare namespace sqlx {
  /** Execute a SQL query and return rows */
  function query(params: { sql: string; params?: any[] }): any;
  /** Explain a query plan */
  function explain(params: { sql: string }): any;
  /** Show pending and applied migrations */
  function migrate_status(): any;
}
```

It then calls `execute` with a JavaScript program:

```javascript
const users = await sqlx.query({ sql: "SELECT id, name FROM users WHERE active = $1", params: [true] });
const tables = users.rows.map(u => u.table_name);
const entities = [];
for (const t of tables) {
  entities.push(await sea_orm.generate_entity({ table: t }));
}
return entities.filter(e => e.code.includes("DateTime"));
```

The meta-server runs this in a sandboxed JS interpreter, dispatching `sqlx.query(...)` and `sea_orm.generate_entity(...)` to the respective backing MCP servers, and returns the final value to the agent.

## Detailed plans

### Meta-server architecture

The meta-server is a stdio MCP server implemented in `cargo-agents mcp-serve`. It:

1. Resolves the workspace (same `WorkspaceDeps` logic as sync/hooks).
2. Collects all applicable `[[mcp_servers]]` from the plugin registry.
3. Exposes two tools: `list_tools` and `execute`.
4. Embeds a JavaScript interpreter for executing agent-submitted programs.

#### The two tools

**`list_tools`**

```
description: |
  List available tools as TypeScript declarations.
  
  Available servers: sqlx, sea_orm, tokio_console
  Call list_tools for full declarations.

parameters:
  servers: array of strings (optional) — which servers to show (default: all)
```

Returns TypeScript declarations for the requested servers. When called with no arguments, returns declarations for all available servers.

**`execute`**

```
parameters:
  script: string — JavaScript program to run
```

Runs the script in a sandboxed JS interpreter. Each MCP server is exposed as a namespace object on the global scope (e.g., `sqlx`, `sea_orm`). Tool functions within each namespace are async — the script should use `await`. The return value of the script (last expression or explicit `return`) is serialized as JSON and returned to the agent.

#### TypeScript declarations from MCP schemas

MCP tool schemas are JSON Schema with `type: "object"` at the root. The conversion to TypeScript declarations is mechanical:

| JSON Schema | TypeScript |
|-------------|-----------|
| `{ "type": "string" }` | `string` |
| `{ "type": "number" }` or `"integer"` | `number` |
| `{ "type": "boolean" }` | `boolean` |
| `{ "type": "array", "items": T }` | `T[]` |
| `{ "type": "object", "properties": {...} }` | `{ field: T; ... }` |
| `{ "enum": ["a", "b"] }` | `"a" \| "b"` |
| not in `required` | `field?: T` |
| anything else | `any` |

Return types are `any` since the MCP spec (2025-03-26) does not type tool outputs. If a server declares `outputSchema` (2025-11-25 spec), we can generate a return type from it.

Descriptions from the JSON Schema `description` field become JSDoc comments on the declaration.

#### JavaScript execution engine

The meta-server embeds a lightweight JS engine. Two candidates:

- **rquickjs** — Rust bindings to QuickJS. ~500KB, sub-ms startup, ES2020, easy host function registration.
- **Boa** — pure Rust. No C dependency, still maturing on spec compliance.

We start with rquickjs (better spec compliance, proven in production). The sandbox exposes only the MCP tool namespaces — no filesystem, network, or other ambient capabilities.

Each namespace function is registered as a host-backed async function. When the script calls `await sqlx.query(...)`, the engine suspends, the meta-server dispatches to the backing MCP server, and resumes the script with the result.

#### Lazy server lifecycle

Plugin MCP servers are not started until the agent requests their declarations (via `list_tools`) or executes a script that calls one of their tools. The meta-server maintains a process table:

- **Cold** — server not running, tool list known from plugin manifest.
- **Starting** — server process spawning, calls queue.
- **Ready** — server running, calls dispatched directly.
- **Dead** — server exited unexpectedly, restart on next call.

Servers are shut down when the meta-server exits (agent session ends).

### Capability index in the description

The `list_tools` description is dynamically generated from applicable plugins:

```
Available servers: sqlx, sea_orm, tokio_console
Call list_tools for full TypeScript declarations.
```

This gives the agent orientation without consuming tokens on full schemas. The model calls `list_tools` when it needs the actual function signatures.

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
- [Code Execution with MCP](https://www.anthropic.com/engineering/code-execution-with-mcp) — proposes presenting MCP tools as code APIs rather than direct tool calls. The agent writes programs that compose tools, filter intermediate results, and use control flow — all in one execution. Reports 98.7% token reduction. This directly informs our `execute` tool design.
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
- `inputSchema` is always JSON Schema with `type: "object"` at the root — straightforward to convert to TypeScript declarations.

Progressive disclosure must be implemented at the application layer — which is what the meta-server does.

### How Symposium differs

The key differentiator from existing aggregators: the meta-server is **workspace-aware** and uses **code execution** rather than one-call-at-a-time proxying. It uses crate predicates to determine which plugin servers are applicable, starts them lazily, presents their schemas as TypeScript declarations, and lets the agent compose multi-step workflows in a single `execute` call. No manual server configuration is needed — plugins declare `[[mcp_servers]]` and the meta-server handles the rest.

## Frequently asked questions

### Why not just register plugin servers directly with a `.gitignore`?

Three reasons. First, `.gitignore` patterns for agent config directories (`.claude/`, `.github/`) are coarse — you'd either ignore too much or need per-file patterns that users have to maintain. Second, direct registration means every plugin's tools land in context at startup regardless of whether the agent needs them. Third, direct registration gives Symposium no way to communicate with the agent after initialization.

### Why `execute` instead of a simple `call_tool`?

A `call_tool` proxy still requires one round trip per tool invocation. For multi-step workflows (query a database, filter results, pass them to another tool), the agent must go back and forth with the meta-server for each step, paying latency and token cost each time. With `execute`, the agent writes a short program that composes multiple calls, filters intermediate data, and uses control flow — all in a single invocation. Intermediate results never enter the agent's context unless explicitly returned. See [Code Execution with MCP](https://www.anthropic.com/engineering/code-execution-with-mcp) for the detailed rationale.

### Why TypeScript declarations instead of JSON Schema?

Models have seen millions of TypeScript type definitions in training. `.d.ts` syntax is the most token-efficient, highest-fidelity way for a model to understand a function's signature. JSON Schema is verbose and less directly actionable — the model would have to mentally convert it before writing code anyway.

### Why JavaScript (QuickJS) instead of Rhai or Lua?

Models produce correct JavaScript at extremely high rates — it's by far the most represented language in training data. JSON is a literal in the language, so there's no serialization ceremony. QuickJS provides ES2020 compliance in ~500KB with sub-millisecond startup. Rhai is Rust-native but less familiar to models; Lua is lightweight but requires explicit JSON handling.

### What about tool namespacing and conflicts?

Each MCP server becomes a namespace: `sqlx.query(...)`, `sea_orm.generate_entity(...)`. If two plugins declare a server with the same name, the first-registered wins and a warning is emitted.

### How does this affect latency?

First call to a cold server pays startup cost (process spawn + MCP handshake). Subsequent calls go directly. For most plugin servers (small Rust binaries), startup is <100ms. The JS interpreter itself is sub-millisecond startup.

### What about HTTP/SSE backing servers?

The meta-server acts as an MCP client to each backing server using whatever transport that server declares (stdio, HTTP, or SSE). From the agent's perspective it's always stdio — the meta-server bridges the transport gap.

### What if a backing server crashes mid-execution?

The meta-server surfaces the error as a JavaScript exception within the script. If the script doesn't catch it, the `execute` call returns an error with the exception message. The server transitions to `Dead` state and is restarted on the next call.

### What if `Cargo.toml` changes mid-session?

The meta-server re-resolves the workspace on `list_tools` calls when `Cargo.lock` mtime has changed (same freshness gate as hooks). This is gated behind the user's `auto-sync` configuration setting — if auto-sync is disabled, the index stays static until the next manual `cargo agents sync`.

### Is there a script size or execution time limit?

Yes. Scripts are limited to a configurable timeout (default: 30s) and the JS engine runs with bounded memory. These limits prevent runaway loops from hanging the agent session.

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

### Step 2: `mcp-serve` subcommand with two-tool skeleton (new tests)

Add `cargo agents mcp-serve` as a new `Commands` variant. It starts a stdio MCP server (via `rmcp`) that advertises `list_tools` and `execute` with hardcoded descriptions and returns empty/stub responses. Exits cleanly on stdin EOF.

Integration test: spawn `cargo-agents mcp-serve` as a child process, send MCP `initialize` + `tools/list` JSON-RPC requests over stdin, assert the response contains exactly the two tools with expected names.

- [ ] Add `rmcp` dependency
- [ ] Add `McpServe` variant to `Commands`, wire handler
- [ ] Implement stdio MCP server with `list_tools` and `execute` stubs
- [ ] Integration test: spawn process, verify JSON-RPC handshake and tool listing

### Step 3: Plugin-driven `list_tools` with TypeScript generation (new tests)

Wire `list_tools` to the real plugin registry. At startup, the meta-server resolves `WorkspaceDeps` and collects applicable `[[mcp_servers]]`. Calling `list_tools` starts the relevant backing servers, fetches their `tools/list` schemas, converts them to TypeScript declarations, and returns the result.

Integration test: use an existing fixture (e.g., `mcp-filtering0` + `workspace0`), spawn `mcp-serve` in that workspace, call `list_tools`, assert the response contains TypeScript declarations for `always-server` tools but not `missing-crate-server` tools.

- [ ] Resolve workspace and plugin registry at meta-server startup
- [ ] Implement JSON Schema → TypeScript declaration conversion
- [ ] On `list_tools`, start backing servers and fetch their tool schemas
- [ ] Generate and return TypeScript declarations grouped by namespace
- [ ] Integration test: verify declarations reflect workspace-filtered plugins
- [ ] Unit tests: JSON Schema → TypeScript conversion for common schema patterns

### Step 4: `execute` with embedded JS engine (new tests)

Embed rquickjs (QuickJS). Register each backing server's tools as async functions on namespace globals. The `execute` tool runs the agent-submitted script, dispatching tool calls to backing servers, and returns the final value.

Integration test: create a minimal mock MCP server (a small script in the fixture that responds to `tools/list` and `tools/call`). Spawn `mcp-serve`, call `execute` with a script that calls the mock, assert the return value passes through correctly. Test error propagation by having the mock return an error.

- [ ] Add `rquickjs` dependency
- [ ] Register namespace globals from backing server tool lists
- [ ] Implement async dispatch: JS `await` → MCP `tools/call` → resume
- [ ] Return script result as JSON to agent
- [ ] Timeout and memory limits
- [ ] Integration tests: successful execution, multi-call scripts, error propagation

### Step 5: Freshness and `auto-sync` gating (new tests)

Re-resolve the workspace on `list_tools` when `Cargo.lock` mtime has changed since last resolution, gated by the `auto-sync` config setting. When auto-sync is off, the index stays static for the session lifetime.

- [ ] Track `Cargo.lock` mtime at startup
- [ ] On `list_tools`, check mtime; if changed and auto-sync enabled, re-resolve
- [ ] Integration test: modify fixture's `Cargo.lock` mid-session, verify index updates
