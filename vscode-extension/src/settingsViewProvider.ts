import * as vscode from "vscode";

/**
 * Settings View Provider
 *
 * Provides a minimal settings webview. Agent and mod configuration
 * is handled by the Symposium Rust agent via ConfigAgent.
 */
export class SettingsViewProvider implements vscode.WebviewViewProvider {
  public static readonly viewType = "symposium.settingsView";
  #view?: vscode.WebviewView;
  #extensionUri: vscode.Uri;

  constructor(extensionUri: vscode.Uri) {
    this.#extensionUri = extensionUri;

    // Listen for configuration changes
    vscode.workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration("symposium")) {
        this.#sendConfiguration();
      }
    });
  }

  /**
   * Refresh - minimal implementation since config is in Rust now
   */
  async refreshAvailability(): Promise<void> {
    this.#sendConfiguration();
  }

  public resolveWebviewView(
    webviewView: vscode.WebviewView,
    context: vscode.WebviewViewResolveContext,
    _token: vscode.CancellationToken,
  ) {
    this.#view = webviewView;

    webviewView.webview.options = {
      enableScripts: true,
      localResourceRoots: [this.#extensionUri],
    };

    webviewView.webview.html = this.#getHtmlForWebview(webviewView.webview);

    // Handle webview visibility changes
    webviewView.onDidChangeVisibility(() => {
      if (webviewView.visible) {
        this.#sendConfiguration();
      }
    });

    // Handle messages from the webview
    webviewView.webview.onDidReceiveMessage(async (message) => {
      switch (message.type) {
        case "get-config":
          this.#sendConfiguration();
          break;
        case "open-settings":
          vscode.commands.executeCommand(
            "workbench.action.openSettings",
            "symposium",
          );
          break;
        case "toggle-require-modifier-to-send":
          await this.#toggleRequireModifierToSend();
          break;
      }
    });
  }

  async #toggleRequireModifierToSend() {
    const config = vscode.workspace.getConfiguration("symposium");
    const currentValue = config.get<boolean>("requireModifierToSend", false);
    await config.update(
      "requireModifierToSend",
      !currentValue,
      vscode.ConfigurationTarget.Global,
    );
    this.#sendConfiguration();
  }

  #sendConfiguration() {
    if (!this.#view) {
      return;
    }

    const config = vscode.workspace.getConfiguration("symposium");
    const requireModifierToSend = config.get<boolean>(
      "requireModifierToSend",
      false,
    );

    this.#view.webview.postMessage({
      type: "config",
      requireModifierToSend,
    });
  }

  #getHtmlForWebview(webview: vscode.Webview) {
    return `<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Symposium Settings</title>
    <style>
        body {
            padding: 16px;
            color: var(--vscode-foreground);
            font-family: var(--vscode-font-family);
            font-size: var(--vscode-font-size);
        }
        h2 {
            margin-top: 0;
            margin-bottom: 16px;
            font-size: 16px;
            font-weight: 600;
        }
        .section {
            margin-bottom: 24px;
        }
        .checkbox-item {
            display: flex;
            align-items: flex-start;
            gap: 8px;
            padding: 8px 0;
        }
        .checkbox-item input[type="checkbox"] {
            margin-top: 2px;
            cursor: pointer;
        }
        .checkbox-item label {
            cursor: pointer;
            flex: 1;
        }
        .checkbox-description {
            font-size: 12px;
            color: var(--vscode-descriptionForeground);
            margin-top: 4px;
        }
        .info-text {
            color: var(--vscode-descriptionForeground);
            font-size: 12px;
            line-height: 1.5;
        }
    </style>
</head>
<body>
    <div class="section">
        <h2>Configuration</h2>
        <p class="info-text">
            Agent and mod configuration is managed by Symposium.
            Use the chat to run <code>/config</code> to configure agents and mods.
        </p>
    </div>

    <div class="section">
        <h2>Preferences</h2>
        <div class="checkbox-item">
            <input type="checkbox" id="require-modifier-to-send" />
            <label for="require-modifier-to-send">
                <div>Require modifier key to send</div>
                <div class="checkbox-description">
                    When enabled, Enter adds a newline and Shift+Enter (or Cmd+Enter) sends the prompt.
                </div>
            </label>
        </div>
    </div>

    <div class="section">
        <a href="#" id="configure-link" style="color: var(--vscode-textLink-foreground); text-decoration: none;">
            Edit all settings...
        </a>
    </div>

    <script>
        const vscode = acquireVsCodeApi();

        // Request initial configuration
        vscode.postMessage({ type: 'get-config' });

        // Handle configure link
        document.getElementById('configure-link').onclick = (e) => {
            e.preventDefault();
            vscode.postMessage({ type: 'open-settings' });
        };

        // Handle require modifier to send checkbox
        document.getElementById('require-modifier-to-send').onchange = (e) => {
            vscode.postMessage({ type: 'toggle-require-modifier-to-send' });
        };

        // Handle messages from extension
        window.addEventListener('message', event => {
            const message = event.data;

            if (message.type === 'config') {
                const checkbox = document.getElementById('require-modifier-to-send');
                checkbox.checked = message.requireModifierToSend;
            }
        });
    </script>
</body>
</html>`;
  }
}
