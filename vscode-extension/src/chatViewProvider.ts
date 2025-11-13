import * as vscode from "vscode";
import { HomerActor } from "./homerActor";

export class ChatViewProvider implements vscode.WebviewViewProvider {
  public static readonly viewType = "symposium.chatView";
  private static readonly STATE_KEY = "symposium.chatState";
  private _view?: vscode.WebviewView;
  private _actor: HomerActor;

  constructor(
    private readonly _extensionUri: vscode.Uri,
    private readonly _context: vscode.ExtensionContext,
  ) {
    this._actor = new HomerActor();
  }

  public resolveWebviewView(
    webviewView: vscode.WebviewView,
    context: vscode.WebviewViewResolveContext,
    _token: vscode.CancellationToken,
  ) {
    this._view = webviewView;

    webviewView.webview.options = {
      enableScripts: true,
      localResourceRoots: [this._extensionUri],
    };

    webviewView.webview.html = this._getHtmlForWebview(webviewView.webview);

    // Handle messages from the webview
    webviewView.webview.onDidReceiveMessage(async (message) => {
      switch (message.type) {
        case "prompt":
          // Stream the response progressively
          for await (const chunk of this._actor.processPrompt(message.prompt)) {
            webviewView.webview.postMessage({
              type: "response-chunk",
              messageId: message.messageId,
              chunk: chunk,
            });
          }
          // Send final message to indicate streaming is complete
          webviewView.webview.postMessage({
            type: "response-complete",
            messageId: message.messageId,
          });
          break;

        case "save-state":
          // Save the state to workspace storage
          console.log("Saving webview state:", message.state);
          await this._context.workspaceState.update(
            ChatViewProvider.STATE_KEY,
            message.state,
          );
          break;

        case "request-saved-state":
          // Webview is requesting saved state on initialization
          const savedState = this._context.workspaceState.get(
            ChatViewProvider.STATE_KEY,
          );
          console.log("Sending saved state to webview:", savedState);
          webviewView.webview.postMessage({
            type: "restore-state",
            state: savedState,
          });
          break;
      }
    });
  }

  private _getHtmlForWebview(webview: vscode.Webview) {
    const scriptUri = webview.asWebviewUri(
      vscode.Uri.joinPath(this._extensionUri, "out", "webview.js"),
    );

    return `<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Symposium Chat</title>
    <style>
        body {
            margin: 0;
            padding: 0;
            overflow: hidden;
        }
        #mynah-root {
            width: 100%;
            height: 100vh;
        }
    </style>
</head>
<body>
    <div id="mynah-root"></div>
    <script src="${scriptUri}"></script>
</body>
</html>`;
  }
}
