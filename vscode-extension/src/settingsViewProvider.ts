import * as vscode from "vscode";
import {
  getEffectiveAgents,
  getCurrentAgentId,
  AgentConfig,
  checkForRegistryUpdates,
  checkAllBuiltInAvailability,
  AvailabilityStatus,
} from "./agentRegistry";

export class SettingsViewProvider implements vscode.WebviewViewProvider {
  public static readonly viewType = "symposium.settingsView";
  #view?: vscode.WebviewView;
  #extensionUri: vscode.Uri;
  #availabilityCache: Map<string, AvailabilityStatus> = new Map();

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
   * Refresh availability checks for built-in agents.
   * Call this at activation and when the settings panel becomes visible.
   */
  async refreshAvailability(): Promise<void> {
    this.#availabilityCache = await checkAllBuiltInAvailability();
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
        // Refresh availability checks and configuration when view becomes visible
        this.refreshAvailability();
      }
    });

    // Handle messages from the webview
    webviewView.webview.onDidReceiveMessage(async (message) => {
      switch (message.type) {
        case "get-config":
          // Send current configuration to webview
          this.#sendConfiguration();
          break;
        case "set-current-agent":
          // Update current agent setting
          const vsConfig = vscode.workspace.getConfiguration("symposium");
          await vsConfig.update(
            "currentAgentId",
            message.agentId,
            vscode.ConfigurationTarget.Global,
          );
          vscode.window.showInformationMessage(
            `Switched to agent: ${message.agentName}`,
          );
          // Send updated configuration to refresh the UI
          this.#sendConfiguration();
          break;

        case "toggle-bypass-permissions":
          // Toggle bypass permissions for an agent
          await this.#toggleBypassPermissions(message.agentId);
          break;
        case "open-settings":
          // Open VSCode settings focused on Symposium
          vscode.commands.executeCommand(
            "workbench.action.openSettings",
            "symposium",
          );
          break;
        case "add-agent-from-registry":
          // Show the add agent from registry dialog
          vscode.commands.executeCommand("symposium.addAgentFromRegistry");
          break;
        case "check-for-updates":
          // Check for registry updates
          await this.#checkForUpdates();
          break;
        case "toggle-require-modifier-to-send":
          // Toggle the requireModifierToSend setting
          await this.#toggleRequireModifierToSend();
          break;
      }
    });
  }

  async #toggleBypassPermissions(agentId: string) {
    const config = vscode.workspace.getConfiguration("symposium");
    const bypassList = config.get<string[]>("bypassPermissions", []);

    // Get the agent to find its display name
    const effectiveAgents = getEffectiveAgents();
    const agent = effectiveAgents.find((a) => a.id === agentId);
    const displayName = agent?.name ?? agentId;

    const isCurrentlyBypassed = bypassList.includes(agentId);
    const newList = isCurrentlyBypassed
      ? bypassList.filter((id) => id !== agentId)
      : [...bypassList, agentId];

    await config.update(
      "bypassPermissions",
      newList,
      vscode.ConfigurationTarget.Global,
    );
    vscode.window.showInformationMessage(
      `${displayName}: Bypass permissions ${!isCurrentlyBypassed ? "enabled" : "disabled"}`,
    );
    this.#sendConfiguration();
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

  async #checkForUpdates() {
    const result = await vscode.window.withProgress(
      {
        location: vscode.ProgressLocation.Notification,
        title: "Checking for agent updates...",
        cancellable: false,
      },
      async () => {
        try {
          return await checkForRegistryUpdates();
        } catch (error) {
          vscode.window.showErrorMessage(
            `Failed to check for updates: ${error instanceof Error ? error.message : String(error)}`,
          );
          return null;
        }
      },
    );

    if (result === null) {
      return;
    }

    if (result.updated.length === 0) {
      vscode.window.showInformationMessage("All agents are up to date.");
    } else {
      vscode.window.showInformationMessage(
        `Updated ${result.updated.length} agent(s): ${result.updated.join(", ")}`,
      );
      // Refresh the UI to show new versions
      this.#sendConfiguration();
    }
  }

  #sendConfiguration() {
    if (!this.#view) {
      return;
    }

    const config = vscode.workspace.getConfiguration("symposium");
    const bypassList = config.get<string[]>("bypassPermissions", []);

    // Get effective agents (built-ins + settings) and merge bypass/availability settings
    const effectiveAgents = getEffectiveAgents();
    const agents = effectiveAgents.map((agent) => {
      const availability = this.#availabilityCache.get(agent.id);
      return {
        ...agent,
        bypassPermissions: bypassList.includes(agent.id),
        // Only built-in agents have availability checks (registry agents are always available)
        disabled: availability ? !availability.available : false,
        disabledReason: availability?.reason,
      };
    });

    const currentAgentId = getCurrentAgentId();
    const requireModifierToSend = config.get<boolean>(
      "requireModifierToSend",
      false,
    );

    this.#view.webview.postMessage({
      type: "config",
      agents,
      currentAgentId,
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
        .agent-list {
            display: flex;
            flex-direction: column;
            gap: 8px;
        }
        .agent-item {
            padding: 8px 12px;
            background: var(--vscode-list-inactiveSelectionBackground);
            border-radius: 4px;
            cursor: pointer;
            display: flex;
            justify-content: space-between;
            align-items: center;
        }
        .agent-name {
            display: flex;
            align-items: baseline;
            gap: 6px;
        }
        .agent-version {
            font-size: 11px;
            color: var(--vscode-descriptionForeground);
        }
        .agent-item:hover {
            background: var(--vscode-list-hoverBackground);
        }
        .agent-item.active {
            background: var(--vscode-list-activeSelectionBackground);
            color: var(--vscode-list-activeSelectionForeground);
        }
        .badges {
            display: flex;
            gap: 6px;
            align-items: center;
        }
        .badge {
            padding: 2px 8px;
            border-radius: 12px;
            font-size: 11px;
            background: var(--vscode-badge-background);
            color: var(--vscode-badge-foreground);
        }
        .badge.bypass {
            background: var(--vscode-inputValidation-warningBackground);
            color: var(--vscode-inputValidation-warningForeground);
            cursor: pointer;
        }
        .badge.bypass:hover {
            opacity: 0.8;
        }
        .agent-item.disabled {
            opacity: 0.5;
            cursor: default;
        }
        .agent-item.disabled:hover {
            background: var(--vscode-list-inactiveSelectionBackground);
        }
        .disabled-reason {
            font-size: 11px;
            color: var(--vscode-descriptionForeground);
            font-style: italic;
        }
        .toggle {
            font-size: 11px;
            color: var(--vscode-descriptionForeground);
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
    </style>
</head>
<body>
    <div class="section">
        <h2>Current Agent</h2>
        <div class="agent-list" id="agent-list">
            <div>Loading...</div>
        </div>
        <div style="margin-top: 12px; display: flex; flex-direction: column; gap: 6px;">
            <a href="#" id="add-agent-link" style="color: var(--vscode-textLink-foreground); text-decoration: none;">
                + Add agent from registry...
            </a>
            <a href="#" id="check-updates-link" style="color: var(--vscode-textLink-foreground); text-decoration: none;">
                ↻ Check for updates
            </a>
        </div>
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
            Configure agents...
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

        // Handle add agent link
        document.getElementById('add-agent-link').onclick = (e) => {
            e.preventDefault();
            vscode.postMessage({ type: 'add-agent-from-registry' });
        };

        // Handle check for updates link
        document.getElementById('check-updates-link').onclick = (e) => {
            e.preventDefault();
            vscode.postMessage({ type: 'check-for-updates' });
        };

        // Handle require modifier to send checkbox
        document.getElementById('require-modifier-to-send').onchange = (e) => {
            vscode.postMessage({ type: 'toggle-require-modifier-to-send' });
        };

        // Handle messages from extension
        window.addEventListener('message', event => {
            const message = event.data;

            if (message.type === 'config') {
                renderAgents(message.agents, message.currentAgentId);
                renderPreferences(message.requireModifierToSend);
            }
        });

        function renderPreferences(requireModifierToSend) {
            const checkbox = document.getElementById('require-modifier-to-send');
            checkbox.checked = requireModifierToSend;
        }

        function renderAgents(agents, currentAgentId) {
            const list = document.getElementById('agent-list');
            list.innerHTML = '';

            for (const agent of agents) {
                const displayName = agent.name || agent.id;
                const isActive = agent.id === currentAgentId;
                const isDisabled = agent.disabled;

                const item = document.createElement('div');
                item.className = 'agent-item' + (isActive ? ' active' : '') + (isDisabled ? ' disabled' : '');

                const badges = [];
                if (isActive) {
                    badges.push('<span class="badge">Active</span>');
                }
                if (agent.bypassPermissions && !isDisabled) {
                    badges.push('<span class="badge bypass" title="Click to disable bypass permissions">Bypass Permissions</span>');
                }

                const versionHtml = agent.version
                    ? \`<span class="agent-version">v\${agent.version}</span>\`
                    : '';

                const disabledHtml = isDisabled && agent.disabledReason
                    ? \`<span class="disabled-reason">(\${agent.disabledReason})</span>\`
                    : '';

                item.innerHTML = \`
                    <div class="agent-name">
                        <span>\${displayName}</span>
                        \${versionHtml}
                        \${disabledHtml}
                    </div>
                    <div class="badges">\${badges.join('')}</div>
                \`;

                // Handle clicking on the agent name (switch agent) - only if not disabled
                if (!isDisabled) {
                    const nameSpan = item.querySelector('.agent-name');
                    nameSpan.onclick = (e) => {
                        e.stopPropagation();
                        vscode.postMessage({ type: 'set-current-agent', agentId: agent.id, agentName: displayName });
                    };

                    // Handle clicking on the bypass badge (toggle bypass)
                    const bypassBadge = item.querySelector('.badge.bypass');
                    if (bypassBadge) {
                        bypassBadge.onclick = (e) => {
                            e.stopPropagation();
                            vscode.postMessage({ type: 'toggle-bypass-permissions', agentId: agent.id });
                        };
                    }
                }

                list.appendChild(item);
            }
        }


    </script>
</body>
</html>`;
  }
}
