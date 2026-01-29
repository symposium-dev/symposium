import * as vscode from "vscode";

/**
 * AgentConfiguration - Identifies a unique agent setup
 *
 * Currently just the workspace folder. Symposium's ConfigAgent handles
 * agent selection and mods via its own configuration system.
 */

export class AgentConfiguration {
  constructor(public readonly workspaceFolder: vscode.WorkspaceFolder) {}

  /**
   * Get a unique key for this configuration.
   * Just the workspace path now.
   */
  key(): string {
    return this.workspaceFolder.uri.fsPath;
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
    return this.workspaceFolder.name;
  }

  /**
   * Create an AgentConfiguration from current VSCode settings
   * @param workspaceFolder - Optional workspace folder. If not provided, will use the first workspace folder or prompt user if multiple exist.
   */
  static async fromSettings(
    workspaceFolder?: vscode.WorkspaceFolder,
  ): Promise<AgentConfiguration> {
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

    return new AgentConfiguration(folder);
  }
}
