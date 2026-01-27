import * as vscode from "vscode";
import * as acp from "@agentclientprotocol/sdk";
import { AcpAgentActor, ToolCallInfo, SlashCommandInfo } from "./acpAgentActor";
import { AgentConfiguration } from "./agentConfiguration";
import { WorkspaceFileIndex } from "./workspaceFileIndex";
import { getConductorCommand } from "./binaryPath";
import { logger } from "./extension";
import { v4 as uuidv4 } from "uuid";

// Display name for the agent - ConfigAgent handles actual agent selection
const AGENT_DISPLAY_NAME = "Symposium";

interface IndexedMessage {
  index: number;
  type: string;
  tabId: string;
  text?: string;
}

export class ChatViewProvider implements vscode.WebviewViewProvider {
  public static readonly viewType = "symposium.chatView";
  #view?: vscode.WebviewView;
  #configToActor: Map<string, AcpAgentActor> = new Map(); // config.key() → AcpAgentActor
  #tabToConfig: Map<string, AgentConfiguration> = new Map(); // tabId → AgentConfiguration
  #tabToAgentSession: Map<string, string> = new Map(); // tabId → agentSessionId
  #agentSessionToTab: Map<string, string> = new Map(); // agentSessionId → tabId
  #messageQueues: Map<string, IndexedMessage[]> = new Map(); // tabId → queue of unacked messages
  #nextMessageIndex: Map<string, number> = new Map(); // tabId → next index to assign
  #extensionUri: vscode.Uri;
  #extensionActivationId: string;
  #pendingApprovals: Map<
    string,
    {
      resolve: (response: any) => void;
      reject: (error: Error) => void;
      agentName: string;
      tabId: string;
    }
  > = new Map(); // approvalId → promise resolvers

  // Queue for notifications that arrive before session mapping is established
  // agentSessionId → array of pending messages to send to webview
  #pendingSessionNotifications: Map<string, any[]> = new Map();

  // File index per workspace folder
  #fileIndexes: Map<string, WorkspaceFileIndex> = new Map();
  // Track which tabs are subscribed to which file index
  #tabToFileIndex: Map<string, WorkspaceFileIndex> = new Map();

  // Current editor selection state
  #currentSelection: {
    filePath: string;
    relativePath: string;
    startLine: number;
    endLine: number;
    text: string;
  } | null = null;
  #selectionDisposable: vscode.Disposable | null = null;

  // Pending requests for selected tab ID
  #selectedTabRequests: Map<
    string,
    { resolve: (tabId: string | undefined) => void }
  > = new Map();

  // Track tabs with active prompts in progress (for cancellation)
  #tabsWithActivePrompt: Set<string> = new Set();

  #extensionContext: vscode.ExtensionContext;

  constructor(
    extensionUri: vscode.Uri,
    context: vscode.ExtensionContext,
    extensionActivationId: string,
  ) {
    this.#extensionUri = extensionUri;
    this.#extensionContext = context;
    this.#extensionActivationId = extensionActivationId;
  }

  /**
   * Get or create an ACP actor for the given configuration.
   * Actors are shared across tabs with the same configuration.
   */
  async #getOrCreateActor(config: AgentConfiguration): Promise<AcpAgentActor> {
    const key = config.key();

    // Return existing actor if we have one for this config
    const existing = this.#configToActor.get(key);
    if (existing) {
      logger.debug("agent", "Reusing existing agent actor", {
        configKey: key,
      });
      return existing;
    }

    logger.important("agent", "Spawning new agent actor", {
      configKey: key,
    });

    // Create a new actor with callbacks
    const actor = new AcpAgentActor({
      onAgentText: (agentSessionId, text) => {
        const receiveTime = Date.now();
        const tabId = this.#agentSessionToTab.get(agentSessionId);
        if (tabId) {
          // Capture for testing if enabled
          if (this.#testResponseCapture.has(tabId)) {
            this.#testResponseCapture.get(tabId)!.push(text);
          }

          logger.debug("perf", "Received chunk from agent", {
            receiveTime,
            textLength: text.length,
          });
          this.#sendToWebview({
            type: "agent-text",
            tabId,
            text,
            timestamp: receiveTime,
          });
          const sendTime = Date.now();
          logger.debug("perf", "Sent chunk to webview", {
            sendTime,
            delay: sendTime - receiveTime,
          });
        }
      },
      onUserText: (agentSessionId, text) => {
        const receiveTime = Date.now();
        const tabId = this.#agentSessionToTab.get(agentSessionId);
        if (tabId) {
          // Capture for testing if enabled
          if (this.#testResponseCapture.has(tabId)) {
            this.#testResponseCapture.get(tabId)!.push(text);
          }

          logger.debug("perf", "Received chunk from user", {
            receiveTime,
            textLength: text.length,
          });
          this.#sendToWebview({
            type: "user-text",
            tabId,
            text,
            timestamp: receiveTime,
          });
          const sendTime = Date.now();
          logger.debug("perf", "Sent chunk to webview", {
            sendTime,
            delay: sendTime - receiveTime,
          });
        }
      },
      onAgentComplete: (agentSessionId) => {
        const tabId = this.#agentSessionToTab.get(agentSessionId);
        if (tabId) {
          // Clear the active prompt flag now that the turn is complete
          this.#tabsWithActivePrompt.delete(tabId);

          this.#sendToWebview({
            type: "agent-complete",
            tabId,
          });
        }
      },
      onRequestPermission: async (
        params: acp.RequestPermissionRequest,
      ): Promise<acp.RequestPermissionResponse> => {
        return this.#requestUserApprovalForSession(params, AGENT_DISPLAY_NAME);
      },
      onToolCall: (agentSessionId: string, toolCall: ToolCallInfo) => {
        const tabId = this.#agentSessionToTab.get(agentSessionId);
        if (tabId) {
          this.#sendToWebview({
            type: "tool-call",
            tabId,
            toolCall,
          });
        }
      },
      onToolCallUpdate: (agentSessionId: string, toolCall: ToolCallInfo) => {
        const tabId = this.#agentSessionToTab.get(agentSessionId);
        if (tabId) {
          this.#sendToWebview({
            type: "tool-call-update",
            tabId,
            toolCall,
          });
        }
      },
      onAvailableCommands: (
        agentSessionId: string,
        commands: SlashCommandInfo[],
      ) => {
        const message = {
          type: "available-commands",
          commands,
        };
        this.#sendSessionNotification(agentSessionId, message);
      },
    });

    // Initialize the actor with the conductor command (bundled or from PATH)
    const conductorCommand = getConductorCommand(this.#extensionContext);
    await actor.initialize(config, conductorCommand);

    // Store it in the map
    this.#configToActor.set(key, actor);

    return actor;
  }

  /**
   * Get or create a file index for the given workspace folder.
   * File indexes are shared across tabs in the same workspace.
   */
  /**
   * Set up tracking for editor selection changes.
   * Broadcasts selection updates to all tabs when selection changes.
   */
  #setupSelectionTracking(): void {
    // Clean up any existing listener
    this.#selectionDisposable?.dispose();

    // Track selection changes
    this.#selectionDisposable = vscode.window.onDidChangeTextEditorSelection(
      (event) => {
        const editor = event.textEditor;
        const selection = event.selections[0]; // Use primary selection

        // Check if there's a non-empty selection
        if (selection.isEmpty) {
          if (this.#currentSelection !== null) {
            this.#currentSelection = null;
            this.#broadcastSelectionUpdate();
          }
          return;
        }

        // Get the selected text
        const text = editor.document.getText(selection);
        if (text.length === 0) {
          if (this.#currentSelection !== null) {
            this.#currentSelection = null;
            this.#broadcastSelectionUpdate();
          }
          return;
        }

        // Get file path info
        const filePath = editor.document.uri.fsPath;
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

        // Update current selection (1-indexed lines for display)
        this.#currentSelection = {
          filePath,
          relativePath,
          startLine: selection.start.line + 1,
          endLine: selection.end.line + 1,
          text,
        };

        this.#broadcastSelectionUpdate();
      },
    );

    // Also check current selection immediately
    const editor = vscode.window.activeTextEditor;
    if (editor && !editor.selection.isEmpty) {
      const selection = editor.selection;
      const text = editor.document.getText(selection);
      if (text.length > 0) {
        const filePath = editor.document.uri.fsPath;
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

        this.#currentSelection = {
          filePath,
          relativePath,
          startLine: selection.start.line + 1,
          endLine: selection.end.line + 1,
          text,
        };
      }
    }
  }

  /**
   * Broadcast selection update to all tabs.
   */
  #broadcastSelectionUpdate(): void {
    // Send to all tabs that have file indexes
    for (const [tabId, index] of this.#tabToFileIndex.entries()) {
      this.#sendFileListToTab(tabId, index);
    }
  }

  async #getOrCreateFileIndex(
    workspaceFolder: vscode.WorkspaceFolder,
    tabId: string,
  ): Promise<WorkspaceFileIndex> {
    const key = workspaceFolder.uri.fsPath;

    let index = this.#fileIndexes.get(key);
    if (!index) {
      index = new WorkspaceFileIndex(workspaceFolder);
      await index.initialize();

      // Subscribe to changes and broadcast to all tabs using this index
      index.onDidChange(() => {
        this.#broadcastFileList(index!);
      });

      this.#fileIndexes.set(key, index);
    }

    // Track this tab's subscription
    this.#tabToFileIndex.set(tabId, index);

    return index;
  }

  /**
   * Read file content for embedding in a prompt.
   * Returns null if file cannot be read.
   */
  async #readFileContent(
    filePath: string,
    workspaceFolder: vscode.WorkspaceFolder,
  ): Promise<{ absolutePath: string; text: string; mimeType: string } | null> {
    // Resolve the path - could be relative to workspace or absolute
    let absolutePath: string;
    if (filePath.startsWith("/")) {
      absolutePath = filePath;
    } else {
      absolutePath = vscode.Uri.joinPath(workspaceFolder.uri, filePath).fsPath;
    }

    try {
      const uri = vscode.Uri.file(absolutePath);
      const content = await vscode.workspace.fs.readFile(uri);
      const text = new TextDecoder().decode(content);

      // Determine MIME type from extension
      const ext = filePath.split(".").pop()?.toLowerCase() || "";
      const mimeType = this.#getMimeType(ext);

      return { absolutePath, text, mimeType };
    } catch (err) {
      logger.error("context", "Failed to read file", {
        path: absolutePath,
        error: err,
      });
      return null;
    }
  }

  /**
   * Read symbol content for embedding in a prompt.
   * Extracts the relevant lines from the file based on the symbol's range.
   */
  async #readSymbolContent(
    filePath: string,
    range: {
      startLine: number;
      startChar: number;
      endLine: number;
      endChar: number;
    },
    symbolName: string,
    workspaceFolder: vscode.WorkspaceFolder,
  ): Promise<{
    absolutePath: string;
    text: string;
    mimeType: string;
    startLine: number;
    endLine: number;
  } | null> {
    // Resolve the path
    let absolutePath: string;
    if (filePath.startsWith("/")) {
      absolutePath = filePath;
    } else {
      absolutePath = vscode.Uri.joinPath(workspaceFolder.uri, filePath).fsPath;
    }

    try {
      const uri = vscode.Uri.file(absolutePath);
      const content = await vscode.workspace.fs.readFile(uri);
      const fullText = new TextDecoder().decode(content);
      const lines = fullText.split("\n");

      // Use the exact range from the LSP - no heuristic expansion
      // LSP lines are 0-indexed
      const startLine = range.startLine;
      const endLine = range.endLine;

      // Extract the relevant lines
      const extractedLines = lines.slice(startLine, endLine + 1);
      const text = extractedLines.join("\n");

      // Determine MIME type
      const ext = filePath.split(".").pop()?.toLowerCase() || "";
      const mimeType = this.#getMimeType(ext);

      return {
        absolutePath,
        text,
        mimeType,
        startLine: startLine + 1, // 1-indexed for display
        endLine: endLine + 1,
      };
    } catch (err) {
      logger.error("context", "Failed to read symbol content", {
        path: absolutePath,
        symbol: symbolName,
        error: err,
      });
      return null;
    }
  }

  /**
   * Get MIME type for a file extension.
   */
  #getMimeType(ext: string): string {
    const mimeTypes: Record<string, string> = {
      ts: "text/typescript",
      tsx: "text/typescript",
      js: "text/javascript",
      jsx: "text/javascript",
      rs: "text/rust",
      py: "text/python",
      rb: "text/ruby",
      go: "text/go",
      java: "text/java",
      c: "text/c",
      cpp: "text/cpp",
      h: "text/c",
      hpp: "text/cpp",
      md: "text/markdown",
      json: "application/json",
      yaml: "text/yaml",
      yml: "text/yaml",
      toml: "text/toml",
      xml: "text/xml",
      html: "text/html",
      css: "text/css",
      sql: "text/sql",
      sh: "text/x-shellscript",
      bash: "text/x-shellscript",
      zsh: "text/x-shellscript",
    };
    return mimeTypes[ext] || "text/plain";
  }

  /**
   * Send the current file list to a specific tab.
   */
  #sendFileListToTab(tabId: string, index: WorkspaceFileIndex): void {
    const files = index.getFiles();
    const symbols = index.getSymbols();
    this.#sendToWebview({
      type: "available-context",
      tabId,
      files,
      symbols,
      selection: this.#currentSelection,
    });
    logger.debug("context", "Sent context to tab", {
      tabId,
      fileCount: files.length,
      symbolCount: symbols.length,
      hasSelection: this.#currentSelection !== null,
    });
  }

  /**
   * Broadcast file list updates to all tabs subscribed to an index.
   */
  #broadcastFileList(index: WorkspaceFileIndex): void {
    for (const [tabId, tabIndex] of this.#tabToFileIndex.entries()) {
      if (tabIndex === index) {
        this.#sendFileListToTab(tabId, index);
      }
    }
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

    logger.debug("webview", "Webview resolved and created");

    // Set up selection tracking
    this.#setupSelectionTracking();

    // Handle webview visibility changes
    webviewView.onDidChangeVisibility(() => {
      if (webviewView.visible) {
        logger.debug("webview", "Webview became visible");
        this.#onWebviewVisible();
      } else {
        logger.debug("webview", "Webview became hidden");
        this.#onWebviewHidden();
      }
    });

    // Handle messages from the webview
    webviewView.webview.onDidReceiveMessage(async (message) => {
      switch (message.type) {
        case "new-tab":
          try {
            // Get the current agent configuration from settings
            const config = await AgentConfiguration.fromSettings();

            // Store the configuration for this tab
            this.#tabToConfig.set(message.tabId, config);

            // Initialize message tracking for this tab
            this.#messageQueues.set(message.tabId, []);
            this.#nextMessageIndex.set(message.tabId, 0);

            // Update tab title immediately (before spawning agent)
            this.#sendToWebview({
              type: "set-tab-title",
              tabId: message.tabId,
              title: AGENT_DISPLAY_NAME,
            });

            // Get or create an actor for this configuration (may spawn process)
            const actor = await this.#getOrCreateActor(config);

            logger.important("agent", "Spawned actor", {
              actor,
              config: config.describe(),
            });

            // Create a new agent session for this tab
            const agentSessionId = await actor.createSession(
              config.workspaceFolder.uri.fsPath,
            );
            this.#tabToAgentSession.set(message.tabId, agentSessionId);
            this.#agentSessionToTab.set(agentSessionId, message.tabId);

            // Replay any notifications that arrived before mapping was established
            this.#replayPendingSessionNotifications(
              agentSessionId,
              message.tabId,
            );

            // Set up file index and send initial file list
            const fileIndex = await this.#getOrCreateFileIndex(
              config.workspaceFolder,
              message.tabId,
            );
            this.#sendFileListToTab(message.tabId, fileIndex);

            logger.important("agent", "Created agent session", {
              agentSessionId,
              tabId: message.tabId,
              config: config.describe(),
            });
          } catch (err) {
            logger.error("agent", "Failed to create agent session", {
              error: err,
            });
          }
          break;

        case "message-ack":
          // Webview acknowledged a message - remove from queue
          this.#handleMessageAck(message.tabId, message.index);
          break;

        case "prompt":
          logger.debug("agent", "Received prompt", {
            tabId: message.tabId,
            contextFiles: message.contextFiles,
          });

          // Get the agent session for this tab
          const agentSessionId = this.#tabToAgentSession.get(message.tabId);
          if (!agentSessionId) {
            logger.error("agent", "No agent session found for tab", {
              tabId: message.tabId,
            });
            return;
          }

          // Get the configuration and actor for this tab
          const tabConfig = this.#tabToConfig.get(message.tabId);
          if (!tabConfig) {
            logger.error("agent", "No configuration found for tab", {
              tabId: message.tabId,
            });
            return;
          }

          const tabActor = this.#configToActor.get(tabConfig.key());
          if (!tabActor) {
            logger.error("agent", "No actor found for configuration", {
              configKey: tabConfig.key(),
            });
            return;
          }

          // Cancel any in-progress prompt before sending a new one
          if (this.#tabsWithActivePrompt.has(message.tabId)) {
            logger.debug("agent", "Cancelling previous prompt before new one", {
              tabId: message.tabId,
              agentSessionId,
            });
            try {
              await tabActor.cancelSession(agentSessionId);
            } catch (err) {
              logger.error("agent", "Failed to cancel previous prompt", {
                error: err,
              });
              // Continue anyway - the new prompt should still work
            }
          }

          // Build content blocks for the prompt
          const contentBlocks: acp.ContentBlock[] = [];

          // Add text prompt
          if (message.prompt) {
            contentBlocks.push({
              type: "text",
              text: message.prompt,
            });
          }

          // Add file context as EmbeddedResource blocks
          if (message.contextFiles && Array.isArray(message.contextFiles)) {
            for (const filePath of message.contextFiles) {
              try {
                const content = await this.#readFileContent(
                  filePath,
                  tabConfig.workspaceFolder,
                );
                if (content !== null) {
                  contentBlocks.push({
                    type: "resource",
                    resource: {
                      uri: `file://${content.absolutePath}`,
                      text: content.text,
                      mimeType: content.mimeType,
                    },
                  });
                  logger.debug("context", "Added file context", {
                    path: filePath,
                    size: content.text.length,
                  });
                }
              } catch (err) {
                logger.error("context", "Failed to read context file", {
                  path: filePath,
                  error: err,
                });
              }
            }
          }

          // Add symbol context as EmbeddedResource blocks
          if (message.contextSymbols && Array.isArray(message.contextSymbols)) {
            for (const sym of message.contextSymbols) {
              try {
                const content = await this.#readSymbolContent(
                  sym.location,
                  sym.range,
                  sym.name,
                  tabConfig.workspaceFolder,
                );
                if (content !== null) {
                  contentBlocks.push({
                    type: "resource",
                    resource: {
                      uri: `file://${content.absolutePath}#L${content.startLine}-L${content.endLine}`,
                      text: content.text,
                      mimeType: content.mimeType,
                    },
                  });
                  logger.debug("context", "Added symbol context", {
                    name: sym.name,
                    path: sym.location,
                    lines: `${content.startLine}-${content.endLine}`,
                    size: content.text.length,
                  });
                }
              } catch (err) {
                logger.error("context", "Failed to read symbol context", {
                  name: sym.name,
                  path: sym.location,
                  error: err,
                });
              }
            }
          }

          // Add selection context as EmbeddedResource blocks
          if (
            message.contextSelections &&
            Array.isArray(message.contextSelections)
          ) {
            for (const sel of message.contextSelections) {
              // Selection already contains the text, no need to read file
              const ext =
                sel.relativePath.split(".").pop()?.toLowerCase() || "";
              const mimeType = this.#getMimeType(ext);

              contentBlocks.push({
                type: "resource",
                resource: {
                  uri: `file://${sel.filePath}#L${sel.startLine}-L${sel.endLine}`,
                  text: sel.text,
                  mimeType,
                },
              });
              logger.debug("context", "Added selection context", {
                path: sel.relativePath,
                lines: `${sel.startLine}-${sel.endLine}`,
                size: sel.text.length,
              });
            }
          }

          logger.debug("agent", "Sending prompt to agent", {
            agentSessionId,
            contentBlockCount: contentBlocks.length,
          });

          // Mark this tab as having an active prompt
          this.#tabsWithActivePrompt.add(message.tabId);

          // Send prompt to agent (responses come via callbacks)
          try {
            await tabActor.sendPrompt(agentSessionId, contentBlocks);
          } catch (err: any) {
            // Clear active prompt flag on error
            this.#tabsWithActivePrompt.delete(message.tabId);
            logger.error("agent", "Failed to send prompt", { error: err });
            this.#sendToWebview({
              type: "agent-error",
              tabId: message.tabId,
              error: err?.message || String(err),
            });
          }
          break;

        case "webview-ready":
          // Webview is initialized and ready to receive messages
          logger.debug("webview", "Webview ready - replaying queued messages");
          this.#replayQueuedMessages();
          break;

        case "log":
          // Webview sending a log message
          logger.debug("webview", message.message, message.data);
          break;

        case "selected-tab-response":
          // Response to a get-selected-tab request
          const tabRequest = this.#selectedTabRequests.get(message.requestId);
          if (tabRequest) {
            this.#selectedTabRequests.delete(message.requestId);
            tabRequest.resolve(message.tabId);
          }
          break;

        case "approval-response":
          // User responded to approval request
          const pending = this.#pendingApprovals.get(message.approvalId);
          if (pending) {
            this.#pendingApprovals.delete(message.approvalId);

            // Handle "bypass all" option - add workspace to bypass list
            if (message.bypassAll) {
              const tabConfig = this.#tabToConfig.get(pending.tabId);
              if (tabConfig) {
                await this.#addToBypassList(
                  tabConfig.workspaceFolder.uri.fsPath,
                );
              }
            }

            // Resolve the promise with the response
            pending.resolve(message.response);
          } else {
            logger.error("approval", "No pending approval found", {
              approvalId: message.approvalId,
            });
          }
          break;
      }
    });
  }

  #handleMessageAck(tabId: string, ackedIndex: number) {
    const queue = this.#messageQueues.get(tabId);
    if (!queue) {
      return;
    }

    // Remove all messages with index <= ackedIndex
    const remaining = queue.filter((msg) => msg.index > ackedIndex);
    this.#messageQueues.set(tabId, remaining);

    logger.debug("webview", "Message acknowledged", {
      tabId,
      ackedIndex,
      remainingInQueue: remaining.length,
    });
  }

  /**
   * Check if permissions are bypassed for a workspace path.
   */
  #isPermissionBypassed(workspacePath: string): boolean {
    const vsConfig = vscode.workspace.getConfiguration("symposium");
    const bypassList = vsConfig.get<string[]>("bypassPermissions", []);
    return bypassList.includes(workspacePath);
  }

  /**
   * Add a workspace path to the bypass permissions list.
   */
  async #addToBypassList(workspacePath: string): Promise<void> {
    const vsConfig = vscode.workspace.getConfiguration("symposium");
    const bypassList = vsConfig.get<string[]>("bypassPermissions", []);

    if (!bypassList.includes(workspacePath)) {
      await vsConfig.update(
        "bypassPermissions",
        [...bypassList, workspacePath],
        vscode.ConfigurationTarget.Global,
      );
      logger.debug("approval", "Added workspace to bypass list", {
        workspacePath,
      });
    }
  }

  /**
   * Request user approval for a permission request, looking up the tab from session ID.
   * Handles bypass permissions and auto-approval.
   */
  async #requestUserApprovalForSession(
    params: acp.RequestPermissionRequest,
    agentName: string,
  ): Promise<acp.RequestPermissionResponse> {
    // Look up tab from session ID
    const tabId = this.#agentSessionToTab.get(params.sessionId);
    if (!tabId) {
      logger.error("approval", "No tab found for session", {
        sessionId: params.sessionId,
      });
      const rejectOption = params.options.find(
        (opt) => opt.kind === "reject_once",
      );
      if (rejectOption) {
        return {
          outcome: { outcome: "selected", optionId: rejectOption.optionId },
        };
      }
      return { outcome: { outcome: "cancelled" } };
    }

    // Get the workspace path for this tab
    const tabConfig = this.#tabToConfig.get(tabId);
    const workspacePath = tabConfig?.workspaceFolder.uri.fsPath;

    // Check if bypass permissions is enabled for this workspace
    if (workspacePath && this.#isPermissionBypassed(workspacePath)) {
      // Auto-approve - find the "allow_once" option
      const allowOption = params.options.find(
        (opt) => opt.kind === "allow_once",
      );
      if (allowOption) {
        logger.debug("approval", "Auto-approved (bypass permissions enabled)", {
          agent: agentName,
          tool: params.toolCall.title,
          workspacePath,
        });
        return {
          outcome: { outcome: "selected", optionId: allowOption.optionId },
        };
      }
    }

    // Need user approval
    return this.#requestUserApprovalForTab(params, tabId, agentName);
  }

  async #requestUserApprovalForTab(
    params: acp.RequestPermissionRequest,
    tabId: string,
    agentName: string,
  ): Promise<acp.RequestPermissionResponse> {
    // Generate unique approval ID
    const approvalId = uuidv4();

    logger.debug("approval", "Requesting user approval", {
      approvalId,
      tabId,
      agent: agentName,
      toolCall: params.toolCall,
    });

    // Create a promise that will be resolved when user responds
    const approvalPromise = new Promise<acp.RequestPermissionResponse>(
      (resolve, reject) => {
        this.#pendingApprovals.set(approvalId, {
          resolve,
          reject,
          agentName,
          tabId,
        });
      },
    );

    // Send approval request to webview
    this.#sendToWebview({
      type: "approval-request",
      tabId,
      approvalId,
      agentName,
      toolCall: params.toolCall,
      options: params.options,
    });

    // Wait for user response
    return approvalPromise;
  }

  #replayQueuedMessages() {
    if (!this.#view) {
      return;
    }

    // Replay all queued messages for all tabs
    for (const [tabId, queue] of this.#messageQueues.entries()) {
      for (const message of queue) {
        logger.debug("webview", "Replaying queued message", {
          tabId,
          messageIndex: message.index,
        });
        this.#view.webview.postMessage(message);
      }
    }
  }

  #sendToWebview(message: any) {
    if (!this.#view) {
      return;
    }

    const tabId = message.tabId;
    if (!tabId) {
      logger.error("webview", "Message missing tabId", { message });
      return;
    }

    // Assign index to message
    const index = this.#nextMessageIndex.get(tabId) ?? 0;
    this.#nextMessageIndex.set(tabId, index + 1);

    const indexedMessage: IndexedMessage = {
      index,
      ...message,
    };

    // Add to queue (unacked messages)
    const queue = this.#messageQueues.get(tabId) ?? [];
    queue.push(indexedMessage);
    this.#messageQueues.set(tabId, queue);

    // Send if webview is visible
    if (this.#view.visible) {
      logger.debug("webview", "Sending message to webview", {
        tabId,
        messageIndex: index,
      });
      this.#view.webview.postMessage(indexedMessage);
    } else {
      logger.debug("webview", "Queued message (webview hidden)", {
        tabId,
        messageIndex: index,
      });
    }
  }

  /**
   * Send a session notification to the webview, or queue it if the session
   * mapping isn't established yet.
   *
   * @param agentSessionId - The agent session ID
   * @param message - The message to send (without tabId - will be added)
   */
  #sendSessionNotification(agentSessionId: string, message: any) {
    const tabId = this.#agentSessionToTab.get(agentSessionId);

    if (tabId) {
      // Session mapping exists, send directly
      this.#sendToWebview({ ...message, tabId });
    } else {
      // Queue until session mapping is established
      logger.debug("agent", "Queuing notification (session not yet mapped)", {
        agentSessionId,
        messageType: message.type,
      });
      const queue = this.#pendingSessionNotifications.get(agentSessionId) ?? [];
      queue.push(message);
      this.#pendingSessionNotifications.set(agentSessionId, queue);
    }
  }

  /**
   * Replay any queued notifications for a session after the mapping is established.
   */
  #replayPendingSessionNotifications(agentSessionId: string, tabId: string) {
    const queue = this.#pendingSessionNotifications.get(agentSessionId);
    if (queue && queue.length > 0) {
      logger.debug("agent", "Replaying queued notifications", {
        agentSessionId,
        tabId,
        count: queue.length,
      });
      for (const message of queue) {
        this.#sendToWebview({ ...message, tabId });
      }
      this.#pendingSessionNotifications.delete(agentSessionId);
    }
  }

  #onWebviewVisible() {
    // Visibility change detected - webview will send "webview-ready" when initialized
    logger.debug("webview", "Webview became visible");
  }

  #onWebviewHidden() {
    // Nothing to do - messages stay queued until acked
    logger.debug("webview", "Webview became hidden");
  }

  #getHtmlForWebview(webview: vscode.Webview) {
    const scriptUri = webview.asWebviewUri(
      vscode.Uri.joinPath(this.#extensionUri, "out", "webview.js"),
    );

    // Get the URI for the no-tabs background image (vase with Ferrises)
    const noTabsImageUri = webview.asWebviewUri(
      vscode.Uri.joinPath(this.#extensionUri, "resources", "no-tabs-image.png"),
    );

    // Get the requireModifierToSend setting
    const config = vscode.workspace.getConfiguration("symposium");
    const requireModifierToSend = config.get<boolean>(
      "requireModifierToSend",
      false,
    );

    return `<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Symposium Chat</title>
    <style>
        body {
            margin: 0;
            padding: 0;
            overflow: hidden;
        }
        #mynah-root {
            width: 100%;
            height: 100vh;
        }
    </style>
</head>
<body>
    <div id="mynah-root"></div>
    <script>
        // Embed extension activation ID so it's available immediately
        window.SYMPOSIUM_EXTENSION_ACTIVATION_ID = "${this.#extensionActivationId}";
        // Embed settings for MynahUI config
        window.SYMPOSIUM_REQUIRE_MODIFIER_TO_SEND = ${requireModifierToSend};
        // Image URI for the no-tabs screen
        window.SYMPOSIUM_NO_TABS_IMAGE_URI = "${noTabsImageUri}";
    </script>
    <script src="${scriptUri}"></script>
</body>
</html>`;
  }

  /**
   * Request the currently selected tab ID from the webview.
   * Returns undefined if no tab is selected.
   */
  async #getSelectedTabId(): Promise<string | undefined> {
    if (!this.#view) {
      return undefined;
    }

    const requestId = `req-${Date.now()}-${Math.random()}`;

    return new Promise((resolve) => {
      this.#selectedTabRequests.set(requestId, { resolve });

      this.#view!.webview.postMessage({
        type: "get-selected-tab",
        requestId,
      });

      // Timeout after 1 second
      setTimeout(() => {
        if (this.#selectedTabRequests.has(requestId)) {
          this.#selectedTabRequests.delete(requestId);
          resolve(undefined);
        }
      }, 1000);
    });
  }

  /**
   * Add a frozen selection as context to the active tab's prompt input.
   * Called when user triggers "Discuss in Symposium" code action.
   * Creates a new tab if none exists.
   */
  public async addSelectionToPrompt(selection: {
    filePath: string;
    relativePath: string;
    startLine: number;
    endLine: number;
    text: string;
  }): Promise<void> {
    // Ask webview for the currently selected tab
    let tabId = await this.#getSelectedTabId();

    // If no tab is selected, tell webview to create one and use that
    if (!tabId) {
      logger.debug("context", "No tab selected, requesting new tab creation");
      tabId = await this.#requestNewTab();
      if (!tabId) {
        logger.error("context", "Failed to create new tab");
        return;
      }
    }

    // Send message to webview to add this as a custom context item
    this.#sendToWebview({
      type: "add-context-to-prompt",
      tabId,
      selection,
    });

    logger.debug("context", "Added frozen selection to prompt", {
      tabId,
      path: selection.relativePath,
      lines: `${selection.startLine}-${selection.endLine}`,
    });
  }

  /**
   * Request the webview to create a new tab and return its ID.
   */
  async #requestNewTab(): Promise<string | undefined> {
    if (!this.#view) {
      return undefined;
    }

    const requestId = `req-${Date.now()}-${Math.random()}`;

    return new Promise((resolve) => {
      this.#selectedTabRequests.set(requestId, { resolve });

      this.#view!.webview.postMessage({
        type: "create-tab",
        requestId,
      });

      // Timeout after 5 seconds (tab creation might take a moment)
      setTimeout(() => {
        if (this.#selectedTabRequests.has(requestId)) {
          this.#selectedTabRequests.delete(requestId);
          resolve(undefined);
        }
      }, 5000);
    });
  }

  dispose() {
    // Dispose all actors
    for (const actor of this.#configToActor.values()) {
      actor.dispose();
    }
    this.#configToActor.clear();

    // Dispose all file indexes
    for (const index of this.#fileIndexes.values()) {
      index.dispose();
    }
    this.#fileIndexes.clear();
    this.#tabToFileIndex.clear();
  }

  // Testing API - only use from integration tests
  public async simulateWebviewMessage(message: any): Promise<void> {
    // Simulate a message from the webview
    // This allows tests to trigger the same code paths as real webview interactions
    const handler = this.#view?.webview.onDidReceiveMessage;
    if (!handler) {
      throw new Error("Webview not initialized - call focus command first");
    }

    // Manually trigger the message handler
    // We need to access the internal message handler, which we set up in resolveWebviewView
    // For now, we'll use a workaround: post the message directly
    // This requires the view to be resolved first
    if (!this.#view) {
      throw new Error("View not resolved");
    }

    // Simulate the message by calling the handler we registered
    // Actually, we can't access the handler directly, so let's expose the logic instead
    await this.#handleWebviewMessage(message);
  }

  public getTabsForTesting(): string[] {
    return Array.from(this.#tabToAgentSession.keys());
  }

  // Store agent responses for testing
  #testResponseCapture: Map<string, string[]> = new Map();

  public startCapturingResponses(tabId: string): void {
    this.#testResponseCapture.set(tabId, []);
  }

  public getResponse(tabId: string): string {
    const chunks = this.#testResponseCapture.get(tabId) || [];
    return chunks.join("");
  }

  public stopCapturingResponses(tabId: string): void {
    this.#testResponseCapture.delete(tabId);
  }

  /**
   * Check if a tab has an active prompt in progress.
   * Used for testing cancellation behavior.
   */
  public hasActivePrompt(tabId: string): boolean {
    return this.#tabsWithActivePrompt.has(tabId);
  }

  async #handleWebviewMessage(message: any): Promise<void> {
    // This is the same logic from resolveWebviewView's onDidReceiveMessage
    // We'll need to refactor to share this code
    switch (message.type) {
      case "new-tab":
        try {
          const config = await AgentConfiguration.fromSettings();
          this.#tabToConfig.set(message.tabId, config);
          this.#messageQueues.set(message.tabId, []);
          this.#nextMessageIndex.set(message.tabId, 0);

          this.#sendToWebview({
            type: "set-tab-title",
            tabId: message.tabId,
            title: AGENT_DISPLAY_NAME,
          });

          const actor = await this.#getOrCreateActor(config);
          const agentSessionId = await actor.createSession(
            config.workspaceFolder.uri.fsPath,
          );
          this.#tabToAgentSession.set(message.tabId, agentSessionId);
          this.#agentSessionToTab.set(agentSessionId, message.tabId);

          // Replay any notifications that arrived before mapping was established
          this.#replayPendingSessionNotifications(
            agentSessionId,
            message.tabId,
          );

          // Set up file index and send initial file list
          const fileIndex = await this.#getOrCreateFileIndex(
            config.workspaceFolder,
            message.tabId,
          );
          this.#sendFileListToTab(message.tabId, fileIndex);

          logger.important("agent", "Agent session created", {
            tabId: message.tabId,
            agentSessionId,
          });
        } catch (err) {
          logger.error("agent", "Failed to create agent session", {
            error: err,
          });
        }
        break;

      case "prompt":
        try {
          logger.debug("agent", "Received prompt", { tabId: message.tabId });

          // Get the agent session for this tab
          const agentSessionId = this.#tabToAgentSession.get(message.tabId);
          if (!agentSessionId) {
            logger.error("agent", "No agent session found for tab", {
              tabId: message.tabId,
            });
            return;
          }

          // Get the configuration and actor for this tab
          const tabConfig = this.#tabToConfig.get(message.tabId);
          if (!tabConfig) {
            logger.error("agent", "No configuration found for tab", {
              tabId: message.tabId,
            });
            return;
          }

          const tabActor = this.#configToActor.get(tabConfig.key());
          if (!tabActor) {
            logger.error("agent", "No actor found for configuration", {
              configKey: tabConfig.key(),
            });
            return;
          }

          // Cancel any in-progress prompt before sending a new one
          if (this.#tabsWithActivePrompt.has(message.tabId)) {
            logger.debug("agent", "Cancelling previous prompt before new one", {
              tabId: message.tabId,
              agentSessionId,
            });
            try {
              await tabActor.cancelSession(agentSessionId);
            } catch (err) {
              logger.error("agent", "Failed to cancel previous prompt", {
                error: err,
              });
              // Continue anyway - the new prompt should still work
            }
          }

          logger.debug("agent", "Sending prompt to agent", {
            tabId: message.tabId,
            agentSessionId,
          });

          // Mark this tab as having an active prompt
          this.#tabsWithActivePrompt.add(message.tabId);

          // Send prompt to agent (responses come via callbacks)
          await tabActor.sendPrompt(agentSessionId, message.prompt);
        } catch (err: any) {
          // Clear active prompt flag on error
          this.#tabsWithActivePrompt.delete(message.tabId);
          logger.error("agent", "Failed to send prompt", { error: err });
          this.#sendToWebview({
            type: "agent-error",
            tabId: message.tabId,
            error: err?.message || String(err),
          });
        }
        break;

      case "approval-response":
        // User responded to approval request
        const pendingApproval = this.#pendingApprovals.get(message.approvalId);
        if (pendingApproval) {
          this.#pendingApprovals.delete(message.approvalId);

          // Handle "bypass all" option - add workspace to bypass list
          if (message.bypassAll) {
            const tabConfig = this.#tabToConfig.get(pendingApproval.tabId);
            if (tabConfig) {
              await this.#addToBypassList(tabConfig.workspaceFolder.uri.fsPath);
            }
          }

          // Resolve the promise with the response
          pendingApproval.resolve(message.response);
        } else {
          logger.error("approval", "No pending approval found", {
            approvalId: message.approvalId,
          });
        }
        break;
    }
  }
}
