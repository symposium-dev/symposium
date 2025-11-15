import * as vscode from "vscode";

/**
 * AgentConfiguration - Identifies a unique agent setup
 *
 * Consists of the base agent name, enabled component names, and workspace folder.
 * Tabs with the same configuration can share an ACP agent process.
 */

export class AgentConfiguration {
  constructor(
    public readonly agentName: string,
    public readonly components: string[],
    public readonly workspaceFolder: vscode.WorkspaceFolder,
  ) {
    // Sort components for consistent comparison
    this.components = [...components].sort();
  }

  /**
   * Get a unique key for this configuration
   */
  key(): string {
    return `${this.agentName}:${this.components.join(",")}:${this.workspaceFolder.uri.fsPath}`;
  }

  /**
   * Check if two configurations are equivalent
   */
  equals(other: AgentConfiguration): boolean {
    return this.key() === other.key();
  }

  /**
   * Get a human-readable description
   */
  describe(): string {
    if (this.components.length === 0) {
      return this.agentName;
    }
    return `${this.agentName} + ${this.components.length} component${this.components.length > 1 ? "s" : ""}`;
  }

  /**
   * Create an AgentConfiguration from current VSCode settings
   * @param workspaceFolder - Optional workspace folder. If not provided, will use the first workspace folder or prompt user if multiple exist.
   */
  static async fromSettings(
    workspaceFolder?: vscode.WorkspaceFolder,
  ): Promise<AgentConfiguration> {
    const config = vscode.workspace.getConfiguration("symposium");

    // Get current agent
    const currentAgentName = config.get<string>("currentAgent", "ElizACP");

    // Get enabled components
    const components = config.get<
      Record<string, { command: string; args?: string[]; disabled?: boolean }>
    >("components", {});

    const enabledComponents = Object.keys(components).filter(
      (name) => !components[name].disabled,
    );

    // Determine workspace folder
    let folder = workspaceFolder;
    if (!folder) {
      const folders = vscode.workspace.workspaceFolders;
      if (!folders || folders.length === 0) {
        throw new Error("No workspace folder open");
      } else if (folders.length === 1) {
        folder = folders[0];
      } else {
        // Multiple folders - ask user to choose
        const chosen = await vscode.window.showWorkspaceFolderPick({
          placeHolder: "Select workspace folder for agent",
        });
        if (!chosen) {
          throw new Error("No workspace folder selected");
        }
        folder = chosen;
      }
    }

    return new AgentConfiguration(currentAgentName, enabledComponents, folder);
  }
}
