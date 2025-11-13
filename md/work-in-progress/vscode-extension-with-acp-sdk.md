# VSCode Extension for Symposium: Using ACP TypeScript SDK

Implementation guide for building a Symposium VSCode extension that uses the official ACP TypeScript SDK to communicate with sacp-conductor.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│  VSCode Extension (Symposium)                               │
│                                                              │
│  ┌──────────────────┐      ┌──────────────────────────┐   │
│  │  Webview Panel   │◄────►│  Extension Host          │   │
│  │  (Chat UI)       │      │                          │   │
│  │                  │      │  ┌────────────────────┐  │   │
│  │  - Messages      │      │  │ ClientSideConnection│  │   │
│  │  - Input box     │      │  │ (ACP SDK)          │  │   │
│  │  - Agent picker  │      │  └────────┬───────────┘  │   │
│  └──────────────────┘      └───────────┼──────────────┘   │
│                                         │                   │
└─────────────────────────────────────────┼───────────────────┘
                                          │ ACP Protocol
                                          │ (JSON-RPC over stdio)
                              ┌───────────▼────────────┐
                              │   sacp-conductor       │
                              │                        │
                              │   --agent claudeCode   │
                              │   --agent elizACP      │
                              └────────────────────────┘
```

## Benefits of Using ACP TypeScript SDK

1. **Protocol Compliance**: Handles all ACP protocol details (JSON-RPC, request/response)
2. **Type Safety**: TypeScript types for all ACP messages
3. **Streaming Support**: Built-in support for streaming responses
4. **Error Handling**: Proper error propagation
5. **Tested**: Used by Zed and other ACP clients

## Installation

```bash
# In your vscode extension project
npm install @agentclientprotocol/sdk
```

Or using the Zed package:
```bash
npm install @zed-industries/agent-client-protocol
```

## Implementation

### 1. Package.json Configuration

```json
{
  "name": "symposium-vscode",
  "displayName": "Symposium",
  "description": "Chat with ACP agents via Symposium conductor",
  "version": "0.1.0",
  "engines": {
    "vscode": "^1.85.0"
  },
  "activationEvents": ["onView:symposium.chatView"],
  "main": "./out/extension.js",
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
        },
        "symposium.conductorPath": {
          "type": "string",
          "default": "sacp-conductor",
          "description": "Path to sacp-conductor executable"
        }
      }
    }
  },
  "scripts": {
    "compile": "tsc -p ./",
    "watch": "tsc -watch -p ./"
  },
  "dependencies": {
    "@agentclientprotocol/sdk": "^0.4.5"
  },
  "devDependencies": {
    "@types/node": "^20.x.x",
    "@types/vscode": "^1.85.0",
    "typescript": "^5.3.0"
  }
}
```

### 2. ACP Conductor Manager

Create a manager that spawns sacp-conductor and uses the ACP SDK:

```typescript
// src/conductorManager.ts
import * as vscode from 'vscode';
import { spawn, ChildProcess } from 'child_process';
import { ClientSideConnection } from '@agentclientprotocol/sdk';

export interface ChatMessage {
    role: 'user' | 'assistant';
    content: string;
}

export class ConductorManager {
    private process?: ChildProcess;
    private connection?: ClientSideConnection;
    private agent: string;
    private conductorPath: string;
    
    constructor() {
        const config = vscode.workspace.getConfiguration('symposium');
        this.agent = config.get<string>('agent', 'claudeCode');
        this.conductorPath = config.get<string>('conductorPath', 'sacp-conductor');
    }
    
    async start(): Promise<void> {
        if (this.process) {
            throw new Error('Conductor already running');
        }
        
        // Spawn sacp-conductor
        this.process = spawn(this.conductorPath, [
            '--agent', this.agent
        ], {
            stdio: ['pipe', 'pipe', 'pipe']
        });
        
        // Check if process started successfully
        if (!this.process.pid) {
            throw new Error('Failed to start conductor process');
        }
        
        // Initialize ACP ClientSideConnection with stdio streams
        this.connection = new ClientSideConnection({
            input: this.process.stdout!,
            output: this.process.stdin!
        });
        
        // Handle process errors
        this.process.on('error', (error) => {
            console.error('Conductor process error:', error);
            vscode.window.showErrorMessage(
                `Conductor error: ${error.message}`
            );
        });
        
        // Handle process exit
        this.process.on('exit', (code, signal) => {
            console.log(`Conductor exited with code ${code}, signal ${signal}`);
            
            if (code !== 0 && code !== null) {
                vscode.window.showWarningMessage(
                    `Conductor crashed (exit code ${code}). Restarting...`
                );
                // Auto-restart on crash
                setTimeout(() => this.restart(), 1000);
            }
        });
        
        // Handle stderr
        this.process.stderr?.on('data', (data) => {
            console.error('Conductor stderr:', data.toString());
        });
        
        // Wait for connection to be ready
        await this.connection.initialize();
        
        console.log('Conductor started successfully');
    }
    
    async stop(): Promise<void> {
        if (this.connection) {
            await this.connection.shutdown();
            this.connection = undefined;
        }
        
        if (this.process) {
            this.process.kill();
            this.process = undefined;
        }
    }
    
    async restart(): Promise<void> {
        await this.stop();
        await this.start();
    }
    
    async changeAgent(agent: string): Promise<void> {
        this.agent = agent;
        await this.restart();
    }
    
    /**
     * Send a chat message to the agent
     */
    async sendMessage(
        message: string,
        onChunk?: (chunk: string) => void
    ): Promise<string> {
        if (!this.connection) {
            throw new Error('Conductor not connected');
        }
        
        // Use ACP SDK to send chat request
        // The exact method depends on ACP SDK API - this is conceptual
        const response = await this.connection.request('chat/sendMessage', {
            message,
            stream: !!onChunk
        });
        
        if (onChunk && response.stream) {
            // Handle streaming response
            let fullResponse = '';
            for await (const chunk of response.stream) {
                fullResponse += chunk.content;
                onChunk(chunk.content);
            }
            return fullResponse;
        }
        
        return response.content;
    }
    
    /**
     * Get available tools from the agent
     */
    async getTools(): Promise<any[]> {
        if (!this.connection) {
            throw new Error('Conductor not connected');
        }
        
        const response = await this.connection.request('tools/list', {});
        return response.tools;
    }
    
    /**
     * Get agent capabilities
     */
    async getCapabilities(): Promise<any> {
        if (!this.connection) {
            throw new Error('Conductor not connected');
        }
        
        return this.connection.capabilities;
    }
    
    isConnected(): boolean {
        return !!this.connection && !!this.process;
    }
}
```

### 3. Webview Provider

```typescript
// src/chatProvider.ts
import * as vscode from 'vscode';
import { ConductorManager } from './conductorManager';

export class SymposiumChatProvider implements vscode.WebviewViewProvider {
    public static readonly viewType = 'symposium.chatView';
    
    private _view?: vscode.WebviewView;
    private _conductor: ConductorManager;
    
    constructor(
        private readonly _extensionContext: vscode.ExtensionContext
    ) {
        this._conductor = new ConductorManager();
    }
    
    public async resolveWebviewView(
        webviewView: vscode.WebviewView,
        context: vscode.WebviewViewResolveContext,
        _token: vscode.CancellationToken
    ) {
        this._view = webviewView;
        
        webviewView.webview.options = {
            enableScripts: true,
            localResourceRoots: [
                vscode.Uri.joinPath(this._extensionContext.extensionUri, 'media')
            ]
        };
        
        webviewView.webview.html = this._getHtmlForWebview(webviewView.webview);
        
        // Handle messages from webview
        webviewView.webview.onDidReceiveMessage(async (message) => {
            try {
                await this._handleMessage(message);
            } catch (error: any) {
                console.error('Error handling message:', error);
                webviewView.webview.postMessage({
                    type: 'error',
                    error: error.message
                });
            }
        });
        
        // Start conductor when view is resolved
        try {
            await this._conductor.start();
            
            // Send initial state to webview
            webviewView.webview.postMessage({
                type: 'ready',
                agent: this._conductor.isConnected() ? 'connected' : 'disconnected'
            });
        } catch (error: any) {
            vscode.window.showErrorMessage(
                `Failed to start conductor: ${error.message}`
            );
        }
    }
    
    private async _handleMessage(message: any): Promise<void> {
        switch (message.type) {
            case 'sendMessage':
                await this._handleSendMessage(message.text);
                break;
                
            case 'selectAgent':
                await this._handleSelectAgent(message.agent);
                break;
                
            case 'getTools':
                await this._handleGetTools();
                break;
        }
    }
    
    private async _handleSendMessage(text: string): Promise<void> {
        if (!this._view) return;
        
        // Show user message in UI
        this._view.webview.postMessage({
            type: 'message',
            role: 'user',
            content: text
        });
        
        try {
            // Send to conductor and get streaming response
            let fullResponse = '';
            await this._conductor.sendMessage(
                text,
                (chunk) => {
                    fullResponse += chunk;
                    // Send chunk to webview
                    this._view?.webview.postMessage({
                        type: 'messageChunk',
                        role: 'assistant',
                        content: chunk,
                        done: false
                    });
                }
            );
            
            // Send completion signal
            this._view.webview.postMessage({
                type: 'messageChunk',
                role: 'assistant',
                content: '',
                done: true
            });
        } catch (error: any) {
            this._view.webview.postMessage({
                type: 'error',
                error: `Failed to send message: ${error.message}`
            });
        }
    }
    
    private async _handleSelectAgent(agent: string): Promise<void> {
        if (!this._view) return;
        
        try {
            // Update configuration
            await vscode.workspace.getConfiguration('symposium').update(
                'agent',
                agent,
                vscode.ConfigurationTarget.Global
            );
            
            // Restart conductor with new agent
            this._view.webview.postMessage({
                type: 'status',
                status: 'restarting'
            });
            
            await this._conductor.changeAgent(agent);
            
            this._view.webview.postMessage({
                type: 'status',
                status: 'ready',
                agent
            });
            
            vscode.window.showInformationMessage(
                `Switched to ${agent}`
            );
        } catch (error: any) {
            vscode.window.showErrorMessage(
                `Failed to switch agent: ${error.message}`
            );
        }
    }
    
    private async _handleGetTools(): Promise<void> {
        if (!this._view) return;
        
        try {
            const tools = await this._conductor.getTools();
            this._view.webview.postMessage({
                type: 'tools',
                tools
            });
        } catch (error: any) {
            console.error('Failed to get tools:', error);
        }
    }
    
    public dispose(): void {
        this._conductor.stop();
    }
    
    private _getHtmlForWebview(webview: vscode.Webview): string {
        const nonce = getNonce();
        
        return `<!DOCTYPE html>
        <html lang="en">
        <head>
            <meta charset="UTF-8">
            <meta http-equiv="Content-Security-Policy" 
                  content="default-src 'none'; 
                           style-src ${webview.cspSource} 'unsafe-inline'; 
                           script-src 'nonce-${nonce}';">
            <style>
                * { box-sizing: border-box; }
                body { 
                    padding: 0; 
                    margin: 0; 
                    font-family: var(--vscode-font-family);
                    display: flex; 
                    flex-direction: column; 
                    height: 100vh; 
                }
                #header {
                    padding: 10px;
                    border-bottom: 1px solid var(--vscode-panel-border);
                    display: flex;
                    align-items: center;
                    gap: 10px;
                }
                #agent-select {
                    flex: 1;
                    background: var(--vscode-input-background);
                    color: var(--vscode-input-foreground);
                    border: 1px solid var(--vscode-input-border);
                    padding: 4px 8px;
                }
                #messages { 
                    flex: 1; 
                    overflow-y: auto; 
                    padding: 10px; 
                }
                .message {
                    margin-bottom: 10px;
                    padding: 8px;
                    border-radius: 4px;
                }
                .message.user {
                    background: var(--vscode-editor-selectionBackground);
                }
                .message.assistant {
                    background: var(--vscode-editor-inactiveSelectionBackground);
                }
                .message.error {
                    background: var(--vscode-inputValidation-errorBackground);
                    border: 1px solid var(--vscode-inputValidation-errorBorder);
                }
                #input-container { 
                    display: flex; 
                    padding: 10px; 
                    gap: 10px;
                    border-top: 1px solid var(--vscode-panel-border);
                }
                #input { 
                    flex: 1; 
                    background: var(--vscode-input-background);
                    color: var(--vscode-input-foreground);
                    border: 1px solid var(--vscode-input-border);
                    padding: 6px 8px;
                    font-family: inherit;
                }
                #send { 
                    background: var(--vscode-button-background);
                    color: var(--vscode-button-foreground);
                    border: none;
                    padding: 6px 12px;
                    cursor: pointer;
                }
                #send:hover {
                    background: var(--vscode-button-hoverBackground);
                }
                #send:disabled {
                    opacity: 0.5;
                    cursor: not-allowed;
                }
            </style>
        </head>
        <body>
            <div id="header">
                <label for="agent-select">Agent:</label>
                <select id="agent-select">
                    <option value="claudeCode">Claude Code</option>
                    <option value="elizACP">ElizACP</option>
                </select>
            </div>
            
            <div id="messages"></div>
            
            <div id="input-container">
                <input type="text" id="input" placeholder="Type a message..." />
                <button id="send">Send</button>
            </div>
            
            <script nonce="${nonce}">
                const vscode = acquireVsCodeApi();
                
                let currentAssistantMessage = null;
                
                // Handle messages from extension
                window.addEventListener('message', event => {
                    const message = event.data;
                    
                    switch (message.type) {
                        case 'ready':
                            console.log('Conductor ready');
                            break;
                            
                        case 'message':
                            addMessage(message.content, message.role);
                            break;
                            
                        case 'messageChunk':
                            if (message.done) {
                                currentAssistantMessage = null;
                            } else {
                                appendToMessage(message.content);
                            }
                            break;
                            
                        case 'error':
                            addMessage(message.error, 'error');
                            break;
                            
                        case 'status':
                            if (message.status === 'restarting') {
                                addMessage('Restarting conductor...', 'system');
                            } else if (message.status === 'ready') {
                                addMessage(\`Ready with agent: \${message.agent}\`, 'system');
                            }
                            break;
                    }
                });
                
                // Send message on button click
                document.getElementById('send').addEventListener('click', sendMessage);
                
                // Send message on Enter key
                document.getElementById('input').addEventListener('keypress', (e) => {
                    if (e.key === 'Enter') {
                        sendMessage();
                    }
                });
                
                // Handle agent selection
                document.getElementById('agent-select').addEventListener('change', (e) => {
                    vscode.postMessage({
                        type: 'selectAgent',
                        agent: e.target.value
                    });
                });
                
                function sendMessage() {
                    const input = document.getElementById('input');
                    const text = input.value.trim();
                    
                    if (!text) return;
                    
                    vscode.postMessage({
                        type: 'sendMessage',
                        text
                    });
                    
                    input.value = '';
                }
                
                function addMessage(text, role) {
                    const div = document.createElement('div');
                    div.className = \`message \${role}\`;
                    div.textContent = text;
                    document.getElementById('messages').appendChild(div);
                    
                    // Auto-scroll to bottom
                    div.scrollIntoView({ behavior: 'smooth' });
                    
                    if (role === 'assistant') {
                        currentAssistantMessage = div;
                    }
                }
                
                function appendToMessage(text) {
                    if (!currentAssistantMessage) {
                        addMessage(text, 'assistant');
                    } else {
                        currentAssistantMessage.textContent += text;
                        currentAssistantMessage.scrollIntoView({ behavior: 'smooth' });
                    }
                }
            </script>
        </body>
        </html>`;
    }
}

function getNonce() {
    let text = '';
    const possible = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';
    for (let i = 0; i < 32; i++) {
        text += possible.charAt(Math.floor(Math.random() * possible.length));
    }
    return text;
}
```

### 4. Extension Activation

```typescript
// src/extension.ts
import * as vscode from 'vscode';
import { SymposiumChatProvider } from './chatProvider';

export function activate(context: vscode.ExtensionContext) {
    console.log('Symposium extension activating');
    
    const provider = new SymposiumChatProvider(context);
    
    context.subscriptions.push(
        vscode.window.registerWebviewViewProvider(
            SymposiumChatProvider.viewType,
            provider,
            {
                webviewOptions: {
                    retainContextWhenHidden: true
                }
            }
        )
    );
    
    // Register commands
    context.subscriptions.push(
        vscode.commands.registerCommand('symposium.openChat', () => {
            vscode.commands.executeCommand(
                'workbench.view.extension.symposium'
            );
        })
    );
    
    console.log('Symposium extension activated');
}

export function deactivate() {
    console.log('Symposium extension deactivated');
}
```

## Next Steps

1. **Study ACP SDK API**: Review the actual SDK documentation to understand exact API
2. **Check Examples**: Look at the SDK's examples directory for real usage patterns
3. **Test with sacp-conductor**: Verify sacp-conductor implements ACP protocol correctly
4. **Add Features**: Markdown rendering, code blocks, tool results visualization
5. **Error Handling**: Improve error messages and recovery
6. **Configuration**: Add more settings (model selection, temperature, etc.)

## Key Advantages

Using the ACP TypeScript SDK gives us:

1. **Standards Compliance**: Guaranteed to work with any ACP-compliant agent
2. **Future-Proof**: As ACP evolves, SDK updates handle protocol changes
3. **Type Safety**: TypeScript types prevent protocol errors
4. **Tested Code**: SDK is used by Zed and other implementations
5. **Less Code**: Don't need to implement JSON-RPC, streaming, error handling ourselves

## Questions to Answer

1. **What's the exact API?** Need to look at SDK docs/examples to see actual method signatures
2. **How does streaming work?** Does SDK return async generators or callbacks?
3. **What about tools?** How does ACP handle tool calls and results?
4. **Configuration**: What initialization options does ClientSideConnection take?
5. **Error handling**: What error types does SDK throw?

These can be answered by studying the SDK's TypeScript library reference and examples.
