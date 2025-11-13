# VSCode Extension Research: Building Symposium Chat

Research findings from studying three VSCode extensions with chat interfaces to inform the design of a Symposium VSCode extension.

## Research Goals

We want to build a VSCode extension that:
- Shows a chat panel called "Symposium Chat"
- Lets users pick between backend agents (Claude Code, ElizACP, etc.)
- Spawns `sacp-conductor` to run the selected agent
- Provides a chat interface similar to existing extensions

## Extensions Studied

1. **AWS Toolkit (amazonq)** - `~/dev/aws-toolkit-vscode/packages/amazonq`
2. **GitHub Copilot Chat** - `~/dev/vscode-copilot-chat`
3. **Continue.dev** - `~/dev/continue`

---

## Key Findings

### 1. Chat Integration Approaches

#### AmazonQ: Custom Webview with LSP Backend

**Architecture:**
- Uses custom webview panel (`AmazonQChatViewProvider` implements `WebviewViewProvider`)
- Backend is a Language Server (LSP) spawned as a separate Node.js process
- Webview loads custom UI from bundled JavaScript (`mynah-ui` library)

**Key Files:**
- `src/lsp/chat/webviewProvider.ts` - Webview panel registration
- `src/lsp/chat/activation.ts` - Chat feature activation
- `src/lsp/client.ts` - LSP client that spawns the language server

**Backend Process Management:**
```typescript
// From client.ts - spawns Node.js process running the language server
const serverOptions = createServerOptions({
    encryptionKey,
    executable: [resourcePaths.node],  // Node.js executable
    serverModule,                      // Path to LSP server JS
    execArgv: argv,
});
```

**Webview HTML Structure:**
```typescript
// Loads custom chat UI from mynah-ui library
webviewView.webview.html = `
  <script src="${this.uiPath}"></script>
  <script src="${this.connectorAdapterPath}"></script>
  <script>
    const hybridChatConnector = new HybridChatAdapter(...)
    qChat = amazonQChat.createChat(vscodeApi, {...}, hybridChatConnector)
  </script>
`
```

**Communication Pattern:**
- Extension → Webview: `webview.postMessage()`
- Webview → Extension: Message listeners registered via `registerMessageListeners()`
- Extension → LSP: Standard LSP protocol (JSON-RPC over stdio)

**Pros:**
- Complete control over chat UI appearance
- Can bundle custom UI libraries
- Backend is separate process (isolation, crash recovery)

**Cons:**
- More complex to build custom UI
- Need to bundle and maintain UI code
- Webview security/CSP considerations

---

#### GitHub Copilot: VS Code Native Chat API

**Architecture:**
- Uses VS Code's built-in Chat API (`vscode.chat.createChatParticipant`)
- Native VS Code chat interface (not custom webview)
- Integrates with VS Code's chat panel

**Key Files:**
- `src/extension/conversation/vscode-node/chatParticipants.ts` - Participant registration
- `src/extension/prompt/node/chatParticipantRequestHandler.ts` - Request handling

**Chat Participant Registration:**
```typescript
// Creates a chat participant using VS Code's native API
const agent = vscode.chat.createChatParticipant(
  id,
  this.getChatParticipantHandler(id, name, defaultIntent, onRequestPaused)
);

agent.iconPath = new vscode.ThemeIcon('copilot');
agent.onDidReceiveFeedback(e => { /* handle feedback */ });
```

**Request Handler:**
```typescript
async handleRequest(
  request: vscode.ChatRequest,
  context: vscode.ChatContext,
  stream: vscode.ChatResponseStream,
  token: vscode.CancellationToken
): Promise<void> {
  // Process request, stream responses back
  stream.markdown("Response text...");
}
```

**Backend:**
- No separate process spawned - makes HTTP calls to backend services
- Uses GitHub's Copilot API endpoints
- Authentication handled via VS Code's auth provider

**Pros:**
- Native VS Code UI (consistent with other chat extensions)
- Built-in features: history, context, markdown rendering
- Simpler development - no custom UI needed
- Better integration with VS Code (command palette, etc.)

**Cons:**
- Less control over UI appearance
- Tied to VS Code's chat API design decisions
- Requires VS Code with Chat API support (newer versions)

---

#### Continue.dev: Custom Webview with React Frontend

**Architecture:**
- Uses custom webview panel (`ContinueGUIWebviewViewProvider` implements `WebviewViewProvider`)
- Frontend is separate React application (in `gui/` directory)
- Core logic runs in extension process (no separate backend process)

**Key Files:**
- `extensions/vscode/src/ContinueGUIWebviewViewProvider.ts` - Webview provider
- `extensions/vscode/src/extension/VsCodeExtension.ts` - Main extension class
- `gui/` - Separate React application for the chat UI

**Frontend/Backend Separation:**
```typescript
// In development: loads from local dev server
scriptUri = "http://localhost:5173/src/main.tsx";

// In production: loads bundled React app
scriptUri = panel.webview.asWebviewUri(
  vscode.Uri.joinPath(extensionUri, "gui/assets/index.js")
).toString();
```

**Communication Protocol:**
```typescript
// VsCodeWebviewProtocol handles bidirectional communication
this.webviewProtocol = new VsCodeWebviewProtocol();
this.webviewProtocol.webview = panel.webview;

// Extension can send requests to webview
await this.webviewProtocol.request("setTheme", { theme: getTheme() });

// Webview can send messages back via postMessage
```

**Core Architecture:**
```typescript
// Core runs in-process (not separate backend)
this.core = new Core(
  this.ide,
  this.configHandler,
  this.battery,
  this.fileSearch,
  // ...
);
```

**Pros:**
- Modern React-based UI (familiar to web developers)
- Frontend/backend well separated (could be reused for other editors)
- Hot reload during development
- Rich UI capabilities

**Cons:**
- Complex build setup (Vite, React, bundling)
- Harder to build and deploy (mentioned in your note)
- More dependencies to manage
- Core runs in extension process (not isolated)

---

## Comparison Matrix

| Feature | AmazonQ | Copilot | Continue.dev |
|---------|---------|---------|--------------|
| **UI Approach** | Custom Webview | Native Chat API | Custom Webview (React) |
| **Backend** | Separate LSP process | HTTP calls to service | In-process Core |
| **Process Isolation** | Yes (LSP) | No | No |
| **UI Technology** | Custom JS (mynah-ui) | VS Code native | React + Vite |
| **Build Complexity** | Medium | Low | High |
| **UI Flexibility** | High | Low | Very High |
| **VS Code Integration** | Medium | Excellent | Medium |
| **Development Experience** | Custom bundling | Simple | Hot reload, modern |

---

## Recommendations for Symposium Chat

### Approach A: Native Chat API (Recommended for MVP)

**Similar to:** GitHub Copilot

**Why:**
- Fastest to build and iterate
- Native VS Code experience
- Built-in features (history, markdown, code blocks)
- Easy backend selection (just spawn different processes)

**Implementation sketch:**
```typescript
// Register chat participant
const symposiumParticipant = vscode.chat.createChatParticipant(
  'symposium.chat',
  async (request, context, stream, token) => {
    // Spawn sacp-conductor with selected agent
    const conductor = await spawnConductor(selectedAgent);
    
    // Forward chat messages to conductor
    // Stream responses back through VS Code chat
  }
);

symposiumParticipant.iconPath = new vscode.ThemeIcon('symposium');
```

**Backend Selection:**
- Use VS Code settings: `symposium.agent` (claudeCode, elizACP, etc.)
- Spawn `sacp-conductor --agent <selected>` when chat starts
- Manage conductor lifecycle (start, restart, stop)

**Next Steps:**
1. Study VS Code Chat API documentation
2. Create minimal chat participant that echoes messages
3. Add sacp-conductor spawning and stdio communication
4. Add agent selection UI (quick pick or settings)

---

### Approach B: Custom Webview (Future Enhancement)

**Similar to:** AmazonQ or Continue.dev

**When to consider:**
- Need custom UI that Chat API doesn't support
- Want to share UI with other editors
- Need features like file previews, custom widgets, etc.

**Implementation:**
- Start with AmazonQ pattern (simpler than Continue.dev)
- Use standard webview with custom HTML/JS
- Add React later if needed

---

## Backend Process Management Patterns

All three extensions manage backend processes differently:

### AmazonQ Pattern (LSP)
```typescript
// Spawn language server as child process
const client = new LanguageClient(
  'amazonq',
  serverOptions,  // { command, args, options }
  clientOptions
);

await client.start();  // VS Code LSP client handles lifecycle
```

**Key aspects:**
- VS Code LanguageClient handles spawning, restart, crash recovery
- stdio communication (JSON-RPC)
- Well-established patterns for process management

**For Symposium:**
- We could use LSP pattern even though sacp-conductor isn't strictly an LSP
- Benefits: crash recovery, logging, lifecycle management built-in
- Would need to adapt sacp-conductor to LSP protocol (or create thin LSP wrapper)

### Continue.dev Pattern (In-Process)
```typescript
// Core runs in extension process
this.core = new Core(...);
await this.core.invoke("chat", { message });
```

**Not recommended for Symposium** - we want conductor as separate process for isolation.

### Custom Process Management
```typescript
// Simple approach for sacp-conductor
class ConductorManager {
  private process?: ChildProcess;
  
  async start(agent: string) {
    this.process = spawn('sacp-conductor', ['--agent', agent]);
    // Handle stdio, errors, crashes
  }
  
  send(message: any) {
    this.process.stdin.write(JSON.stringify(message));
  }
  
  async restart() { /* ... */ }
}
```

**Recommended for Symposium:**
- Start simple with custom process management
- Add sophistication as needed (crash recovery, logging, etc.)
- Consider LSP client wrapper later if beneficial

---

## Open Questions for Discussion

1. **Chat API vs Custom Webview?**
   - Chat API is simpler but less flexible
   - Do we need custom UI features beyond basic chat?

2. **Agent Selection UX:**
   - Settings-based: `symposium.defaultAgent`
   - Command palette: "Symposium: Select Agent"
   - In-chat switcher: `/agent claudeCode`
   - Sidebar panel with dropdown?

3. **Conductor Lifecycle:**
   - One conductor per workspace?
   - One per chat session?
   - Persistent vs ephemeral?

4. **Configuration:**
   - Where do users configure agents? (settings.json, .symposium/config.yaml)
   - How to pass config to sacp-conductor?

5. **Build Complexity:**
   - How much build infrastructure do we want to maintain?
   - TypeScript + simple bundling vs full React setup?

---

## References

### VS Code Chat API
- [Chat Extension Guide](https://code.visualstudio.com/api/extension-guides/chat)
- [Chat API Reference](https://code.visualstudio.com/api/references/vscode-api#chat)

### Example Extensions
- AmazonQ: `~/dev/aws-toolkit-vscode/packages/amazonq`
- GitHub Copilot: `~/dev/vscode-copilot-chat`
- Continue.dev: `~/dev/continue`

### Useful Patterns
- Webview API: https://code.visualstudio.com/api/extension-guides/webview
- Language Client: https://code.visualstudio.com/api/language-extensions/language-server-extension-guide
- Extension Samples: https://github.com/microsoft/vscode-extension-samples
