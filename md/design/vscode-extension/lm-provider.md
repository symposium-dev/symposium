# Language Model Provider

This chapter describes the architecture for exposing ACP agents as VS Code Language Models via the `LanguageModelChatProvider` API (introduced in VS Code 1.104). This allows ACP agents to appear in VS Code's model picker and be used by any extension that consumes the Language Model API.

## Overview

The Language Model Provider bridges VS Code's stateless Language Model API to ACP's stateful session model. When users select "Symposium" in the model picker, requests are routed through Symposium to the configured ACP agent.

```
┌─────────────────────────────────────────────────────────────────┐
│                         VS Code                                 │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │              Language Model Consumer                       │  │
│  │         (Copilot, other extensions, etc.)                 │  │
│  └─────────────────────────┬─────────────────────────────────┘  │
│                            │                                    │
│                            ▼                                    │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │           LanguageModelChatProvider (TypeScript)          │  │
│  │                                                           │  │
│  │  - Thin adapter layer                                     │  │
│  │  - Serializes VS Code API calls to JSON-RPC               │  │
│  │  - Forwards to Rust process                               │  │
│  │  - Deserializes responses, streams back via progress      │  │
│  └─────────────────────────┬─────────────────────────────────┘  │
└────────────────────────────┼────────────────────────────────────┘
                             │ JSON-RPC (stdio)
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│              symposium-acp-agent vscodelm                       │
│                                                                 │
│  - Receives serialized VS Code LM API calls                    │
│  - Manages session state                                        │
│  - Routes to ACP agent (or Eliza for prototype)                │
│  - Streams responses back                                       │
└─────────────────────────────────────────────────────────────────┘
```

## Design Decisions

### TypeScript/Rust Split

The TypeScript extension is a thin adapter:
- Registers as `LanguageModelChatProvider`
- Serializes `provideLanguageModelChatResponse` calls to JSON-RPC
- Sends to Rust process over stdio
- Deserializes responses and streams back via `progress` callback

The Rust process handles all logic:
- Session management
- Message history tracking
- ACP protocol (future)
- Response streaming

This keeps the interesting logic in Rust where it's testable and maintainable.

### Session Management

VS Code's Language Model API is stateless: each `sendRequest` includes the full message history. ACP sessions are stateful.

For the prototype, we take a simple approach:
- Single user message → create new session
- Message history matching → continue existing session
- Mismatch → return error

Future work will implement proper session caching with history diffing.

### Prototype Scope

The initial implementation uses `elizacp::eliza::Eliza` instead of a real ACP agent. This allows us to:
- Validate the JSON-RPC protocol between TypeScript and Rust
- Exercise the streaming response path
- Test the `LanguageModelChatProvider` integration

Once the plumbing works, we'll replace Eliza with actual ACP session management.

## JSON-RPC Protocol

The protocol between TypeScript and Rust mirrors the `LanguageModelChatProvider` interface.

### Requests (TypeScript → Rust)

**`lm/provideLanguageModelChatResponse`**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "lm/provideLanguageModelChatResponse",
  "params": {
    "messages": [
      { "role": "user", "content": [{ "type": "text", "value": "Hello" }] }
    ]
  }
}
```

### Notifications (Rust → TypeScript)

**`lm/responsePart`** - Streams response chunks
```json
{
  "jsonrpc": "2.0",
  "method": "lm/responsePart",
  "params": {
    "requestId": 1,
    "part": { "type": "text", "value": "How " }
  }
}
```

**`lm/responseComplete`** - Signals end of response
```json
{
  "jsonrpc": "2.0",
  "method": "lm/responseComplete",
  "params": {
    "requestId": 1
  }
}
```

### Response

After all parts are streamed, the request completes:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {}
}
```

## Implementation Status

- [ ] Rust: `vscodelm` subcommand in symposium-acp-agent
- [ ] Rust: JSON-RPC message parsing
- [ ] Rust: Eliza integration for prototype
- [ ] Rust: Response streaming
- [ ] TypeScript: LanguageModelChatProvider registration
- [ ] TypeScript: JSON-RPC client over stdio
- [ ] TypeScript: Progress callback integration
- [ ] End-to-end test with model picker

## Future Work

- Session caching with message history diffing
- Full ACP agent integration
- MCP-over-ACP tool bridging
- Token counting heuristics
- Model metadata from agent capabilities
- Cancellation support
