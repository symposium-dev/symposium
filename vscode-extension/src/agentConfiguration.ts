import * as vscode from "vscode";

/**
 * AgentConfiguration - Identifies a unique agent setup
 *
 * Consists of the base agent name and workspace folder.
 * Tabs with the same configuration can share an ACP agent process.
 */

export class AgentConfiguration {
  constructor(
    public readonly agentName: string,
    public readonly workspaceFolder: vscode.WorkspaceFolder,
    public readonly enableSparkle: boolean = true,
    public readonly enableCrateResearcher: boolean = true,
  ) {}

  /**
   * Get a unique key for this configuration
   */
  key(): string {
    const components = [
      this.enableSparkle ? "sparkle" : "",
      this.enableCrateResearcher ? "crate-researcher" : "",
    ]
      .filter((c) => c)
      .join("+");
    return `${this.agentName}:${this.workspaceFolder.uri.fsPath}:${components}`;
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
    const components = [
      this.enableSparkle ? "Sparkle" : null,
      this.enableCrateResearcher ? "Rust Crate Researcher" : null,
    ].filter((c) => c !== null);

    if (components.length === 0) {
      return this.agentName;
    }
    return `${this.agentName} + ${components.join(" + ")}`;
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
    const currentAgentName = config.get<string>("currentAgent", "Claude Code");

    // Get component settings
    const enableSparkle = config.get<boolean>("enableSparkle", true);
    const enableCrateResearcher = config.get<boolean>(
      "enableCrateResearcher",
      true,
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

    return new AgentConfiguration(
      currentAgentName,
      folder,
      enableSparkle,
      enableCrateResearcher,
    );
  }
}
