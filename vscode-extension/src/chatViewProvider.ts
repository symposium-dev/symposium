import * as vscode from 'vscode';

export class ChatViewProvider implements vscode.WebviewViewProvider {
    public static readonly viewType = 'symposium.chatView';

    constructor(private readonly _extensionUri: vscode.Uri) {}

    public resolveWebviewView(
        webviewView: vscode.WebviewView,
        context: vscode.WebviewViewResolveContext,
        _token: vscode.CancellationToken
    ) {
        webviewView.webview.options = {
            enableScripts: true,
            localResourceRoots: [this._extensionUri]
        };

        webviewView.webview.html = this._getHtmlForWebview(webviewView.webview);
    }

    private _getHtmlForWebview(webview: vscode.Webview) {
        return `<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Symposium Chat</title>
    <style>
        body {
            padding: 10px;
            color: var(--vscode-foreground);
            font-family: var(--vscode-font-family);
        }
        h1 {
            font-size: 1.2em;
            margin: 0 0 10px 0;
        }
    </style>
</head>
<body>
    <h1>Symposium Chat</h1>
    <p>Chat interface coming soon...</p>
</body>
</html>`;
    }
}
