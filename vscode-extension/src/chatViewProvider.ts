import * as vscode from "vscode";
import { SymposiumPanel } from "./symposiumPanel";

export class ChatViewProvider implements vscode.WebviewViewProvider {
  public static readonly viewType = "symposium.chatView";

  constructor(private readonly _extensionUri: vscode.Uri) {}

  public resolveWebviewView(
    webviewView: vscode.WebviewView,
    context: vscode.WebviewViewResolveContext,
    _token: vscode.CancellationToken,
  ) {
    // Create or show the Symposium panel
    SymposiumPanel.createOrShow(webviewView, this._extensionUri);
  }
}
