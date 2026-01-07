# VS Code Language Model Tool Rejection Handling

This reference documents how VS Code handles tool rejection in the Language Model API.

## Tool Call Timing: Why Providers Can't Detect Rejection

**Tool calls are processed after providers return, not during streaming.** When a `LanguageModelChatProvider` emits a `LanguageModelToolCallPart` via `progress.report()`, VS Code does not process it immediately. Instead:

```typescript
// VS Code's internal consumption pattern
const toolCalls: LanguageModelToolCallPart[] = [];

for await (const part of response.stream) {
    if (part instanceof LanguageModelTextPart) {
        stream.markdown(part.value);  // Text streams immediately to UI
    } else if (part instanceof LanguageModelToolCallPart) {
        toolCalls.push(part);  // Tool calls are BUFFERED, not processed
    }
}

// Only AFTER stream completes: process collected tool calls
if (toolCalls.length > 0) {
    await processToolCalls(toolCalls);  // Confirmation UI appears HERE
}
```

The temporal sequence:

1. Provider emits `ToolCallPart` via `progress.report()`
2. Provider continues executing or returns
3. **Only then**: VS Code shows confirmation UI
4. User accepts or rejects
5. If rejected: the chat session cancels entirely

This means:
- **You cannot block inside `provideLanguageModelChatResponse`** waiting for the tool decision
- **The `CancellationToken` cannot detect tool rejection** during execution, because rejection happens after your method returns
- **You must use history matching** to detect approval on subsequent requests

### Detecting Approval via History

On approval, the next `provideLanguageModelChatResponse` call includes:

1. An **Assistant message** with your `ToolCallPart`
2. A **User message** with a `ToolResultPart` containing the matching `callId`

```typescript
for (const msg of messages) {
    if (msg.role === 'user') {
        for (const part of msg.content) {
            if (part instanceof LanguageModelToolResultPart) {
                if (part.callId === yourPreviousToolCallId) {
                    // User approved - tool was invoked
                }
            }
        }
    }
}
```

On rejection, the chat session cancels - you won't receive a follow-up request at all.

## Consumer Perspective: `invokeTool()` on Rejection

**It throws an exception.** When the user clicks "Cancel" on the confirmation dialog, `invokeTool()` rejects with a `CancellationError`:

```typescript
try {
  const result = await vscode.lm.invokeTool(call.name, {
    input: call.input,
    toolInvocationToken: request.toolInvocationToken
  }, token);
} catch (err) {
  if (err instanceof vscode.CancellationError) {
    // User declined the tool confirmation
  }
}
```

There is no special "rejected" result object - rejection is purely via exception.

## Critical Limitation: Rejection Cancels Entire Chat

When a user hits "Cancel" on a tool confirmation, the whole chat gets cancelled - not just that individual tool invocation. This is a documented behavioral issue ([GitHub Issue #241039](https://github.com/microsoft/vscode/issues/241039)).

The expected behavior would be that a cancelled tool call responds to the LLM with an error message for that specific tool, allowing the LLM to reason based on received results. Currently, this doesn't happen.

### Provider Perspective

If you're a `LanguageModelChatProvider` that emitted a `LanguageModelToolCallPart`:

- You don't receive a signal in the next request's message history
- The entire request chain is terminated via cancellation
- There's no opportunity to continue with partial results

## Cancellation vs. Rejection: No Distinction

Both user rejection (clicking "Cancel" on confirmation) and user cancellation (stopping the entire chat) surface identically as `CancellationError`. The API provides no way to distinguish between:

- User rejected this specific tool but wants to continue the chat
- User cancelled the entire request

## What Happens After Cancellation

### History After Rejection

The cancelled turn does NOT appear in history:

- `ChatResponseTurn` entries only exist for completed responses
- If the handler threw/rejected (due to cancellation), there's no `ChatResult`
- The user's prompt (`ChatRequestTurn`) does appear, but with no corresponding response

So the history looks like:

```
Turn 1: User prompt → "Help me edit this file"
Turn 1: Assistant response → [MISSING - cancelled]
Turn 2: User prompt → "Try a different approach"
```

### What the Model Sees on Follow-up

When the user sends a follow-up after rejection:

**What the model sees:**
- The original user request
- NO assistant response for that turn (it was cancelled)
- The new user message

**What the model does NOT see:**
- The tool call it attempted
- Any partial text streamed before the tool call
- The fact that there was a rejection at all

This means the tool call effectively "never happened" from the model's perspective.

## Summary

| Scenario | API Behavior | Chat continues? | In history? |
|----------|--------------|-----------------|-------------|
| User approves tool | `invokeTool()` resolves with result | Yes | Yes |
| User rejects tool | `invokeTool()` throws `CancellationError` | **No** | **No** |
| User cancels entire chat | `CancellationToken` triggered | No | No |

## Key Takeaways

1. **No partial execution:** Cannot reject some tools while accepting others
2. **No rejection signaling:** Model doesn't know a tool was rejected
3. **Clean slate on retry:** The cancelled turn disappears from history
4. **Exception-based flow:** All rejections surface as `CancellationError`

## References

- [GitHub Issue #241039 - Tool cancellation cancels entire chat](https://github.com/microsoft/vscode/issues/241039)
- [GitHub Issue #213274 - Chat tools API proposal](https://github.com/microsoft/vscode/issues/213274)
