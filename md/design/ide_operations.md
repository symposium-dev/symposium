# IDE Operations

The IDE Operations component provides agents with rich code navigation and search capabilities through a domain-specific language called Dialect.

## Overview

IDE Operations enables agents to:
- Find symbol definitions and references
- Search code by pattern (regex)
- Navigate to specific file locations
- Query code structure through LSP-like operations

These capabilities are exposed through a single MCP tool that accepts Dialect programs, making it easy for LLMs to express complex IDE queries in a natural, composable way.

## Architecture

The IDE Operations feature uses the component/adapter pattern:

### IDE Operations Adapter

The adapter sits upstream (closer to the editor) and ensures the `ide_operations` capability is available.

**Capability Detection:**
- Checks for `ide_operations` in editor meta capabilities during initialization
- If present: passes IDE operation requests directly through to the editor
- If missing: provides fallback implementation (e.g., spawns rust-analyzer)

**Fallback Strategy (Future):**
- Detect language from workspace
- Spawn appropriate language server
- Proxy LSP requests to provide IDE operations capability

### IDE Operations Component

The component sits downstream (closer to the agent) and provides the `ide_operation` MCP tool to agents.

**Tool Signature:**
```
ide_operation(program: string) -> Value
```

**Responsibilities:**
- Parse and validate Dialect programs
- Execute Dialect functions through an interpreter
- Send IDE queries upstream through the adapter
- Return structured results to the agent

## The Dialect Language

Dialect is a simple functional language for expressing IDE queries. It's designed to be LLM-friendly while remaining precise enough to execute reliably.

### Core Functions

**Symbol Navigation:**
```
findDefinitions(`MyClass`)      // Find where MyClass is defined
findDefinition(`MyClass`)       // Alias for findDefinitions
findReferences(`validateToken`) // Find all uses of validateToken
```

**Code Search:**
```
search(`src/auth.rs`, `async fn`)           // Search file for pattern
search(`src`, `struct.*User`, `.rs`)        // Search directory for pattern in .rs files
search(`tests`, `#\[test\]`)                // Search all files in directory
```

**Location Targeting:**
```
lines(`src/main.rs`, 10, 15)    // Target specific line range
```

### Composability

Dialect functions can be composed and used as arguments to other functions:

```
// Find all references to the validateToken function
findReferences(findDefinition(`validateToken`))
```

### Return Values

Dialect functions return structured JSON values:

**Symbol Definitions:**
```json
{
  "name": "MyClass",
  "kind": "class",
  "definedAt": {
    "path": "src/models.rs",
    "start": {"line": 42, "column": 1},
    "end": {"line": 55, "column": 2}
  }
}
```

**File Ranges:**
```json
{
  "path": "src/auth.rs",
  "start": {"line": 10, "column": 1},
  "end": {"line": 15, "column": 20}
}
```

## Message Flow

### Tool Call from Agent

1. Agent calls `ide_operation` with Dialect program
2. Component parses and begins executing the program
3. Component sends IDE operation request upstream through the adapter
4. Adapter forwards to editor (or executes fallback)
5. Results flow back through the chain to the agent

### Example Flow

```
Agent: ide_operation("findDefinition(`User`)")
  ↓
Component: Parse "findDefinition(`User`)"
  ↓
Component → Adapter: IDE query request (upstream)
  ↓
Adapter → Editor: LSP-style query
  ↓
Editor: Returns definition location
  ↓
Adapter → Component: Forwards result
  ↓
Component: Formats as Dialect result
  ↓
Agent: Receives structured symbol definition
```

## Editor Requirements

Editors that want to provide native IDE operations support must:

1. **Advertise Capability:**
   Include `ide_operations` in meta capabilities during initialization

2. **Handle IDE Requests:**
   Accept and process IDE operation requests (specification TBD)

3. **Return Structured Results:**
   Respond with Dialect-compatible JSON structures

## Implementation Notes

### Dialect Interpreter

The component includes a Dialect interpreter that:
- Parses Dialect syntax into an AST
- Executes functions with proper type checking
- Manages async execution for IDE queries
- Handles errors gracefully

### Language Server Fallback

When the editor doesn't provide IDE operations, the adapter can:
- Detect project language (e.g., Rust via Cargo.toml)
- Spawn appropriate language server (e.g., rust-analyzer)
- Proxy LSP requests to fulfill IDE operations
- Manage language server lifecycle

This fallback ensures IDE operations work even with basic ACP editors.
