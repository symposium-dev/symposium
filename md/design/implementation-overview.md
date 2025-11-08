# Implementation Overview

Symposium appears to external clients as a single ACP proxy, but internally uses a **conductor** to orchestrate a dynamic chain of component proxies. This architecture allows Symposium to adapt to different client capabilities and provide consistent functionality regardless of what the editor or agent natively supports.

## Architecture

### External View

From the outside, Symposium is a standard ACP proxy that sits between an editor and an agent:

```mermaid
flowchart LR
    Editor --> Symposium --> Agent
```

### Internal Structure

Internally, Symposium runs a [conductor](https://symposium-dev.github.io/symposium-acp/) in proxy mode that orchestrates multiple component proxies:

```mermaid
flowchart LR
    Editor --> S[Symposium Conductor]
    S --> C1[Component 1]
    C1 --> A1[Adapter 1]
    A1 --> C2[Component 2]
    C2 --> Agent
```

The conductor dynamically builds this chain based on what capabilities the editor and agent provide.

## Component Pattern

Some Symposium features are implemented as **component/adapter pairs**:

### Components

Components provide functionality to agents through MCP tools and other mechanisms. They:
- Expose high-level capabilities (e.g., Dialect-based IDE operations)
- May rely on primitive capabilities from upstream (the editor)
- Are always included in the chain when their functionality is relevant

### Adapters

Adapters "shim" for missing primitive capabilities by providing fallback implementations. They:
- Check whether required primitive capabilities exist upstream
- Provide the capability if it's missing (e.g., spawn rust-analyzer to provide IDE operations)
- Pass through transparently if the capability already exists
- Are conditionally included only when needed

## Capability-Driven Assembly

During initialization, Symposium:

1. **Receives capabilities from the editor** - examines what the upstream client provides
2. **Queries the agent** - discovers what capabilities the downstream agent supports
3. **Builds the proxy chain** - spawns components and adapters based on detected gaps and opportunities
4. **Advertises enriched capabilities** - tells the editor what the complete chain provides

This approach allows Symposium to work with minimal ACP clients (by providing fallback implementations) while taking advantage of native capabilities when available (by passing through directly).

For detailed information about the initialization sequence and capability negotiation, see [Initialization Sequence](./initialization-sequence.md).

## Example: IDE Operations

The IDE Operations feature demonstrates the component/adapter pattern:

**If the editor provides primitive IDE operations:**
```
Editor → Symposium → IDE Ops Component → Agent
         (conductor)  (enriches to MCP tools)
```

**If the editor lacks IDE operations:**
```
Editor → Symposium → IDE Ops Adapter → IDE Ops Component → Agent
         (conductor)  (spawns LSP)      (enriches to MCP tools)
```

In both cases, the agent receives the same `ide_operation` MCP tool, but the path to providing it differs based on what the editor supports.
