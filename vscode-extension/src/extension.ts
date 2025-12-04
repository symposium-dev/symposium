import * as vscode from "vscode";
import { ChatViewProvider } from "./chatViewProvider";
import { SettingsViewProvider } from "./settingsViewProvider";
import { DiscussCodeActionProvider } from "./discussCodeActionProvider";
import { Logger } from "./logger";
import { v4 as uuidv4 } from "uuid";

// Global logger instance
export const logger = new Logger("Symposium");

// Export for testing
let chatProviderForTesting: ChatViewProvider | undefined;

export function getChatProviderForTesting(): ChatViewProvider | undefined {
  return chatProviderForTesting;
}

export function activate(context: vscode.ExtensionContext) {
  logger.important("extension", "Symposium extension is now active");

  // Generate extension activation ID for this VSCode session
  const extensionActivationId = uuidv4();
  logger.debug("extension", "Generated extension activation ID", {
    extensionActivationId,
  });

  // Register the chat webview view provider
  const chatProvider = new ChatViewProvider(
    context.extensionUri,
    context,
    extensionActivationId,
  );
  chatProviderForTesting = chatProvider; // Store for testing
  context.subscriptions.push(
    vscode.window.registerWebviewViewProvider(
      ChatViewProvider.viewType,
      chatProvider,
    ),
  );

  // Register the settings webview view provider
  const settingsProvider = new SettingsViewProvider(context.extensionUri);
  context.subscriptions.push(
    vscode.window.registerWebviewViewProvider(
      SettingsViewProvider.viewType,
      settingsProvider,
    ),
  );

  // Register the command to open chat
  context.subscriptions.push(
    vscode.commands.registerCommand("symposium.openChat", () => {
      vscode.commands.executeCommand("symposium.chatView.focus");
    }),
  );

  // Debug command to inspect saved state
  context.subscriptions.push(
    vscode.commands.registerCommand("symposium.inspectState", async () => {
      const state = context.workspaceState.get("symposium.chatState");
      const stateJson = JSON.stringify(state, null, 2);
      const doc = await vscode.workspace.openTextDocument({
        content: stateJson,
        language: "json",
      });
      await vscode.window.showTextDocument(doc);
    }),
  );

  // Register "Discuss in Symposium" code action provider
  context.subscriptions.push(
    vscode.languages.registerCodeActionsProvider(
      "*", // All file types
      new DiscussCodeActionProvider(),
      {
        providedCodeActionKinds:
          DiscussCodeActionProvider.providedCodeActionKinds,
      },
    ),
  );

  // Register command for "Discuss in Symposium" code action
  context.subscriptions.push(
    vscode.commands.registerCommand("symposium.discussSelection", async () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor || editor.selection.isEmpty) {
        logger.debug("command", "discussSelection: no selection");
        return;
      }

      // Capture the selection now (frozen)
      const selection = editor.selection;
      const text = editor.document.getText(selection);
      const filePath = editor.document.uri.fsPath;

      // Get relative path
      const workspaceFolder = vscode.workspace.getWorkspaceFolder(
        editor.document.uri,
      );
      let relativePath = filePath;
      if (workspaceFolder) {
        const prefix = workspaceFolder.uri.fsPath;
        if (filePath.startsWith(prefix)) {
          relativePath = filePath.slice(prefix.length);
          if (relativePath.startsWith("/") || relativePath.startsWith("\\")) {
            relativePath = relativePath.slice(1);
          }
        }
      }

      const selectionData = {
        filePath,
        relativePath,
        startLine: selection.start.line + 1, // 1-indexed
        endLine: selection.end.line + 1,
        text,
      };

      logger.debug("command", "discussSelection triggered", {
        path: relativePath,
        lines: `${selectionData.startLine}-${selectionData.endLine}`,
      });

      // Focus the chat panel
      await vscode.commands.executeCommand("symposium.chatView.focus");

      // Add the selection to the prompt
      chatProvider.addSelectionToPrompt(selectionData);
    }),
  );

  // Testing commands - only for integration tests
  context.subscriptions.push(
    vscode.commands.registerCommand(
      "symposium.test.simulateNewTab",
      async (tabId: string) => {
        await chatProvider.simulateWebviewMessage({ type: "new-tab", tabId });
      },
    ),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("symposium.test.getTabs", () => {
      return chatProvider.getTabsForTesting();
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand(
      "symposium.test.sendPrompt",
      async (tabId: string, prompt: string) => {
        await chatProvider.simulateWebviewMessage({
          type: "prompt",
          tabId,
          prompt,
        });
      },
    ),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand(
      "symposium.test.startCapturingResponses",
      (tabId: string) => {
        chatProvider.startCapturingResponses(tabId);
      },
    ),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand(
      "symposium.test.getResponse",
      (tabId: string) => {
        return chatProvider.getResponse(tabId);
      },
    ),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand(
      "symposium.test.stopCapturingResponses",
      (tabId: string) => {
        chatProvider.stopCapturingResponses(tabId);
      },
    ),
  );
}

export function deactivate() {}
