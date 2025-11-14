# Testing Implementation

This chapter documents the testing framework built for the VSCode extension, including the architecture, patterns, and rationale behind key decisions.

## Overview

The test suite provides automated verification of the extension's core functionality without manual testing. Tests use real ACP agents (ElizACP) over actual protocol connections, providing end-to-end validation.

## Architecture

### Test Infrastructure

The test suite uses `@vscode/test-cli` which:
- Downloads and runs a VSCode instance
- Loads the extension in development mode
- Executes Mocha tests in the extension host context

Configuration in `.vscode-test.mjs`:
```javascript
{
  files: "out/test/**/*.test.js",
  version: "stable",
  workspaceFolder: "./test-workspace",
  mocha: { ui: "tdd", timeout: 20000 }
}
```

### Testing API

Rather than coupling tests to implementation details, we expose a clean testing API through VSCode commands:

**Test Commands:**
- `symposium.test.simulateNewTab` - Create a tab
- `symposium.test.getTabs` - Get list of tab IDs
- `symposium.test.sendPrompt` - Send a prompt to a tab
- `symposium.test.startCapturingResponses` - Begin capturing agent responses
- `symposium.test.getResponse` - Get accumulated responses
- `symposium.test.stopCapturingResponses` - Stop capturing

These commands are thin wrappers around `ChatViewProvider` methods:
- `simulateWebviewMessage(message)` - Trigger message handlers
- `getTabsForTesting()` - Inspect tab state
- `startCapturingResponses(tabId)` - Enable response capture
- `getResponse(tabId)` - Retrieve captured text
- `stopCapturingResponses(tabId)` - Disable capture

### Structured Logging

Tests verify behavior through structured log events rather than console output scraping.

**Logger Infrastructure:**
The `Logger` class provides:
- **Output Channel** - Logs visible in VSCode Output panel
- **Event Emitter** - Testable events with structured data
- **Categories** - Namespace for different subsystems (webview, agent)

Example log event:
```typescript
{
  timestamp: Date,
  level: "info" | "warn" | "error",
  category: "agent",
  message: "Agent session created",
  data: {
    tabId: "tab-1",
    agentSessionId: "uuid...",
    agentName: "ElizACP",
    components: ["symposium-acp"]
  }
}
```

Tests capture and assert on events:
```typescript
const logEvents: LogEvent[] = [];
const disposable = logger.onLog((event) => logEvents.push(event));

// ... test actions ...

const agentSpawned = logEvents.filter(
  e => e.category === "agent" && e.message === "Spawning new agent actor"
);
assert.strictEqual(agentSpawned.length, 1);
```

## Test Suites

### Extension Activation Tests

Basic sanity checks that the extension loads:
```typescript
test("Extension should be present", async () => {
  const extension = vscode.extensions.getExtension("symposium.symposium");
  assert.ok(extension);
});

test("Extension should activate", async () => {
  await extension.activate();
  assert.strictEqual(extension.isActive, true);
});
```

### Webview Lifecycle Test

Verifies state persistence across webview hide/show cycles:

**Setup:**
1. Activate extension and open chat view
2. Create a tab (triggers agent spawn and session creation)
3. Verify tab exists

**Hide/Show Cycle:**
1. Switch to Explorer view (hides chat, may dispose webview)
2. Switch back to chat view (shows chat, may recreate webview)
3. Verify tab still exists

**Assertions:**
- Tab persists across hide/show
- Webview hidden/visible events fire
- Agent not respawned (reused)
- Exactly one session created

This test validates the message queue replay mechanism that preserves state when webviews are recreated.

### Conversation Test

End-to-end test of the ACP protocol flow:

**Flow:**
1. Create tab → spawns ElizACP agent
2. Start capturing responses
3. Send prompt: "Hello, how are you?"
4. Wait for response
5. Verify response received

**Assertions:**
- Response length > 0
- Response content is relevant
- Prompt received and sent events logged
- Agent spawned or reused

**Example Response:**
```
ElizACP response: Hello. How are you feeling today?
```

### Multi-Tab Test

Validates conversation isolation and session management:

**Scenario:**
1. Create tab 1 → new agent session
2. Create tab 2 → new session, same agent actor
3. Send "What is your name?" to tab 1
4. Send "Tell me about yourself." to tab 2
5. Send "How are you?" to tab 1
6. Verify responses

**Assertions:**
- 2 separate agent sessions created
- Session IDs are different
- Agent actor reused (spawned once)
- 3 prompts handled correctly
- Responses are independent per tab
- Tab 1 response contains both answers

**Verification of Agent Reuse:**
```
[agent] Spawning new agent actor { ... }
[agent] Reusing existing agent actor { ... }
```

This proves the `AgentConfiguration → AcpAgentActor` mapping works correctly.

## Design Decisions

### Why Message-Based Testing?

**Alternative Considered:** Direct access to ChatViewProvider internals

**Chosen:** Command-based testing API

**Rationale:**
- Decouples tests from implementation details
- Tests the same code paths as real usage
- Allows refactoring without breaking tests
- Commands are self-documenting

### Why Real Agents?

**Alternative Considered:** Mock agent responses

**Chosen:** Real ElizACP over ACP protocol

**Rationale:**
- Tests the full protocol stack
- Verifies conductor integration
- Catches protocol-level bugs
- Provides realistic timing and behavior

ElizACP is:
- Lightweight and deterministic
- Fast (responds in ~1-2 seconds)
- Reliable for testing

### Why Structured Logging?

**Alternative Considered:** Console output scraping with regex

**Chosen:** Event-based logging with structured data

**Rationale:**
- Enables precise assertions on event counts
- Provides rich context for debugging
- Output panel visibility for live debugging
- No brittle string matching

### Test Isolation Strategy

**Problem:** Tests share VSCode instance, agent processes persist

**Solution:** Tests are order-independent:
- Assert "spawned OR reused" rather than exact counts
- Focus on test-specific events (hide/show, prompts)
- Capture logs starting from test, not globally

This allows tests to pass regardless of execution order.

## Running Tests

```bash
npm test
```

Output shows:
- Test results (passing/failing)
- Log events from extension
- Agent communication logs
- Summary statistics per test

## Future Improvements

Potential enhancements to the testing framework:

1. **Test Fixtures** - Pre-configured agent states
2. **Snapshot Testing** - Verify full conversation flows
3. **Performance Tests** - Measure agent spawn time, response latency
4. **Error Injection** - Test failure scenarios (agent crash, timeout)
5. **WebdriverIO Integration** - Test actual webview UI interactions

## Related Documentation

- [Testing Guide](./testing.md) - Comprehensive guide to VSCode extension testing
- [Message Protocol](./message-protocol.md) - Details on extension ↔ webview communication
- [State Persistence](./state-persistence.md) - How state survives webview lifecycle
