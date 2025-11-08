# Components

Symposium's functionality is delivered through component proxies that are orchestrated by the internal conductor. Some features use a component/adapter pattern while others are standalone components.

## Component Types

### Standalone Components

Some components provide functionality that doesn't depend on upstream capabilities. These components work with any editor and add features purely through the proxy layer.

**Example:** A component that provides git history analysis through MCP tools doesn't need special editor support - it can work with the filesystem directly.

### Component/Adapter Pairs

Other components rely on primitive capabilities from the upstream editor. For these, Symposium uses a two-layer approach:

#### Adapter Layer

The adapter sits upstream in the proxy chain and provides primitive capabilities that the component needs.

**Responsibilities:**
- Check for required capabilities during initialization
- Pass requests through if the editor provides the capability
- Provide fallback implementation if the capability is missing
- Abstract away editor differences from the component

**Example:** The IDE Operations adapter checks if the editor supports `ide_operations`. If not, it can spawn a language server (like rust-analyzer) to provide that capability.

#### Component Layer

The component sits downstream from its adapter and enriches primitive capabilities into higher-level MCP tools.

**Responsibilities:**
- Expose MCP tools to the agent
- Process tool invocations
- Send requests upstream through the adapter
- Return results to the agent

**Example:** The IDE Operations component exposes an `ide_operation` MCP tool that accepts Dialect programs and translates them into IDE operation requests sent upstream.

## Component Lifecycle

For component/adapter pairs:

1. **Initialization** - Adapter receives initialize request from upstream (editor)
2. **Capability Check** - Adapter examines editor capabilities
3. **Conditional Spawning** - Adapter spawns fallback if capability is missing
4. **Chain Assembly** - Conductor wires adapter → component → downstream
5. **Request Flow** - Agent calls MCP tool → component → adapter → editor
6. **Response Flow** - Results flow back: editor → adapter → component → agent

## Proxy Chain Direction

The proxy chain flows from editor to agent:

```
Editor → [Adapter] → [Component] → Agent
```

- **Upstream** = toward the editor
- **Downstream** = toward the agent

Adapters sit closer to the editor, components sit closer to the agent.

## Current Components

### IDE Operations

Provides IDE-aware code navigation and search capabilities through the Dialect language.

- **Adapter:** Checks for `ide_operations` capability, spawns language servers as fallback
- **Component:** Exposes `ide_operation` MCP tool that accepts Dialect programs

See [IDE Operations](./ide_operations.md) for detailed design.

## Future Components

Additional components can be added following these patterns:

- **Walkthroughs** - Interactive code explanations (may need component/adapter pair)
- **Git Operations** - Repository analysis (likely standalone)
- **Build Integration** - Compilation and testing workflows (likely standalone)
