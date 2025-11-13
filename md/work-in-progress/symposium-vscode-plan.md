# Symposium VSCode Extension: Implementation Plan

Plan for building a VSCode extension that provides a dedicated chat panel for interacting with ACP agents via sacp-conductor.

## Overview

**Goal:** Create a polished chat interface in VSCode where users can:
- Chat with different agents (Claude Code, ElizACP, etc.)
- Switch agents without restarting VSCode
- Get a production-quality UI experience

**Key Decisions:**
1. ✅ **Custom webview panel** (not Chat Participant API - that requires `@mentions`)
2. ✅ **mynah-ui for the chat interface** (AWS's battle-tested chat library)
3. ✅ **ACP TypeScript SDK** (Zed's SDK for protocol handling)
4. ✅ **sacp-conductor as backend** (spawned as separate process)

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│  VSCode Extension (Symposium)                                   │
│                                                                  │
│  ┌──────────────────┐      ┌──────────────────────────────┐   │
│  │  Webview Panel   │◄────►│  Extension Host              │   │
│  │                  │      │                               │   │
│  │  ┌────────────┐  │      │  ┌─────────────────────────┐ │   │
│  │  │ mynah-ui   │  │      │  │ ConductorManager        │ │   │
│  │  │            │  │      │  │                         │ │   │
│  │  │ - Chat UI  │  │      │  │ ┌─────────────────────┐ │ │   │
│  │  │ - Messages │  │      │  │ │ ClientSideConnection│ │ │   │
│  │  │ - Input    │  │      │  │ │ (ACP SDK)           │ │ │   │
│  │  │ - Markdown │  │      │  │ └──────────┬──────────┘ │ │   │
│  │  └────────────┘  │      │  └─────────────┼────────────┘ │   │
│  └──────────────────┘      └────────────────┼──────────────┘   │
│                                              │                   │
└──────────────────────────────────────────────┼───────────────────┘
                                               │ ACP Protocol
                                               │ (stdio/JSON-RPC)
                                   ┌───────────▼────────────┐
                                   │   sacp-conductor       │
                                   │                        │
                                   │   --agent claudeCode   │
                                   │   --agent elizACP      │
                                   └────────────────────────┘
```

## Component Details

### 1. mynah-ui Chat Interface

**What we get:**
- Production-ready chat UI
- Markdown rendering with syntax highlighting
- Code block formatting
- Streaming message support
- User input handling
- Theme integration (automatically matches VS Code themes)
- Event-driven API

**Package:** `@aws/mynah-ui` (Apache 2.0 licensed)
- Latest version: 4.35.5
- Actively maintained by AWS
- Used in Amazon Q for VSCode, JetBrains, Visual Studio, Eclipse
- Framework-agnostic (vanilla JS/TS)
- Documentation: https://github.com/aws/mynah-ui

**Key Features:**
- No framework dependencies (works directly in webviews)
- Configurable through constructor options
- Event handlers for user actions
- Supports tabs (for multiple conversations)
- File tree rendering
- Progress indicators
- Custom commands/quick actions

### 2. ACP TypeScript SDK

**What we get:**
- Protocol compliance (JSON-RPC over stdio)
- Type-safe message handling
- Request/response correlation
- Streaming support
- Error handling

**Package:** `@agentclientprotocol/sdk` or `@zed-industries/agent-client-protocol`
- Maintained by Zed Industries
- Used by Zed editor
- Apache 2.0 licensed

**Key Class:** `ClientSideConnection`
- Wraps stdio communication with agent
- Provides typed API for ACP messages
- Handles protocol details

### 3. sacp-conductor Backend

**What it does:**
- Spawned as child process by extension
- Runs selected agent (Claude Code, ElizACP, etc.)
- Implements ACP protocol
- Communicates via stdio

**Configuration:**
- Agent selection: `--agent <name>`
- Additional flags TBD

## Implementation Phases

### Phase 1: Minimal Working Chat (Week 1)

**Goal:** Get basic chat working end-to-end

**Tasks:**
1. Create VSCode extension skeleton
   - `package.json` with view container and webview view
   - Basic activation code
   
2. Integrate mynah-ui
   - Install `@aws/mynah-ui`
   - Create webview HTML that loads mynah-ui
   - Wire up basic message sending
   
3. Spawn sacp-conductor
   - Create `ConductorManager` class
   - Spawn process with one agent (e.g., claudeCode)
   - Basic stdio communication (can be manual, before ACP SDK)
   
4. Connect the pieces
   - Webview → Extension → Conductor → Back
   - Display responses in mynah-ui

**Success Criteria:**
- Extension loads in VSCode
- Chat panel appears in sidebar
- Can type message and get response from agent
- Messages display nicely with markdown

### Phase 2: Production Features (Week 2)

**Goal:** Polish and features for daily use

**Tasks:**
1. Integrate ACP SDK properly
   - Replace manual stdio with `ClientSideConnection`
   - Proper request/response handling
   - Error handling
   
2. Agent selection
   - Configuration setting for default agent
   - UI control (dropdown or command palette)
   - Runtime agent switching
   
3. Streaming responses
   - Display chunks as they arrive
   - Progress indicators
   - Stop generation button
   
4. Error handling and recovery
   - Conductor crash detection
   - Auto-restart logic
   - User-friendly error messages
   
5. Polish
   - Icons and branding
   - Loading states
   - Empty states
   - Settings page

**Success Criteria:**
- Can switch between agents
- Streaming responses work smoothly
- Conductor crashes don't break extension
- Feels polished and professional

### Phase 3: Advanced Features (Week 3+)

**Optional enhancements:**

1. **Tool visualization**
   - Show when agent uses tools
   - Display tool results
   - File tree for file operations
   
2. **Multi-agent support**
   - Multiple conversations with different agents
   - Tab support (mynah-ui has this built-in)
   
3. **History and persistence**
   - Save conversation history
   - Restore on reload
   
4. **Configuration**
   - Model selection
   - Temperature and other parameters
   - Custom agent configurations
   
5. **Developer features**
   - Debug panel
   - View raw ACP messages
   - Performance metrics

## File Structure

```
symposium-vscode/
├── package.json                 # Extension manifest
├── tsconfig.json               # TypeScript config
├── src/
│   ├── extension.ts            # Entry point
│   ├── chatProvider.ts         # WebviewViewProvider
│   ├── conductorManager.ts     # Manages sacp-conductor process
│   └── webview/
│       ├── index.html          # Webview HTML
│       └── main.ts             # Webview script (mynah-ui setup)
├── media/
│   └── symposium-icon.svg     # Activity bar icon
└── out/                        # Compiled JS (gitignored)
```

## Key Code Patterns

### Webview HTML (Loads mynah-ui)

```html
<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <meta http-equiv="Content-Security-Policy" content="...">
    <link rel="stylesheet" href="${mynahCssUri}">
</head>
<body>
    <div id="mynah-ui-root"></div>
    <script src="${mynahJsUri}"></script>
    <script nonce="${nonce}">
        const vscode = acquireVsCodeApi();
        
        // Initialize mynah-ui
        const mynahUI = new mynahUI.MynahUI({
            onSendMessage: (message) => {
                vscode.postMessage({
                    type: 'sendMessage',
                    text: message
                });
            }
        });
        
        // Handle responses from extension
        window.addEventListener('message', event => {
            const message = event.data;
            
            switch (message.type) {
                case 'messageChunk':
                    mynahUI.updateLastMessage({
                        body: message.content
                    });
                    break;
                case 'messageDone':
                    // Mark message complete
                    break;
            }
        });
    </script>
</body>
</html>
```

### Extension: WebviewViewProvider

```typescript
export class SymposiumChatProvider implements vscode.WebviewViewProvider {
    public static readonly viewType = 'symposium.chatView';
    
    private _view?: vscode.WebviewView;
    private _conductor: ConductorManager;
    
    constructor(private readonly _context: vscode.ExtensionContext) {
        this._conductor = new ConductorManager();
    }
    
    async resolveWebviewView(webviewView: vscode.WebviewView) {
        this._view = webviewView;
        
        // Configure webview
        webviewView.webview.options = {
            enableScripts: true,
            localResourceRoots: [
                vscode.Uri.joinPath(this._context.extensionUri, 'media'),
                vscode.Uri.joinPath(this._context.extensionUri, 'node_modules/@aws/mynah-ui/dist')
            ]
        };
        
        // Set HTML
        webviewView.webview.html = this._getHtmlForWebview(webviewView.webview);
        
        // Handle messages from webview
        webviewView.webview.onDidReceiveMessage(async (message) => {
            if (message.type === 'sendMessage') {
                await this._handleSendMessage(message.text);
            }
        });
        
        // Start conductor
        await this._conductor.start();
    }
    
    private async _handleSendMessage(text: string) {
        // Send to conductor, get streaming response
        await this._conductor.sendMessage(text, (chunk) => {
            this._view?.webview.postMessage({
                type: 'messageChunk',
                content: chunk
            });
        });
        
        this._view?.webview.postMessage({
            type: 'messageDone'
        });
    }
}
```

### Extension: ConductorManager

```typescript
export class ConductorManager {
    private process?: ChildProcess;
    private connection?: ClientSideConnection; // From ACP SDK
    
    async start() {
        // Spawn sacp-conductor
        const config = vscode.workspace.getConfiguration('symposium');
        const agent = config.get<string>('agent', 'claudeCode');
        
        this.process = spawn('sacp-conductor', ['--agent', agent], {
            stdio: ['pipe', 'pipe', 'pipe']
        });
        
        // Initialize ACP connection
        this.connection = new ClientSideConnection({
            input: this.process.stdout!,
            output: this.process.stdin!
        });
        
        await this.connection.initialize();
        
        // Handle crashes
        this.process.on('exit', (code) => {
            if (code !== 0) {
                // Auto-restart
                setTimeout(() => this.start(), 1000);
            }
        });
    }
    
    async sendMessage(
        text: string,
        onChunk?: (chunk: string) => void
    ): Promise<void> {
        // Use ACP SDK to send message
        // Handle streaming response
        // Call onChunk for each chunk
    }
    
    async changeAgent(agent: string): Promise<void> {
        await this.stop();
        // Update config
        await this.start();
    }
}
```

## Dependencies

```json
{
  "dependencies": {
    "@aws/mynah-ui": "^4.35.5",
    "@agentclientprotocol/sdk": "^0.4.5"
  },
  "devDependencies": {
    "@types/node": "^20.x",
    "@types/vscode": "^1.85.0",
    "typescript": "^5.3.0"
  }
}
```

## Configuration Schema

```json
{
  "configuration": {
    "title": "Symposium",
    "properties": {
      "symposium.agent": {
        "type": "string",
        "enum": ["claudeCode", "elizACP"],
        "default": "claudeCode",
        "description": "Which agent to use for chat"
      },
      "symposium.conductorPath": {
        "type": "string",
        "default": "sacp-conductor",
        "description": "Path to sacp-conductor executable"
      },
      "symposium.autoRestart": {
        "type": "boolean",
        "default": true,
        "description": "Automatically restart conductor on crash"
      }
    }
  }
}
```

## Testing Strategy

### Manual Testing
1. **Basic chat flow**
   - Send message → receive response
   - Markdown rendering
   - Code block formatting
   
2. **Agent switching**
   - Switch agent → chat continues
   - Different agents behave differently
   
3. **Error scenarios**
   - Conductor crashes → auto-restart
   - Invalid responses → error handling
   - Network issues → graceful degradation

### Automated Testing (Future)
- Unit tests for ConductorManager
- Integration tests for ACP communication
- E2E tests with mock conductor

## Open Questions

1. **mynah-ui API details**
   - Exact initialization options
   - How to update streaming messages
   - Customization options
   - Theme configuration

2. **ACP SDK API**
   - Exact `ClientSideConnection` API
   - Streaming response handling
   - Error types and handling
   - Tool/capability queries

3. **sacp-conductor**
   - Command-line interface
   - Configuration options
   - Error reporting
   - Health checks

4. **Agent-specific features**
   - Do different agents need different UI?
   - Custom tool visualization?
   - Agent capabilities discovery?

## Next Steps

1. **Review mynah-ui documentation**
   - Read GitHub README
   - Study examples
   - Check API reference
   
2. **Review ACP SDK**
   - Look at SDK examples
   - Check TypeScript definitions
   - Understand streaming model
   
3. **Prototype Phase 1**
   - Create extension skeleton
   - Get mynah-ui loading
   - Wire up basic communication
   
4. **Iterate**
   - Test with real agents
   - Gather feedback
   - Add features

## Success Metrics

**Phase 1 Success:**
- Extension installs and activates
- Chat UI loads and looks professional
- Can send message and get response
- Markdown and code blocks render correctly

**Phase 2 Success:**
- Can switch agents smoothly
- Streaming feels responsive
- Handles errors gracefully
- Users report it "just works"

**Long-term Success:**
- Users prefer it to other chat interfaces
- Supports all Symposium agents
- Extensible for future features
- Community contributions
