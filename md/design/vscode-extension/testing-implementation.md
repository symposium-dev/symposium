# Testing Implementation

This chapter documents the testing framework architecture for the VSCode extension, explaining how tests are structured and how to extend the testing system with new capabilities.

## Architecture

### Test Infrastructure

The test suite uses `@vscode/test-cli` which downloads and runs a VSCode instance, loads the extension in development mode, and executes Mocha tests in the extension host context.

Configuration in `.vscode-test.mjs`:
```javascript
{
  files: "out/test/**/*.test.js",
  version: "stable",
  workspaceFolder: "./test-workspace",
  mocha: { ui: "tdd", timeout: 20000 }
}
```

Tests run with:
```bash
npm test
```

### Testing API Design

Rather than coupling tests to implementation details, the extension exposes a command-based testing API. Tests invoke VSCode commands which delegate to public testing methods on `ChatViewProvider`.

**Pattern:**
```typescript
// In extension.ts - register test command
context.subscriptions.push(
  vscode.commands.registerCommand("symposium.test.commandName", 
    async (arg1, arg2) => {
      return await chatProvider.testingMethod(arg1, arg2);
    }
  )
);

// In test - invoke via command
const result = await vscode.commands.executeCommand(
  "symposium.test.commandName", 
  arg1, 
  arg2
);
```

**Current Testing Commands:**
- `symposium.test.simulateNewTab(tabId)` - Create a tab
- `symposium.test.getTabs()` - Get list of tab IDs
- `symposium.test.getQueuedMessages(tabId)` - Inspect indexed outbound messages queued for a tab
- `symposium.test.resetState()` - Clear actors/sessions/message queues between scenarios
- `symposium.test.resetActors()` - Legacy reset hook used by older startup tests
- `symposium.test.sendPrompt(tabId, prompt)` - Send prompt to tab
- `symposium.test.startCapturingResponses(tabId)` - Begin capturing agent responses
- `symposium.test.getResponse(tabId)` - Get accumulated response text
- `symposium.test.stopCapturingResponses(tabId)` - Stop capturing
- `symposium.test.hasActivePrompt(tabId)` - Check whether a prompt is currently in flight

### Startup Watchdog Integration Coverage

Startup failure integration coverage is implemented in
`vscode-extension/src/test/startup-failure-matrix.test.ts`.

The suite drives deterministic startup behavior by replacing the real agent with
`vscode-extension/test-fixtures/fake-startup-agent.cjs`:

1. Set `symposium.acpAgentPath` to the fake agent script.
2. Set `symposium.proxySpawnArgs` to `--startup-scenario=<scenario>`.
3. Force tiny watchdog thresholds for test speed and determinism:
   - `symposium.startupSlowThresholdMs = 100`
   - `symposium.startupHardTimeoutMs = 300`

Assertions are made through first-class extension test hooks rather than UI scraping:

- `symposium.test.resetState` to isolate each scenario.
- `symposium.test.getQueuedMessages` to assert user-visible `agent-error` delivery.
- `logger.onLog` capture to assert ordering (for example, slow-threshold warning before hard-timeout failure).

### Startup Failure Mode Assertion Matrix

| Scenario | Fake-agent behavior | Expected assertions |
| --- | --- | --- |
| `exit` | Process exits during initialize after writing scenario banner to stderr | `agent-stderr` log includes scenario banner; watchdog failure log reason is usually `process-exit`/`stdout-close` (and can be `hard-timeout` under scheduler races); queued `agent-error` contains a startup failure summary |
| `hang` | Process never responds to initialize | Slow-threshold warning log appears first; outbound `agent-startup-slow` chat message is emitted; watchdog failure log follows with reason `hard-timeout`; queued `agent-error` contains a startup failure summary |
| `close` | Process closes stdout during initialize | Watchdog failure log reason is `stdout-close`; queued `agent-error` contains a startup failure summary |
| `acp-error` | Process returns ACP initialize error response | Watchdog failure log reason is `initialize-rejected`; queued `agent-error` includes the initialize rejection summary |

The key contract is that startup failures are asserted through queued chat traffic (`agent-error`) plus structured logs, not through transient notifications. User-facing chat payloads remain concise while logs carry detailed diagnostics.

### Adding New Test Commands

To test new behavior:

1. **Add public method to `ChatViewProvider`** (or relevant class):
```typescript
export class ChatViewProvider {
  // Existing test methods...
  
  public async newTestingMethod(param: string): Promise<ResultType> {
    // Implementation that exposes needed behavior
    return result;
  }
}
```

2. **Register command in `extension.ts`**:
```typescript
context.subscriptions.push(
  vscode.commands.registerCommand(
    "symposium.test.newCommand",
    async (param: string) => {
      return await chatProvider.newTestingMethod(param);
    }
  )
);
```

3. **Use in tests**:
```typescript
test("Should test new behavior", async () => {
  const result = await vscode.commands.executeCommand(
    "symposium.test.newCommand",
    "test-param"
  );
  assert.strictEqual(result.expected, true);
});
```

### Structured Logging for Assertions

Tests verify behavior through structured log events rather than console scraping.

**Logger Architecture:**
```typescript
export class Logger {
  private outputChannel: vscode.OutputChannel;
  private eventEmitter = new vscode.EventEmitter<LogEvent>();
  
  public get onLog(): vscode.Event<LogEvent> {
    return this.eventEmitter.event;
  }
  
  public info(category: string, message: string, data?: any): void {
    const event: LogEvent = { 
      timestamp: new Date(), 
      level: "info", 
      category, 
      message, 
      data 
    };
    this.eventEmitter.fire(event);
    this.outputChannel.appendLine(/* formatted output */);
  }
}
```

**Dual Purpose:**
- **Testing** - Event emitter allows tests to capture and assert on events
- **Live Debugging** - Output channel shows logs in VSCode Output panel

**Usage in Tests:**
```typescript
const logEvents: LogEvent[] = [];
const disposable = logger.onLog((event) => logEvents.push(event));

// ... perform test actions ...

const relevantEvents = logEvents.filter(
  e => e.category === "agent" && e.message === "Session created"
);
assert.strictEqual(relevantEvents.length, 2);
```

### Adding New Log Points

To make behavior testable:

1. **Add log statement in implementation**:
```typescript
logger.info("category", "Descriptive message", {
  relevantData: value,
  moreContext: other
});
```

2. **Filter and assert in tests**:
```typescript
const events = logEvents.filter(
  e => e.category === "category" && e.message === "Descriptive message"
);
assert.ok(events.length > 0);
assert.strictEqual(events[0].data.relevantData, expectedValue);
```

**Log Categories:**
- `webview` - Webview lifecycle events
- `agent` - Agent spawning, sessions, communication
- Add new categories as needed for different subsystems

## Design Decisions

### Command-Based Testing API

**Alternative:** Direct access to `ChatViewProvider` internals from tests

**Chosen:** Command-based testing API

**Rationale:**
- Decouples tests from implementation details
- Tests the same code paths as real usage
- Allows refactoring without breaking tests
- Commands document the testing interface

### Real Agents vs Mocks

**Alternative:** Mock agent responses with canned data

**Chosen:** Hybrid strategy
- Real ACP agent for end-to-end behavior and prompt/stream integration paths
- Deterministic fake startup agent for watchdog/failure-mode matrix coverage

**Rationale:**
- Tests the full protocol stack (JSON-RPC, stdio, conductor)
- Verifies conductor integration
- Catches protocol-level bugs
- Keeps startup-failure assertions deterministic (no flaky timing windows)
- Enables explicit coverage of process-exit/stdout-close/hard-timeout/initialize-rejected paths

### Event-Based Logging

**Alternative:** Console output scraping with regex

**Chosen:** Event emitter with structured data

**Rationale:**
- Enables precise assertions on event counts and data
- Provides rich context for debugging
- Output panel visibility for live debugging
- No brittle string matching
- Same infrastructure serves testing and development

### Test Isolation

**Challenge:** Tests share VSCode instance, agent processes persist across tests

**Strategy:** Make tests order-independent:
- Assert "spawned OR reused" rather than exact counts
- Focus on test-specific events (e.g., prompts sent, responses received)
- Capture logs from test start, not globally
- Don't assume clean state between tests

This allows the test suite to pass regardless of execution order.

## Writing Tests

Tests live in `src/test/*.test.ts` and use Mocha's TDD interface:

```typescript
suite("Feature Tests", () => {
  test("Should do something", async function() {
    this.timeout(20000); // Extend timeout for async operations
    
    // Setup log capture
    const logEvents: LogEvent[] = [];
    const disposable = logger.onLog((event) => logEvents.push(event));
    
    // Perform test actions via commands
    await vscode.commands.executeCommand("symposium.test.doSomething");
    
    // Wait for async completion
    await new Promise(resolve => setTimeout(resolve, 1000));
    
    // Assert on results
    const events = logEvents.filter(/* ... */);
    assert.ok(events.length > 0);
    
    disposable.dispose();
  });
});
```

**Key Patterns:**
- Use `async function()` (not arrow functions) to access `this.timeout()`
- Extend timeout for operations involving agent spawning
- Always dispose log listeners
- Prefer polling helpers over fixed sleeps when asserting asynchronous queue/log state

## Related Documentation

- [Message Protocol](./message-protocol.md) - Extension ↔ webview communication
- [State Persistence](./state-persistence.md) - How state survives webview lifecycle
- [Testing](./testing.md) - Broader VS Code extension testing patterns and startup matrix notes
