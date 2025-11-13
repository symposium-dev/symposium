"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
exports.SymposiumPanel = void 0;
const vscode = __importStar(require("vscode"));
const homerActor_1 = require("./actors/homerActor");
/**
 * Manages the Symposium chat webview panel.
 * Handles webview lifecycle, message routing, and actor communication.
 */
class SymposiumPanel {
    static currentPanel;
    static viewType = 'symposium.chatView';
    _panel;
    _extensionUri;
    _actor;
    _disposables = [];
    static createOrShow(webviewView, extensionUri) {
        // If we already have a panel, return it
        if (SymposiumPanel.currentPanel) {
            return SymposiumPanel.currentPanel;
        }
        // Otherwise, create a new panel
        const panel = new SymposiumPanel(webviewView, extensionUri);
        SymposiumPanel.currentPanel = panel;
        return panel;
    }
    constructor(webviewView, extensionUri) {
        this._panel = webviewView;
        this._extensionUri = extensionUri;
        this._actor = new homerActor_1.HomerActor();
        // Set up webview options
        this._panel.webview.options = {
            enableScripts: true,
            localResourceRoots: [this._extensionUri]
        };
        // Set the HTML content
        this._panel.webview.html = this._getHtmlForWebview(this._panel.webview);
        // Listen for messages from the webview
        this._panel.webview.onDidReceiveMessage((message) => this._handleWebviewMessage(message), null, this._disposables);
        // Clean up when panel is disposed
        this._panel.onDidDispose(() => this.dispose(), null, this._disposables);
    }
    async _handleWebviewMessage(message) {
        switch (message.type) {
            case 'sendPrompt':
                await this._handleSendPrompt(message);
                break;
        }
    }
    async _handleSendPrompt(message) {
        const { tabId, prompt, messageId } = message;
        // Send stream start message
        this._postMessage({
            type: 'streamStart',
            tabId,
            messageId
        });
        try {
            // Get response stream from actor
            const responseStream = this._actor.sendPrompt(prompt);
            // Stream chunks to webview
            for await (const chunk of responseStream) {
                this._postMessage({
                    type: 'streamChunk',
                    tabId,
                    messageId,
                    content: chunk
                });
            }
            // Send stream end message
            this._postMessage({
                type: 'streamEnd',
                tabId,
                messageId
            });
        }
        catch (error) {
            console.error('Error handling prompt:', error);
            // Send error as final chunk and end stream
            this._postMessage({
                type: 'streamChunk',
                tabId,
                messageId,
                content: `Error: ${error instanceof Error ? error.message : 'Unknown error'}`
            });
            this._postMessage({
                type: 'streamEnd',
                tabId,
                messageId
            });
        }
    }
    _postMessage(message) {
        this._panel.webview.postMessage(message);
    }
    _getHtmlForWebview(webview) {
        // Get the URI for our webview script
        const scriptUri = webview.asWebviewUri(vscode.Uri.joinPath(this._extensionUri, 'out', 'webview.js'));
        // Use a nonce to allow only specific scripts
        const nonce = getNonce();
        return `<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${webview.cspSource} 'unsafe-inline'; script-src 'nonce-${nonce}';">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Symposium Chat</title>
</head>
<body>
    <div id="mynah-root"></div>
    <script nonce="${nonce}" src="${scriptUri}"></script>
</body>
</html>`;
    }
    dispose() {
        SymposiumPanel.currentPanel = undefined;
        // Clean up resources
        while (this._disposables.length) {
            const disposable = this._disposables.pop();
            if (disposable) {
                disposable.dispose();
            }
        }
    }
}
exports.SymposiumPanel = SymposiumPanel;
function getNonce() {
    let text = '';
    const possible = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';
    for (let i = 0; i < 32; i++) {
        text += possible.charAt(Math.floor(Math.random() * possible.length));
    }
    return text;
}
//# sourceMappingURL=symposiumPanel.js.map