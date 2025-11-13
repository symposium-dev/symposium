# VSCode Webview Deep Dive: Implementation Patterns for Symposium

Deep dive into how AmazonQ and Continue.dev implement their dedicated chat panels using VSCode webviews.

## Executive Summary

Both extensions create **dedicated sidebar panels** (not shared chat participants) using:
1. Custom `viewsContainers` in the activity bar
2. Webview-based views for the chat UI
3. Bidirectional communication between extension and webview
4. Backend process management (LSP for AmazonQ, in-process for Continue.dev)

This is the right pattern for Symposium Chat - a dedicated panel where users type messages directly without `@` prefixes.

---

## View Registration Pattern

### Package.json Configuration

Both extensions register custom view containers and webview views:

#### AmazonQ Pattern
```json
{
  "contributes": {
    "viewsContainers": {
      "activitybar": [
        {
          "id": "amazonq",
          "title": "Amazon Q",
          "icon": "resources/amazonq-logo.svg"
        }
      ]
    },
    "views": {
      "amazonq": [
        {
          "type": "webview",
          "id": "aws.amazonq.AmazonQChatView",
          "name": "Chat",
          "when": "!aws.isWebExtHost && !aws.amazonq.showLoginView"
        }
      ]
    }
  }
}
```

#### Continue.dev Pattern
```json
{
  "contributes": {
    "viewsContainers": {
      "activitybar": [
        {
          "id": "continue",
          "title": "Continue",
          "icon": "media/sidebar-icon.png"
        }
      ]
    },
    "views": {
      "continue": [
        {
          "type": "webview",
          "id": "continue.continueGUIView",
          "name": "Continue",
          "icon": "media/sidebar-icon.png",
          "visibility": "visible"
        }
      ]
    }
  }
}
```

**Key Points:**
- `viewsContainers.activitybar` adds icon to VS Code's left sidebar
- `views` defines webview panel within that container
- `type: "webview"` enables custom HTML/JS UI
- Unique `id` used to register provider in extension code

### Symposium Registration (Proposed)

```json
{
  "contributes": {
    "viewsContainers": {
      "activitybar": [
        {
          "id": "symposium",
          "title": "Symposium",
          "icon": "media/symposium-icon.svg"
        }
      ]
    },
    "views": {
      "symposium": [
        {
          "type": "webview",
          "id": "symposium.chatView",
          "name": "Chat"
        }
      ]
    }
  }
}
```

---

## Webview Provider Implementation

### AmazonQ: WebviewViewProvider Pattern

**File:** `src/lsp/chat/webviewProvider.ts`

```typescript
export class AmazonQChatViewProvider implements WebviewViewProvider {
    public static readonly viewType = 'aws.amazonq.AmazonQChatView'
    
    webviewView?: WebviewView
    webview?: Webview
    
    constructor(
        private readonly mynahUIPath: string,
        private readonly languageClient: BaseLanguageClient
    ) {}
    
    public async resolveWebviewView(
        webviewView: WebviewView,
        context: WebviewViewResolveContext,
        _token: CancellationToken
    ) {
        // Configure webview options
        webviewView.webview.options = {
            enableScripts: true,
            enableCommandUris: true,
            localResourceRoots: [lspDir, dist]
        }
        
        // Set HTML content
        webviewView.webview.html = await this.getWebviewContent()
        
        this.webviewView = webviewView
        this.webview = webviewView.webview
    }
    
    private async getWebviewContent() {
        // Load mynah-ui chat library
        return `
          <!DOCTYPE html>
          <html>
            <head>
              <meta charset="UTF-8">
              <meta http-equiv="Content-Security-Policy" content="...">
            </head>
            <body>
              <script src="${this.uiPath}"></script>
              <script src="${this.connectorAdapterPath}"></script>
              <script>
                const connector = new HybridChatAdapter(...)
                qChat = amazonQChat.createChat(vscodeApi, {...}, connector)
              </script>
            </body>
          </html>
        `
    }
}
```

**Registration:**
```typescript
// In activation.ts
const provider = new AmazonQChatViewProvider(mynahUIPath, languageClient)

disposables.push(
    window.registerWebviewViewProvider(
        AmazonQChatViewProvider.viewType,
        provider,
        { webviewOptions: { retainContextWhenHidden: true } }
    )
)
```

---

### Continue.dev: Similar Pattern with React

**File:** `extensions/vscode/src/ContinueGUIWebviewViewProvider.ts`

```typescript
export class ContinueGUIWebviewViewProvider implements vscode.WebviewViewProvider {
    public static readonly viewType = "continue.continueGUIView"
    public webviewProtocol: VsCodeWebviewProtocol
    
    private _webview?: vscode.Webview
    private _webviewView?: vscode.WebviewView
    
    constructor(
        private readonly windowId: string,
        private readonly extensionContext: vscode.ExtensionContext,
    ) {
        this.webviewProtocol = new VsCodeWebviewProtocol()
    }
    
    resolveWebviewView(
        webviewView: vscode.WebviewView,
        _context: vscode.WebviewViewResolveContext,
        _token: vscode.CancellationToken,
    ): void {
        this.webviewProtocol.webview = webviewView.webview
        this._webviewView = webviewView
        this._webview = webviewView.webview
        
        webviewView.webview.html = this.getSidebarContent(
            this.extensionContext,
            webviewView,
        )
    }
    
    getSidebarContent(context, panel, page?, edits?, isFullScreen?) {
        const inDevelopmentMode = 
            context?.extensionMode === vscode.ExtensionMode.Development
        
        // In dev: load from local server
        // In prod: load bundled React app
        const scriptUri = inDevelopmentMode
            ? "http://localhost:5173/src/main.tsx"
            : panel.webview.asWebviewUri(
                vscode.Uri.joinPath(extensionUri, "gui/assets/index.js")
              ).toString()
        
        return `
          <!DOCTYPE html>
          <html>
            <head>
              <script>const vscode = acquireVsCodeApi();</script>
              <link href="${styleMainUri}" rel="stylesheet">
            </head>
            <body>
              <div id="root"></div>
              <script type="module" src="${scriptUri}"></script>
            </body>
          </html>
        `
    }
}
```

**Key Differences:**
- Continue uses React for UI (hot reload in dev mode)
- AmazonQ uses pre-built chat library (mynah-ui)
- Continue has more complex build setup

---

## Communication Patterns

### Extension ↔ Webview Communication

Both use VS Code's webview messaging API, but with different abstractions:

#### AmazonQ: Direct postMessage + LSP Client

**Extension → Webview:**
```typescript
// Send message to webview
await provider.webview?.postMessage({
    command: 'chatRequestType',
    params: decryptedMessage,
    tabId: tabId,
})
```

**Webview → Extension:**
```typescript
// Register message listener
provider.webview?.onDidReceiveMessage(async (message) => {
    switch (message.command) {
        case 'aws/chat/ready':
            await webview.postMessage({
                command: CHAT_OPTIONS,
                params: chatOptions,
            })
            break
            
        case 'aws/chat/sendMessage':
            // Forward to LSP backend
            const chatResult = await languageClient.sendRequest(
                chatRequestType.method,
                chatParams,
                cancellationToken.token
            )
            break
    }
})
```

**Extension → LSP Backend:**
```typescript
// Send request to language server
const chatResult = await languageClient.sendRequest<ChatResult>(
    chatRequestType.method,
    { ...chatRequest, partialResultToken },
    cancellationToken.token
)

// Listen for streaming responses
const chatDisposable = languageClient.onProgress(
    chatRequestType,
    partialResultToken,
    (partialResult) => handlePartialResult(partialResult, provider, tabId)
)
```

---

#### Continue.dev: Protocol Abstraction

**VsCodeWebviewProtocol class:**
```typescript
export class VsCodeWebviewProtocol 
  implements IMessenger<FromWebviewProtocol, ToWebviewProtocol> {
    
    // Extension → Webview
    send(messageType: string, data: any, messageId?: string): string {
        const id = messageId ?? uuidv4()
        this.webview?.postMessage({ messageType, data, messageId: id })
        return id
    }
    
    // Webview → Extension (register handler)
    on<T extends keyof FromWebviewProtocol>(
        messageType: T,
        handler: (message) => Promise<Response> | Response,
    ): void {
        if (!this.listeners.has(messageType)) {
            this.listeners.set(messageType, [])
        }
        this.listeners.get(messageType)?.push(handler)
    }
    
    // Extension → Webview (request/response)
    public request<T extends keyof ToWebviewProtocol>(
        messageType: T,
        data: ToWebviewProtocol[T][0],
    ): Promise<ToWebviewProtocol[T][1]> {
        const messageId = uuidv4()
        return new Promise((resolve) => {
            this.send(messageType, data, messageId)
            
            const disposable = this.webview.onDidReceiveMessage((msg) => {
                if (msg.messageId === messageId) {
                    resolve(msg.data)
                    disposable?.dispose()
                }
            })
        })
    }
    
    // Webview → Extension (receive messages)
    set webview(webView: vscode.Webview) {
        this._webview = webView
        this._webviewListener = webView.onDidReceiveMessage(async (msg) => {
            const handlers = this.listeners.get(msg.messageType) || []
            for (const handler of handlers) {
                try {
                    const response = await handler(msg)
                    
                    // Handle streaming responses
                    if (response && typeof response[Symbol.asyncIterator] === 'function') {
                        for await (const chunk of response) {
                            this.send(msg.messageType, { 
                                done: false, 
                                content: chunk 
                            }, msg.messageId)
                        }
                        this.send(msg.messageType, { 
                            done: true 
                        }, msg.messageId)
                    } else {
                        this.send(msg.messageType, { 
                            done: true, 
                            content: response 
                        }, msg.messageId)
                    }
                } catch (e) {
                    this.send(msg.messageType, { 
                        done: true, 
                        error: e.message 
                    }, msg.messageId)
                }
            }
        })
    }
}
```

**Usage:**
```typescript
// Core registers message handlers
this.webviewProtocol.on("llm/streamChat", async (msg) => {
    const generator = this.core.streamChat(msg.data)
    return generator  // Protocol handles streaming
})

// Extension can request from webview
await this.webviewProtocol.request("setTheme", { theme: getTheme() })
```

---

## Backend Process Management

### AmazonQ: Language Server Process (LSP)

**Spawning the LSP:**

**File:** `src/lsp/client.ts`

```typescript
export async function startLanguageServer(
    extensionContext: vscode.ExtensionContext,
    resourcePaths: AmazonQResourcePaths
) {
    const serverModule = resourcePaths.lsp  // Path to LSP server JS
    
    const argv = [
        '--nolazy',
        '--preserve-symlinks',
        '--stdio',
        '--pre-init-encryption',
        '--set-credentials-encryption-key'
    ]
    
    const executable = [resourcePaths.node]  // Node.js binary path
    
    const serverOptions = createServerOptions({
        encryptionKey,
        executable: executable,
        serverModule,
        execArgv: argv,
        warnThresholds: { memory: memoryWarnThreshold }
    })
    
    const clientOptions: LanguageClientOptions = {
        documentSelector: [{ scheme: 'file', language: '*' }],
        synchronize: { ... },
        initializationOptions: { ... }
    }
    
    const client = new LanguageClient(
        'amazonq',
        'Amazon Q Language Server',
        serverOptions,
        clientOptions
    )
    
    await client.start()  // VS Code handles process lifecycle
    
    // Activate chat feature with LSP client
    await activate(client, encryptionKey, resourcePaths.ui)
}
```

**Process Lifecycle:**
- VS Code's `LanguageClient` handles spawning, crashes, restarts
- stdio communication (JSON-RPC protocol)
- Automatic crash detection and recovery:
```typescript
function onServerRestartHandler(client: BaseLanguageClient) {
    return client.onDidChangeState(async (e) => {
        if (e.oldState === State.Starting && e.newState === State.Running) {
            telemetry.languageServer_crash.emit({ id: 'AmazonQ' })
            await auth.refreshConnection(true)
            await initializeLanguageServerConfiguration(client, 'crash-recovery')
        }
    })
}
```

**Communication:**
```typescript
// Request/response
const chatResult = await languageClient.sendRequest<ChatResult>(
    'aws/chat',
    chatParams,
    cancellationToken.token
)

// Streaming responses via progress tokens
const partialResultToken = uuidv4()
languageClient.onProgress(
    chatRequestType,
    partialResultToken,
    (partialResult) => {
        // Forward to webview
        provider.webview?.postMessage({ ...partialResult })
    }
)
```

---

### Continue.dev: In-Process Core

**No separate backend process** - Core runs in extension host:

```typescript
// In VsCodeExtension.ts
constructor(context: vscode.ExtensionContext) {
    this.ide = new VsCodeIde(this.webviewProtocolPromise, context)
    this.configHandler = new ConfigHandler(...)
    
    // Core runs in-process
    this.core = new Core(
        this.ide,
        this.configHandler,
        this.battery,
        this.fileSearch,
        // ...
    )
    
    // Connect core to webview protocol
    const messenger = new InProcessMessenger<
        FromCoreProtocol,
        ToCoreProtocol
    >()
    
    this.core.invoke = messenger.request.bind(messenger)
}
```

**Communication:**
```typescript
// Webview → Core (via messenger)
this.webviewProtocol.on("llm/streamChat", async (msg) => {
    // Directly invoke core (same process)
    return await this.core.streamChat(msg.data)
})
```

**Pros:**
- Simpler - no separate process
- Faster communication (no IPC)
- Easier debugging

**Cons:**
- Core crash affects entire extension
- No process isolation
- Harder to manage long-running operations

---

## Comparison: AmazonQ vs Continue.dev

| Aspect | AmazonQ | Continue.dev |
|--------|---------|--------------|
| **UI Framework** | mynah-ui (custom) | React + Vite |
| **Backend** | Separate LSP process | In-process Core |
| **Communication** | Direct postMessage + LSP | Protocol abstraction |
| **Process Isolation** | Yes (LSP) | No |
| **Crash Recovery** | Automatic (LSP client) | Manual |
| **Streaming** | LSP progress tokens | Async generators |
| **Build Complexity** | Medium | High |
| **Dev Experience** | Reload extension | Hot reload (React) |

---

## Recommendation for Symposium

### Approach: Hybrid Pattern

Combine the best of both:

1. **Use WebviewViewProvider** (like both)
2. **Spawn sacp-conductor as separate process** (like AmazonQ's LSP)
3. **Simple communication protocol** (don't need full LSP)
4. **Start with simple HTML/JS UI** (avoid React complexity initially)

### Implementation Sketch

**package.json:**
```json
{
  "contributes": {
    "viewsContainers": {
      "activitybar": [
        {
          "id": "symposium",
          "title": "Symposium",
          "icon": "media/symposium-icon.svg"
        }
      ]
    },
    "views": {
      "symposium": [
        {
          "type": "webview",
          "id": "symposium.chatView",
          "name": "Chat"
        }
      ]
    },
    "configuration": {
      "title": "Symposium",
      "properties": {
        "symposium.agent": {
          "type": "string",
          "enum": ["claudeCode", "elizACP"],
          "default": "claudeCode",
          "description": "Which agent to use"
        }
      }
    }
  }
}
```

**Webview Provider:**
```typescript
// src/SymposiumChatProvider.ts
export class SymposiumChatProvider implements vscode.WebviewViewProvider {
    public static readonly viewType = 'symposium.chatView'
    
    private _view?: vscode.WebviewView
    private _conductor?: ChildProcess
    
    constructor(
        private readonly _extensionContext: vscode.ExtensionContext
    ) {}
    
    public async resolveWebviewView(
        webviewView: vscode.WebviewView,
        context: vscode.WebviewViewResolveContext,
        _token: vscode.CancellationToken
    ) {
        this._view = webviewView
        
        webviewView.webview.options = {
            enableScripts: true,
            localResourceRoots: [
                vscode.Uri.joinPath(this._extensionContext.extensionUri, 'media')
            ]
        }
        
        webviewView.webview.html = this._getHtmlForWebview(webviewView.webview)
        
        // Handle messages from webview
        webviewView.webview.onDidReceiveMessage(async (message) => {
            switch (message.type) {
                case 'sendMessage':
                    await this._sendToConductor(message.text)
                    break
                case 'selectAgent':
                    await this._restartConductor(message.agent)
                    break
            }
        })
        
        // Start conductor
        await this._startConductor()
    }
    
    private async _startConductor() {
        const config = vscode.workspace.getConfiguration('symposium')
        const agent = config.get<string>('agent', 'claudeCode')
        
        // Spawn sacp-conductor
        this._conductor = spawn('sacp-conductor', ['--agent', agent], {
            stdio: ['pipe', 'pipe', 'pipe']
        })
        
        // Handle output from conductor
        this._conductor.stdout?.on('data', (data) => {
            const message = JSON.parse(data.toString())
            // Forward to webview
            this._view?.webview.postMessage({
                type: 'message',
                content: message
            })
        })
        
        // Handle errors
        this._conductor.on('error', (error) => {
            vscode.window.showErrorMessage(`Conductor error: ${error.message}`)
        })
        
        // Handle crashes
        this._conductor.on('exit', (code) => {
            if (code !== 0) {
                vscode.window.showWarningMessage(
                    'Conductor crashed. Restarting...'
                )
                this._startConductor()
            }
        })
    }
    
    private async _sendToConductor(text: string) {
        if (this._conductor) {
            const message = JSON.stringify({ type: 'chat', text })
            this._conductor.stdin?.write(message + '\n')
        }
    }
    
    private async _restartConductor(agent: string) {
        // Kill old conductor
        this._conductor?.kill()
        // Start new one with different agent
        await this._startConductor()
    }
    
    private _getHtmlForWebview(webview: vscode.Webview) {
        const nonce = getNonce()
        
        return `<!DOCTYPE html>
        <html lang="en">
        <head>
            <meta charset="UTF-8">
            <meta http-equiv="Content-Security-Policy" 
                  content="default-src 'none'; 
                           style-src ${webview.cspSource} 'unsafe-inline'; 
                           script-src 'nonce-${nonce}';">
            <style>
                body { 
                    padding: 0; 
                    margin: 0; 
                    display: flex; 
                    flex-direction: column; 
                    height: 100vh; 
                }
                #messages { flex: 1; overflow-y: auto; padding: 10px; }
                #input-container { display: flex; padding: 10px; }
                #input { flex: 1; }
            </style>
        </head>
        <body>
            <div id="messages"></div>
            <div id="input-container">
                <input type="text" id="input" placeholder="Type a message..." />
                <button id="send">Send</button>
            </div>
            
            <script nonce="${nonce}">
                const vscode = acquireVsCodeApi()
                
                // Handle messages from extension
                window.addEventListener('message', event => {
                    const message = event.data
                    if (message.type === 'message') {
                        addMessage(message.content)
                    }
                })
                
                // Send message on button click
                document.getElementById('send').addEventListener('click', () => {
                    const input = document.getElementById('input')
                    vscode.postMessage({
                        type: 'sendMessage',
                        text: input.value
                    })
                    addMessage(input.value, true)
                    input.value = ''
                })
                
                function addMessage(text, isUser = false) {
                    const div = document.createElement('div')
                    div.textContent = (isUser ? 'You: ' : 'Bot: ') + text
                    document.getElementById('messages').appendChild(div)
                }
            </script>
        </body>
        </html>`
    }
}
```

**Activation:**
```typescript
// src/extension.ts
export function activate(context: vscode.ExtensionContext) {
    const provider = new SymposiumChatProvider(context)
    
    context.subscriptions.push(
        vscode.window.registerWebviewViewProvider(
            SymposiumChatProvider.viewType,
            provider,
            { webviewOptions: { retainContextWhenHidden: true } }
        )
    )
}
```

---

## Next Steps

1. **Start with minimal HTML/JS UI** - avoid React complexity initially
2. **Spawn sacp-conductor as child process** - use Node.js `child_process`
3. **Implement stdio communication** - JSON messages over stdin/stdout
4. **Add crash recovery** - restart conductor on exit
5. **Add agent selection** - configuration + restart logic
6. **Enhance UI progressively** - markdown rendering, code blocks, etc.
7. **Consider React later** - if UI complexity grows

---

## Key Takeaways

1. **Webview is the right choice** for dedicated Symposium Chat panel
2. **Separate process (like AmazonQ's LSP)** provides isolation and crash recovery
3. **Simple communication protocol** is sufficient - don't need full LSP
4. **Start simple** - basic HTML/JS, enhance later
5. **Both patterns are proven** - choose based on requirements and complexity tolerance
