import * as vscode from 'vscode';
import { ChatViewProvider } from './chatViewProvider';

export function activate(context: vscode.ExtensionContext) {
    console.log('Symposium extension is now active');

    // Register the webview view provider
    const provider = new ChatViewProvider(context.extensionUri);
    context.subscriptions.push(
        vscode.window.registerWebviewViewProvider(ChatViewProvider.viewType, provider)
    );

    // Register the command to open chat
    context.subscriptions.push(
        vscode.commands.registerCommand('symposium.openChat', () => {
            vscode.commands.executeCommand('symposium.chatView.focus');
        })
    );
}

export function deactivate() {}
