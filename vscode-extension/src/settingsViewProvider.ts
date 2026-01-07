import * as vscode from "vscode";
import {
  getEffectiveAgents,
  getCurrentAgentId,
  AgentConfig,
  checkForRegistryUpdates,
  checkAllBuiltInAvailability,
  AvailabilityStatus,
  fetchRegistry,
  addAgentFromRegistry,
  RegistryEntry,
  fetchRegistryExtensions,
} from "./agentRegistry";
import {
  ExtensionSettingsEntry,
  ExtensionRegistryEntry,
  getExtensionsFromSettings,
  getExtensionDisplayInfo,
  saveExtensions,
  showAddExtensionDialog,
} from "./extensionRegistry";

export class SettingsViewProvider implements vscode.WebviewViewProvider {
  public static readonly viewType = "symposium.settingsView";
  #view?: vscode.WebviewView;
  #extensionUri: vscode.Uri;
  #availabilityCache: Map<string, AvailabilityStatus> = new Map();
  #registryCache: RegistryEntry[] = [];
  #extensionRegistryCache: ExtensionRegistryEntry[] = [];

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
   * Refresh availability checks for built-in agents and fetch registry.
   * Call this at activation and when the settings panel becomes visible.
   */
  async refreshAvailability(): Promise<void> {
    // Fetch all in parallel
    const [availability, registry, extensionRegistry] = await Promise.all([
      checkAllBuiltInAvailability(),
      fetchRegistry().catch(() => [] as RegistryEntry[]),
      fetchRegistryExtensions().catch(() => [] as ExtensionRegistryEntry[]),
    ]);
    this.#availabilityCache = availability;
    this.#registryCache = registry;
    this.#extensionRegistryCache = extensionRegistry;
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
          // If not installed, install from registry first
          if (!message.installed) {
            const registryEntry = this.#registryCache.find(
              (e) => e.id === message.agentId,
            );
            if (registryEntry) {
              await addAgentFromRegistry(registryEntry);
            }
          }
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
        case "toggle-extension":
          // Toggle an extension's enabled state
          await this.#toggleExtension(message.extensionId);
          break;
        case "delete-extension":
          // Delete an extension from the list
          await this.#deleteExtension(message.extensionId);
          break;
        case "add-extension":
          // Show the add extension dialog
          await this.#showAddExtensionDialog();
          break;
        case "reorder-extensions":
          // Reorder extensions
          await this.#reorderExtensions(message.extensions);
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

  async #toggleExtension(extensionId: string) {
    const extensions = getExtensionsFromSettings();
    const newExtensions = extensions.map((ext) =>
      ext.id === extensionId ? { ...ext, _enabled: !ext._enabled } : ext,
    );
    await saveExtensions(newExtensions);
    this.#sendConfiguration();
  }

  async #deleteExtension(extensionId: string) {
    const extensions = getExtensionsFromSettings();
    const newExtensions = extensions.filter((ext) => ext.id !== extensionId);
    await saveExtensions(newExtensions);
    this.#sendConfiguration();
  }

  async #showAddExtensionDialog() {
    const added = await showAddExtensionDialog(this.#extensionRegistryCache);
    if (added) {
      this.#sendConfiguration();
    }
  }

  async #reorderExtensions(newOrder: Array<{ id: string; enabled: boolean }>) {
    const extensions = getExtensionsFromSettings();

    // Preserve full entries, just reorder and update enabled state
    // Note: webview sends 'enabled', we store as '_enabled'
    const byId = new Map(extensions.map((e) => [e.id, e]));
    const reordered = newOrder
      .map((item) => {
        const entry = byId.get(item.id);
        if (entry) {
          return { ...entry, _enabled: item.enabled };
        }
        return null;
      })
      .filter((e): e is ExtensionSettingsEntry => e !== null);

    await saveExtensions(reordered);
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
    const effectiveIds = new Set(effectiveAgents.map((a) => a.id));

    const installedAgents = effectiveAgents.map((agent) => {
      const availability = this.#availabilityCache.get(agent.id);
      return {
        ...agent,
        bypassPermissions: bypassList.includes(agent.id),
        // Only built-in agents have availability checks (registry agents are always available)
        disabled: availability ? !availability.available : false,
        disabledReason: availability?.reason,
        installed: true,
      };
    });

    // Add uninstalled registry agents
    const uninstalledAgents = this.#registryCache
      .filter((entry) => !effectiveIds.has(entry.id))
      .map((entry) => ({
        ...entry,
        bypassPermissions: false,
        disabled: false,
        disabledReason: undefined,
        installed: false,
      }));

    const agents = [...installedAgents, ...uninstalledAgents].sort((a, b) =>
      (a.name ?? a.id).localeCompare(b.name ?? b.id),
    );

    const currentAgentId = getCurrentAgentId();
    const requireModifierToSend = config.get<boolean>(
      "requireModifierToSend",
      false,
    );

    // Get extensions configuration
    const extensions = getExtensionsFromSettings();

    // Build extensions data with display info
    const extensionsWithInfo = extensions.map((ext) => {
      const displayInfo = getExtensionDisplayInfo(
        ext,
        this.#extensionRegistryCache,
      );
      return {
        id: ext.id,
        enabled: ext._enabled,
        name: displayInfo.name,
        description: displayInfo.description,
        source: ext._source,
      };
    });

    this.#view.webview.postMessage({
      type: "config",
      agents,
      currentAgentId,
      requireModifierToSend,
      extensions: extensionsWithInfo,
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
        /* Extension list styles */
        .extension-list {
            display: flex;
            flex-direction: column;
            gap: 4px;
        }
        .extension-item {
            padding: 8px 12px;
            background: var(--vscode-list-inactiveSelectionBackground);
            border-radius: 4px;
            display: flex;
            align-items: center;
            gap: 8px;
        }
        .extension-item.disabled {
            opacity: 0.5;
        }
        .extension-item .drag-handle {
            cursor: grab;
            color: var(--vscode-descriptionForeground);
            padding: 0 4px;
        }
        .extension-item .drag-handle:active {
            cursor: grabbing;
        }
        .extension-item.dragging {
            opacity: 0.5;
            background: var(--vscode-list-activeSelectionBackground);
        }
        .extension-item.drag-over {
            border-top: 2px solid var(--vscode-focusBorder);
        }
        .extension-checkbox {
            cursor: pointer;
        }
        .extension-info {
            flex: 1;
            min-width: 0;
        }
        .extension-name {
            font-weight: 500;
        }
        .extension-description {
            font-size: 11px;
            color: var(--vscode-descriptionForeground);
            white-space: nowrap;
            overflow: hidden;
            text-overflow: ellipsis;
        }
        .extension-delete {
            cursor: pointer;
            color: var(--vscode-descriptionForeground);
            padding: 2px 6px;
            border-radius: 3px;
        }
        .extension-delete:hover {
            background: var(--vscode-toolbar-hoverBackground);
            color: var(--vscode-errorForeground);
        }
        .add-extension-link {
            color: var(--vscode-textLink-foreground);
            text-decoration: none;
            display: inline-block;
            margin-top: 8px;
        }
        .add-extension-link:hover {
            text-decoration: underline;
        }

        .no-extensions {
            color: var(--vscode-descriptionForeground);
            font-style: italic;
            padding: 8px 0;
        }
    </style>
</head>
<body>
    <div class="section">
        <h2>Current Agent</h2>
        <div class="agent-list" id="agent-list">
            <div>Loading...</div>
        </div>
        <div style="margin-top: 12px;">
            <a href="#" id="check-updates-link" style="color: var(--vscode-textLink-foreground); text-decoration: none;">
                ↻ Check for updates
            </a>
        </div>
    </div>

    <div class="section">
        <h2>Extensions</h2>
        <div class="extension-list" id="extension-list">
            <div>Loading...</div>
        </div>
        <a href="#" id="add-extension-link" class="add-extension-link" style="display: none;">
            + Add extension
        </a>

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
                renderExtensions(message.extensions);
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
                        vscode.postMessage({ type: 'set-current-agent', agentId: agent.id, agentName: displayName, installed: agent.installed });
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

        // Current extensions state for drag-and-drop
        let currentExtensions = [];

        function renderExtensions(extensions) {
            currentExtensions = extensions;
            const list = document.getElementById('extension-list');
            const addLink = document.getElementById('add-extension-link');

            list.innerHTML = '';

            if (extensions.length === 0) {
                list.innerHTML = '<div class="no-extensions">No extensions configured</div>';
            } else {
                for (let i = 0; i < extensions.length; i++) {
                    const ext = extensions[i];
                    const item = document.createElement('div');
                    item.className = 'extension-item' + (ext.enabled ? '' : ' disabled');
                    item.draggable = true;
                    item.dataset.index = i;
                    item.dataset.id = ext.id;

                    item.innerHTML = \`
                        <span class="drag-handle" title="Drag to reorder">&#x2630;</span>
                        <input type="checkbox" class="extension-checkbox" \${ext.enabled ? 'checked' : ''} data-id="\${ext.id}" title="Enable/disable">
                        <div class="extension-info">
                            <div class="extension-name">\${ext.name}</div>
                            <div class="extension-description">\${ext.description}</div>
                        </div>
                        <span class="extension-delete" data-id="\${ext.id}" title="Remove">&times;</span>
                    \`;

                    // Checkbox toggle
                    item.querySelector('.extension-checkbox').onchange = (e) => {
                        e.stopPropagation();
                        vscode.postMessage({ type: 'toggle-extension', extensionId: ext.id });
                    };

                    // Delete button
                    item.querySelector('.extension-delete').onclick = (e) => {
                        e.stopPropagation();
                        vscode.postMessage({ type: 'delete-extension', extensionId: ext.id });
                    };

                    // Drag events
                    item.ondragstart = (e) => {
                        item.classList.add('dragging');
                        e.dataTransfer.effectAllowed = 'move';
                        e.dataTransfer.setData('text/plain', i.toString());
                    };
                    item.ondragend = () => {
                        item.classList.remove('dragging');
                        document.querySelectorAll('.extension-item').forEach(el => el.classList.remove('drag-over'));
                    };
                    item.ondragover = (e) => {
                        e.preventDefault();
                        e.dataTransfer.dropEffect = 'move';
                        const dragging = document.querySelector('.extension-item.dragging');
                        if (dragging !== item) {
                            item.classList.add('drag-over');
                        }
                    };
                    item.ondragleave = () => {
                        item.classList.remove('drag-over');
                    };
                    item.ondrop = (e) => {
                        e.preventDefault();
                        item.classList.remove('drag-over');
                        const fromIndex = parseInt(e.dataTransfer.getData('text/plain'));
                        const toIndex = parseInt(item.dataset.index);
                        if (fromIndex !== toIndex) {
                            // Reorder the array
                            const newOrder = [...currentExtensions];
                            const [moved] = newOrder.splice(fromIndex, 1);
                            newOrder.splice(toIndex, 0, moved);
                            vscode.postMessage({
                                type: 'reorder-extensions',
                                extensions: newOrder.map(e => ({ id: e.id, enabled: e.enabled }))
                            });
                        }
                    };

                    list.appendChild(item);
                }
            }

            // Always show the add extension link - it opens a dialog
            addLink.style.display = 'inline-block';
        }

        // Handle add extension link - opens QuickPick dialog
        document.getElementById('add-extension-link').onclick = (e) => {
            e.preventDefault();
            vscode.postMessage({ type: 'add-extension' });
        };

    </script>
</body>
</html>`;
  }
}
