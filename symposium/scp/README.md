# SCP - Symposium Component Protocol

A Rust library providing foundational building blocks for implementing the Symposium Component Protocol. Currently provides a generic JSON-RPC 2.0 implementation and ACP (Agent Client Protocol) support for both agents and editors.

## Architecture Overview

The library is structured in three layers:

```
┌─────────────────────────────────────┐
│  ACP Protocol Layer                 │  ← Agent Client Protocol support
│  (acp/agent.rs, acp/editor.rs)     │
├─────────────────────────────────────┤
│  JSON-RPC 2.0 Layer                 │  ← Generic JSON-RPC implementation
│  (jsonrpc.rs, jsonrpc/actors.rs)   │
├─────────────────────────────────────┤
│  Async I/O (tokio, futures)         │  ← Transport layer
└─────────────────────────────────────┘
```

### Design Principles

1. **Layer independence**: The JSON-RPC layer has no knowledge of ACP. You can use it for any JSON-RPC application.
2. **Type safety**: Request/response pairs are statically typed using traits, catching mismatches at compile time.
3. **Handler composition**: Multiple handlers can be chained together, each claiming specific message types.
4. **Actor-based concurrency**: Message processing is split across three cooperating actors to prevent deadlocks.

## JSON-RPC Layer

The JSON-RPC layer provides a generic implementation of the JSON-RPC 2.0 protocol over stdio or any async read/write streams.

### Core Types

- **`JsonRpcConnection<H>`**: Main entry point. Manages bidirectional JSON-RPC communication.
- **`JsonRpcHandler`**: Trait for implementing message handlers.
- **`JsonRpcCx`**: Context for sending requests and notifications.
- **`JsonRpcRequestCx<T>`**: Context for responding to incoming requests.

### Actor Architecture

The connection spawns three actors that cooperate via channels:

```
┌──────────────────┐     ┌──────────────────┐     ┌──────────────────┐
│ Outgoing Actor   │     │ Incoming Actor   │     │ Reply Actor      │
│                  │     │                  │     │                  │
│ Serializes and   │────▶│ Deserializes and │────▶│ Correlates       │
│ writes messages  │     │ routes to        │     │ request IDs with │
│ to transport     │     │ handlers         │     │ response futures │
└──────────────────┘     └──────────────────┘     └──────────────────┘
```

**Why this design?**
- The outgoing actor handles all writes, preventing interleaved JSON.
- The incoming actor routes messages to handlers without blocking on I/O.
- The reply actor manages the map of pending requests, keeping correlation logic centralized.

### Handler Chain Pattern

Handlers use a chain-of-responsibility pattern. When a message arrives, each handler gets a chance to claim it:

```rust
impl JsonRpcHandler for MyHandler {
    async fn handle_request(&mut self, method: &str, params: &Option<Params>,
                           response: JsonRpcRequestCx<Response>)
                           -> Result<Handled<JsonRpcRequestCx<Response>>, Error> {
        if method == "my_method" {
            // Process and respond
            response.respond(MyResponse { ... })?;
            Ok(Handled::Yes)
        } else {
            // Pass to next handler
            Ok(Handled::No(response))
        }
    }
}
```

Handlers are added via `add_handler()` and tried in order until one returns `Handled::Yes`.

### Example: JSON-RPC Echo Server

```rust
use scp::{JsonRpcConnection, JsonRpcHandler, JsonRpcRequestCx, Handled};
use futures::io::{AsyncRead, AsyncWrite};

// Define request/response types
#[derive(serde::Deserialize, serde::Serialize)]
struct EchoRequest { message: String }

#[derive(serde::Deserialize, serde::Serialize)]
struct EchoResponse { echo: String }

impl JsonRpcRequest for EchoRequest {
    type Response = EchoResponse;
    const METHOD: &'static str = "echo";
}

// Implement handler
struct EchoHandler;

impl JsonRpcHandler for EchoHandler {
    async fn handle_request(&mut self, method: &str, params: &Option<jsonrpcmsg::Params>,
                           response: JsonRpcRequestCx<jsonrpcmsg::Response>)
                           -> Result<Handled<JsonRpcRequestCx<jsonrpcmsg::Response>>, acp::Error> {
        if method == "echo" {
            let request: EchoRequest = scp::util::json_cast(params)?;
            response.cast().respond(EchoResponse {
                echo: request.message
            })?;
            Ok(Handled::Yes)
        } else {
            Ok(Handled::No(response))
        }
    }
}

// Run server
#[tokio::main]
async fn main() -> Result<(), acp::Error> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    JsonRpcConnection::new(stdout, stdin)
        .add_handler(EchoHandler)
        .serve()
        .await
}
```

### Example: JSON-RPC Client

```rust
use scp::{JsonRpcConnection, JsonRpcCx};

#[tokio::main]
async fn main() -> Result<(), acp::Error> {
    let (stdin, stdout) = /* your streams */;

    JsonRpcConnection::new(stdout, stdin)
        .with_client(|cx: JsonRpcCx| async move {
            // Send a request
            let response = cx.send_request(EchoRequest {
                message: "hello".to_string()
            }).recv().await?;

            println!("Got echo: {}", response.echo);
            Ok(())
        })
        .await
}
```

The connection serves messages in the background while your client function runs, then cleanly shuts down when the client function returns.

## ACP Protocol Layer

The ACP layer builds on the JSON-RPC foundation to implement the Agent Client Protocol, which defines bidirectional communication between agents and editors.

### Core Types

**For implementing agents:**
- **`AcpAgent<CB>`**: Handler for agent-side messages (requests from editors).
- **`AcpAgentCallbacks`**: Trait you implement to handle requests agents receive.

**For implementing editors:**
- **`AcpEditor<CB>`**: Handler for editor-side messages (requests from agents).
- **`AcpEditorCallbacks`**: Trait you implement to handle requests editors receive.

**For proxies:** Implement both `AcpAgentCallbacks` and `AcpEditorCallbacks` to sit in the middle of the communication chain.

### ACP Protocol Methods

**Agent-side methods** (editors → agents):
- `initialize` - Protocol negotiation and capability exchange
- `authenticate` - Authentication flow
- `session/new` - Create a new agent session
- `session/load` - Load an existing session
- `session/prompt` - Send a user prompt to the agent
- `session/set_mode` - Change session mode
- `session/cancel` - Cancel an in-progress request (notification)

**Editor-side methods** (agents → editors):
- `session/request_permission` - Ask user for permission to execute tools
- `fs/read_text_file` - Read file contents
- `fs/write_text_file` - Write to files
- `terminal/create` - Start a terminal command
- `terminal/output` - Get terminal output
- `terminal/wait_for_exit` - Wait for command completion
- `terminal/kill` - Terminate running command
- `terminal/release` - Release terminal resources
- `session/update` - Stream progress updates (notification)

### Example: Minimal ACP Agent

```rust
use scp::acp::{AcpAgent, AcpAgentCallbacks};
use scp::{JsonRpcConnection, JsonRpcRequestCx};
use agent_client_protocol as acp;

// Implement the callbacks
struct MyAgent;

impl AcpAgentCallbacks for MyAgent {
    async fn initialize(&mut self, args: acp::InitializeRequest,
                       response: JsonRpcRequestCx<acp::InitializeResponse>)
                       -> Result<(), acp::Error> {
        // Advertise capabilities
        response.respond(acp::InitializeResponse {
            capabilities: acp::AgentCapabilities {
                streaming: Some(true),
                ..Default::default()
            },
            ..Default::default()
        })?;
        Ok(())
    }

    async fn prompt(&mut self, args: acp::PromptRequest,
                   response: JsonRpcRequestCx<acp::PromptResponse>)
                   -> Result<(), acp::Error> {
        // Get the prompt text from the request
        let prompt_text = args.prompt.text;

        // Process the prompt (simplified)
        let reply = format!("Echo: {}", prompt_text);

        // Send response
        response.respond(acp::PromptResponse {
            text: Some(reply),
            ..Default::default()
        })?;
        Ok(())
    }

    // Implement other required methods...
    async fn authenticate(&mut self, args: acp::AuthenticateRequest,
                         response: JsonRpcRequestCx<acp::AuthenticateResponse>)
                         -> Result<(), acp::Error> { todo!() }
    async fn session_cancel(&mut self, args: acp::CancelNotification,
                           cx: &JsonRpcCx) -> Result<(), acp::Error> { todo!() }
    async fn new_session(&mut self, args: acp::NewSessionRequest,
                        response: JsonRpcRequestCx<acp::NewSessionResponse>)
                        -> Result<(), acp::Error> { todo!() }
    async fn load_session(&mut self, args: acp::LoadSessionRequest,
                         response: JsonRpcRequestCx<acp::LoadSessionResponse>)
                         -> Result<(), acp::Error> { todo!() }
    async fn set_session_mode(&mut self, args: acp::SetSessionModeRequest,
                              response: JsonRpcRequestCx<acp::SetSessionModeResponse>)
                              -> Result<(), acp::Error> { todo!() }
}

#[tokio::main]
async fn main() -> Result<(), acp::Error> {
    let agent = MyAgent;
    let acp_handler = AcpAgent::new(agent);

    JsonRpcConnection::new(tokio::io::stdout(), tokio::io::stdin())
        .add_handler(acp_handler)
        .serve()
        .await
}
```

### Combining Multiple Handlers

You can chain multiple handlers to extend ACP with custom methods:

```rust
struct CustomHandler;

impl JsonRpcHandler for CustomHandler {
    async fn handle_request(&mut self, method: &str, /* ... */) {
        if method == "custom/my_extension" {
            // Handle your custom method
            Ok(Handled::Yes)
        } else {
            Ok(Handled::No(response))
        }
    }
}

// Chain handlers: try CustomHandler first, then AcpAgent
JsonRpcConnection::new(stdout, stdin)
    .add_handler(CustomHandler)
    .add_handler(AcpAgent::new(my_agent))
    .serve()
    .await
```

This pattern enables the proxy architecture: each proxy can add its own handler to the chain while forwarding unhandled messages downstream.

## Using the Library

### As a JSON-RPC Server

1. Define your request/response types implementing `serde::Serialize` and `serde::Deserialize`
2. Implement `JsonRpcRequest` trait for your request types
3. Create a handler struct implementing `JsonRpcHandler`
4. Build a `JsonRpcConnection` with your handler and call `.serve()`

### As an ACP Agent

1. Create a struct to hold your agent state
2. Implement `AcpAgentCallbacks` trait with your agent logic
3. Wrap it in `AcpAgent::new(your_agent)`
4. Add to a `JsonRpcConnection` and serve
5. Use `AcpAgentExt` trait methods to make requests to the editor:

```rust
use scp::acp::AcpAgentExt;  // Import the extension trait

async fn prompt(&mut self, args: PromptRequest, response: JsonRpcRequestCx<PromptResponse>) {
    let cx = response.json_rpc_cx();

    // Extension trait provides convenient methods
    let content = cx.read_text_file(ReadTextFileRequest {
        path: "src/main.rs".into(),
        ..Default::default()
    }).recv().await?;

    cx.session_update(SessionNotification { /* ... */ })?;
}
```

### As an ACP Editor

1. Create a struct to hold your editor state
2. Implement `AcpEditorCallbacks` trait to handle agent requests
3. Wrap it in `AcpEditor::new(your_editor)`
4. Add to a `JsonRpcConnection` and serve
5. Use `AcpEditorExt` trait methods to make requests to the agent:

```rust
use scp::acp::AcpEditorExt;  // Import the extension trait

async fn read_text_file(&mut self, args: ReadTextFileRequest, response: JsonRpcRequestCx<ReadTextFileResponse>) {
    let cx = response.json_rpc_cx();

    // Extension trait provides convenient methods
    let result = cx.prompt(PromptRequest { /* ... */ }).recv().await?;
}
```

### As an ACP Proxy

Proxies implement both callback traits:
1. Implement `AcpAgentCallbacks` to receive messages from upstream (editor)
2. Implement `AcpEditorCallbacks` to receive messages from downstream (agent)
3. Use `cx.send_request()` to forward and transform messages in both directions
4. Add custom handlers for proxy-specific extensions

## Type Safety Patterns

### Request/Response Correlation

The `JsonRpcRequest` trait ensures responses match requests at compile time:

```rust
impl JsonRpcRequest for MyRequest {
    type Response = MyResponse;  // Compiler enforces this pairing
    const METHOD: &'static str = "my_method";
}

// This works:
let response: MyResponse = cx.send_request(MyRequest { ... }).recv().await?;

// This would be a compile error:
let wrong: OtherResponse = cx.send_request(MyRequest { ... }).recv().await?;
```

### Response Context Casting

When handling generic `jsonrpcmsg::Response`, use `.cast()` to get typed context:

```rust
async fn handle_request(&mut self, method: &str, params: &Option<Params>,
                       response: JsonRpcRequestCx<jsonrpcmsg::Response>) {
    if method == "my_method" {
        let request: MyRequest = json_cast(params)?;

        // Cast to typed response context
        response.cast::<MyResponse>().respond(MyResponse { ... })?;
    }
}
```

## Error Handling

The library uses two error types:

- **`acp::Error`**: JSON-RPC protocol errors (method not found, invalid params, etc.)
- **`acp::Error`**: ACP protocol errors, automatically converted to JSON-RPC errors

Convert ACP errors using the utility function:

```rust
use scp::util::acp_to_jsonrpc_error;

let acp_err = acp::Error { code: 1000, message: "Agent busy".into(), data: None };
let jsonrpc_err = acp_to_jsonrpc_error(acp_err);
```

## Implementation Status

### Complete
- ✅ JSON-RPC 2.0 server and client implementation
- ✅ Actor-based message routing
- ✅ Handler chain pattern
- ✅ ACP agent-side support (handling requests from editors)
- ✅ ACP editor-side support (handling requests from agents)
- ✅ Type-safe request/response correlation

### In Progress
- 🚧 SCP-specific protocol extensions (`_scp/successor/*` messages)

### Planned
- ⏳ Orchestrator binary for managing proxy chains
- ⏳ Reference proxy implementations
- ⏳ Process lifecycle management

## Design Rationale

### Why actors instead of async/await everywhere?

The actor pattern separates concerns and prevents deadlocks that can occur when multiple async tasks try to write to the same transport. The outgoing actor ensures messages are serialized atomically, while the incoming actor can dispatch to handlers without blocking on I/O.

### Why trait-based handlers instead of closures?

Traits enable:
- Stateful handlers that can maintain connections or track sessions
- Handler composition via the chain pattern
- Clear separation between protocol layers
- Testable, modular components

### Why separate the JSON-RPC and ACP layers?

This design allows:
- Reusing the JSON-RPC implementation for non-ACP protocols
- Testing each layer independently
- Adding SCP extensions without modifying ACP support
- Building proxies that layer additional behavior

The JSON-RPC layer is protocol-agnostic and could be extracted as a standalone crate if needed.

## Examples Directory

See the `examples/` directory for complete working examples:
- `echo_server.rs` - Minimal JSON-RPC server
- `echo_client.rs` - JSON-RPC client making requests
- `acp_agent.rs` - Basic ACP agent implementation

## Dependencies

- `agent-client-protocol` - ACP protocol types and definitions
- `jsonrpcmsg` - JSON-RPC message types
- `tokio` - Async runtime
- `futures` - Async utilities and traits
- `serde` / `serde_json` - Serialization

## Contributing

When extending the library:

1. **Keep layers independent**: Changes to JSON-RPC shouldn't require ACP changes
2. **Maintain type safety**: Use traits to enforce compile-time guarantees
3. **Document actor interactions**: Changes to the actor system should document message flows
4. **Add tests**: Unit tests for protocol logic, integration tests for end-to-end flows
