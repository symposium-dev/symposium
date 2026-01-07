# VS Code Language Model Tool API

This reference documents VS Code's Language Model Tool API (1.104+), which enables extensions to contribute callable tools that LLMs can invoke during chat interactions.

## Tool Registration

Tools require dual registration: static declaration in `package.json` and runtime registration via `vscode.lm.registerTool()`.

### package.json Declaration

```json
{
  "contributes": {
    "languageModelTools": [{
      "name": "myext_searchFiles",
      "displayName": "Search Files",
      "toolReferenceName": "searchFiles",
      "canBeReferencedInPrompt": true,
      "modelDescription": "Searches workspace files matching a pattern",
      "userDescription": "Search for files in the workspace",
      "icon": "$(search)",
      "inputSchema": {
        "type": "object",
        "properties": {
          "pattern": { "type": "string", "description": "Glob pattern to match" },
          "maxResults": { "type": "number", "default": 10 }
        },
        "required": ["pattern"]
      },
      "when": "workspaceFolderCount > 0"
    }]
  }
}
```

### Runtime Registration

```typescript
interface LanguageModelTool<T> {
  invoke(
    options: LanguageModelToolInvocationOptions<T>,
    token: CancellationToken
  ): ProviderResult<LanguageModelToolResult>;

  prepareInvocation?(
    options: LanguageModelToolInvocationPrepareOptions<T>,
    token: CancellationToken
  ): ProviderResult<PreparedToolInvocation>;
}

// Registration in activate()
export function activate(context: vscode.ExtensionContext) {
  context.subscriptions.push(
    vscode.lm.registerTool('myext_searchFiles', new SearchFilesTool())
  );
}
```

## Tool Call Flow

The model provider streams `LanguageModelToolCallPart` objects, and the consumer handles invocation and result feeding.

### Sequence

1. Model receives prompt and tool definitions
2. Model generates `LanguageModelToolCallPart` objects with parameters
3. VS Code presents confirmation UI
4. Consumer invokes `vscode.lm.invokeTool()`
5. Results wrap in `LanguageModelToolResultPart`
6. New request includes results for model's next response

### Key Types

```typescript
class LanguageModelToolCallPart {
  callId: string;   // Unique ID to match with results
  name: string;     // Tool name to invoke
  input: object;    // LLM-generated parameters
}

class LanguageModelToolResultPart {
  callId: string;  // Must match LanguageModelToolCallPart.callId
  content: Array<LanguageModelTextPart | LanguageModelPromptTsxPart | unknown>;
}
```

### Consumer Tool Loop

```typescript
async function handleWithTools(
  model: vscode.LanguageModelChat,
  messages: vscode.LanguageModelChatMessage[],
  token: vscode.CancellationToken
) {
  const options: vscode.LanguageModelChatRequestOptions = {
    tools: vscode.lm.tools.map(t => ({
      name: t.name,
      description: t.description,
      inputSchema: t.inputSchema ?? {}
    })),
    toolMode: vscode.LanguageModelChatToolMode.Auto
  };

  while (true) {
    const response = await model.sendRequest(messages, options, token);
    const toolCalls: vscode.LanguageModelToolCallPart[] = [];
    let text = '';

    for await (const part of response.stream) {
      if (part instanceof vscode.LanguageModelTextPart) {
        text += part.value;
      } else if (part instanceof vscode.LanguageModelToolCallPart) {
        toolCalls.push(part);
      }
    }

    if (toolCalls.length === 0) break;

    const results: vscode.LanguageModelToolResultPart[] = [];
    for (const call of toolCalls) {
      const result = await vscode.lm.invokeTool(call.name, {
        input: call.input,
        toolInvocationToken: undefined
      }, token);
      results.push(new vscode.LanguageModelToolResultPart(call.callId, result.content));
    }

    messages.push(vscode.LanguageModelChatMessage.Assistant([
      new vscode.LanguageModelTextPart(text),
      ...toolCalls
    ]));
    messages.push(vscode.LanguageModelChatMessage.User(results));
  }
}
```

### Tool Mode

```typescript
enum LanguageModelChatToolMode {
  Auto = 1,      // Model chooses whether to use tools
  Required = 2   // Model must use a tool
}
```

## Confirmation UI

Every tool invocation triggers a confirmation dialog. Extensions customize via `prepareInvocation()`.

```typescript
interface PreparedToolInvocation {
  invocationMessage?: string;  // Shown during execution
  confirmationMessages?: {
    title: string;
    message: string | MarkdownString;
  };
}

class SearchFilesTool implements vscode.LanguageModelTool<{pattern: string}> {
  async prepareInvocation(
    options: vscode.LanguageModelToolInvocationPrepareOptions<{pattern: string}>,
    _token: vscode.CancellationToken
  ): Promise<vscode.PreparedToolInvocation> {
    return {
      invocationMessage: `Searching for files matching "${options.input.pattern}"...`,
      confirmationMessages: {
        title: 'Search Workspace Files',
        message: new vscode.MarkdownString(
          `Search for files matching pattern **\`${options.input.pattern}\`**?`
        )
      }
    };
  }
}
```

### Approval Levels

- Single use
- Current session
- Current workspace
- Always allow

Users reset approvals via **Chat: Reset Tool Confirmations** command.

### Configuration

- `chat.tools.eligibleForAutoApproval`: Require manual approval for specific tools
- `chat.tools.global.autoApprove`: Allow all tools without prompting
- `chat.tools.urls.autoApprove`: Auto-approve URL patterns

## Tool Visibility

### When Clauses

```json
{
  "contributes": {
    "languageModelTools": [{
      "name": "debug_getCallStack",
      "when": "debugState == 'running'"
    }]
  }
}
```

### Private Tools

Skip `vscode.lm.registerTool()` to keep tools extension-only.

### Filtering

```typescript
const options: vscode.LanguageModelChatRequestOptions = {
  tools: vscode.lm.tools
    .filter(tool => tool.tags.includes('vscode_editing'))
    .map(tool => ({
      name: tool.name,
      description: tool.description,
      inputSchema: t.inputSchema ?? {}
    }))
};
```

### Tool Information

```typescript
interface LanguageModelToolInformation {
  readonly name: string;
  readonly description: string;
  readonly inputSchema: object | undefined;
  readonly tags: readonly string[];
}

const allTools = vscode.lm.tools;  // readonly LanguageModelToolInformation[]
```

## Provider-Side Tool Handling

For `LanguageModelChatProvider` implementations:

```typescript
interface LanguageModelChatProvider<T extends LanguageModelChatInformation> {
  provideLanguageModelChatResponse(
    model: T,
    messages: readonly LanguageModelChatRequestMessage[],
    options: LanguageModelChatRequestOptions,  // Contains tools array
    progress: Progress<LanguageModelResponsePart>,
    token: CancellationToken
  ): Thenable<any>;
}

interface LanguageModelChatInformation {
  readonly id: string;
  readonly name: string;
  readonly family: string;
  readonly version: string;
  readonly maxInputTokens: number;
  readonly maxOutputTokens: number;
  readonly capabilities: {
    readonly toolCalling?: boolean | number;
  };
}
```

Providers stream tool calls via `progress.report()` using `LanguageModelToolCallPart`.

## Limits

- 128 tool limit per request
- Use tool picker to deselect unneeded tools
- Enable virtual tools via `github.copilot.chat.virtualTools.threshold`
