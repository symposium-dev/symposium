import * as vscode from "vscode";
import { Actor } from "./actors/actor";
import { HomerActor } from "./actors/homerActor";

/**
 * Message types for webview ↔ extension communication
 */
interface WebviewMessage {
  type: "sendPrompt";
  tabId: string;
  prompt: string;
  messageId: string;
}

interface ExtensionMessage {
  type: "streamStart" | "streamChunk" | "streamEnd";
  tabId: string;
  messageId: string;
  content?: string;
}

/**
 * Manages the Symposium chat webview panel.
 * Handles webview lifecycle, message routing, and actor communication.
 */
export class SymposiumPanel {
  public static currentPanel: SymposiumPanel | undefined;
  public static readonly viewType = "symposium.chatView";

  private readonly _panel: vscode.WebviewView;
  private readonly _extensionUri: vscode.Uri;
  private readonly _actor: Actor;
  private _disposables: vscode.Disposable[] = [];

  public static createOrShow(
    webviewView: vscode.WebviewView,
    extensionUri: vscode.Uri,
  ): SymposiumPanel {
    // If we already have a panel, return it
    if (SymposiumPanel.currentPanel) {
      return SymposiumPanel.currentPanel;
    }

    // Otherwise, create a new panel
    const panel = new SymposiumPanel(webviewView, extensionUri);
    SymposiumPanel.currentPanel = panel;
    return panel;
  }

  private constructor(
    webviewView: vscode.WebviewView,
    extensionUri: vscode.Uri,
  ) {
    console.log("[SymposiumPanel] Constructing panel");
    this._panel = webviewView;
    this._extensionUri = extensionUri;
    this._actor = new HomerActor();

    // Set up webview options
    this._panel.webview.options = {
      enableScripts: true,
      localResourceRoots: [this._extensionUri],
    };

    // Set the HTML content
    this._panel.webview.html = this._getHtmlForWebview(this._panel.webview);
    console.log("[SymposiumPanel] HTML set, webview initialized");

    // Listen for messages from the webview
    this._panel.webview.onDidReceiveMessage(
      (message: WebviewMessage) => {
        console.log("[SymposiumPanel] Received message from webview:", message);
        this._handleWebviewMessage(message);
      },
      null,
      this._disposables,
    );

    // Clean up when panel is disposed
    this._panel.onDidDispose(() => this.dispose(), null, this._disposables);
  }

  private async _handleWebviewMessage(message: WebviewMessage) {
    switch (message.type) {
      case "sendPrompt":
        await this._handleSendPrompt(message);
        break;
    }
  }

  private async _handleSendPrompt(message: WebviewMessage) {
    const { tabId, prompt, messageId } = message;
    console.log("[SymposiumPanel] Handling prompt:", {
      tabId,
      prompt,
      messageId,
    });

    // Send stream start message
    this._postMessage({
      type: "streamStart",
      tabId,
      messageId,
    });
    console.log("[SymposiumPanel] Sent streamStart");

    try {
      // Get response stream from actor
      const responseStream = this._actor.sendPrompt(prompt);
      console.log("[SymposiumPanel] Got response stream from actor");

      // Stream chunks to webview
      let chunkCount = 0;
      for await (const chunk of responseStream) {
        chunkCount++;
        this._postMessage({
          type: "streamChunk",
          tabId,
          messageId,
          content: chunk,
        });
      }
      console.log(`[SymposiumPanel] Streamed ${chunkCount} chunks`);

      // Send stream end message
      this._postMessage({
        type: "streamEnd",
        tabId,
        messageId,
      });
      console.log("[SymposiumPanel] Sent streamEnd");
    } catch (error) {
      console.error("[SymposiumPanel] Error handling prompt:", error);
      // Send error as final chunk and end stream
      this._postMessage({
        type: "streamChunk",
        tabId,
        messageId,
        content: `Error: ${error instanceof Error ? error.message : "Unknown error"}`,
      });
      this._postMessage({
        type: "streamEnd",
        tabId,
        messageId,
      });
    }
  }

  private _postMessage(message: ExtensionMessage) {
    this._panel.webview.postMessage(message);
  }

  private _getHtmlForWebview(webview: vscode.Webview): string {
    // Get the URI for our webview script
    const scriptUri = webview.asWebviewUri(
      vscode.Uri.joinPath(this._extensionUri, "out", "webview.js"),
    );

    // Use a nonce to allow only specific scripts
    const nonce = getNonce();

    return `<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${webview.cspSource} 'unsafe-inline'; script-src 'nonce-${nonce}'; img-src ${webview.cspSource} data: https:; font-src ${webview.cspSource};">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Symposium Chat</title>
    <style>
        :root {
            /* Basic mynah-ui theme variables - using VSCode dark theme colors */
            --mynah-color-bg: var(--vscode-editor-background);
            --mynah-color-tab-active: var(--vscode-tab-activeBackground);
            --mynah-color-text-default: var(--vscode-editor-foreground);
            --mynah-color-text-strong: var(--vscode-editor-foreground);
            --mynah-color-text-weak: var(--vscode-descriptionForeground);
            --mynah-color-text-link: var(--vscode-textLink-foreground);
            --mynah-color-text-input: var(--vscode-input-foreground);
            --mynah-color-button: var(--vscode-button-background);
            --mynah-color-button-reverse: var(--vscode-button-foreground);
            --mynah-color-border-default: var(--vscode-panel-border);
            --mynah-font-family: var(--vscode-font-family);
            --mynah-font-size-small: var(--vscode-font-size);
            --mynah-font-size-medium: calc(var(--vscode-font-size) * 1.1);
            --mynah-font-size-large: calc(var(--vscode-font-size) * 1.25);
            --mynah-line-height: 1.5;
            --mynah-syntax-bg: var(--vscode-textCodeBlock-background);
            --mynah-card-bg: var(--vscode-editor-background);
            --mynah-shadow-button-stroke: none;
        }
        html, body {
            margin: 0;
            padding: 0;
            height: 100%;
            overflow: hidden;
            background-color: var(--vscode-editor-background);
            color: var(--vscode-editor-foreground);
        }
        #mynah-root {
            height: 100%;
            width: 100%;
        }
    </style>
</head>
<body>
    <div id="mynah-root"></div>
    <script nonce="${nonce}" src="${scriptUri}"></script>
</body>
</html>`;
  }

  public dispose() {
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

function getNonce() {
  let text = "";
  const possible =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
  for (let i = 0; i < 32; i++) {
    text += possible.charAt(Math.floor(Math.random() * possible.length));
  }
  return text;
}
