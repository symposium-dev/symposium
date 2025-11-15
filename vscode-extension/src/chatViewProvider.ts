import * as vscode from "vscode";
import * as acp from "@agentclientprotocol/sdk";
import { AcpAgentActor } from "./acpAgentActor";
import { AgentConfiguration } from "./agentConfiguration";
import { logger } from "./extension";
import { v4 as uuidv4 } from "uuid";

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
    }
  > = new Map(); // approvalId → promise resolvers

  constructor(
    extensionUri: vscode.Uri,
    context: vscode.ExtensionContext,
    extensionActivationId: string,
  ) {
    this.#extensionUri = extensionUri;
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
      logger.info("agent", "Reusing existing agent actor", {
        configKey: key,
        agentName: config.agentName,
      });
      return existing;
    }

    logger.info("agent", "Spawning new agent actor", {
      configKey: key,
      agentName: config.agentName,
      components: config.components,
    });

    // Create a new actor with callbacks
    const actor = new AcpAgentActor({
      onAgentText: (agentSessionId, text) => {
        const tabId = this.#agentSessionToTab.get(agentSessionId);
        if (tabId) {
          // Capture for testing if enabled
          if (this.#testResponseCapture.has(tabId)) {
            this.#testResponseCapture.get(tabId)!.push(text);
          }

          this.#sendToWebview({
            type: "agent-text",
            tabId,
            text,
          });
        }
      },
      onAgentComplete: (agentSessionId) => {
        const tabId = this.#agentSessionToTab.get(agentSessionId);
        if (tabId) {
          this.#sendToWebview({
            type: "agent-complete",
            tabId,
          });
        }
      },
      onRequestPermission: async (
        params: acp.RequestPermissionRequest,
      ): Promise<acp.RequestPermissionResponse> => {
        // Check if this agent has bypass permissions enabled
        const vsConfig = vscode.workspace.getConfiguration("symposium");
        const agents = vsConfig.get<Record<string, any>>("agents", {});
        const agentConfig = agents[config.agentName];
        const bypassPermissions = agentConfig?.bypassPermissions || false;

        if (bypassPermissions) {
          // Auto-approve - find the "allow_once" option
          const allowOption = params.options.find(
            (opt) => opt.kind === "allow_once",
          );
          if (allowOption) {
            logger.info(
              "approval",
              "Auto-approved (bypass permissions enabled)",
              {
                agent: config.agentName,
                tool: params.toolCall.title,
              },
            );
            return {
              outcome: { outcome: "selected", optionId: allowOption.optionId },
            };
          }
        }

        // Need user approval - send request to webview and wait for response
        return this.#requestUserApproval(params, config.agentName);
      },
    });

    // Initialize the actor
    await actor.initialize(config);

    // Store it in the map
    this.#configToActor.set(key, actor);

    return actor;
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

    logger.info("webview", "Webview resolved and created");

    // Handle webview visibility changes
    webviewView.onDidChangeVisibility(() => {
      if (webviewView.visible) {
        logger.info("webview", "Webview became visible");
        this.#onWebviewVisible();
      } else {
        logger.info("webview", "Webview became hidden");
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
              title: config.agentName,
            });

            // Get or create an actor for this configuration (may spawn process)
            const actor = await this.#getOrCreateActor(config);

            // Create a new agent session for this tab
            const agentSessionId = await actor.createSession(
              config.workspaceFolder.uri.fsPath,
            );
            this.#tabToAgentSession.set(message.tabId, agentSessionId);
            this.#agentSessionToTab.set(agentSessionId, message.tabId);

            console.log(
              `Created agent session ${agentSessionId} for tab ${message.tabId} using ${config.describe()}`,
            );
          } catch (err) {
            console.error("Failed to create agent session:", err);
          }
          break;

        case "message-ack":
          // Webview acknowledged a message - remove from queue
          this.#handleMessageAck(message.tabId, message.index);
          break;

        case "prompt":
          console.log(`Received prompt for tab ${message.tabId}`);

          // Get the agent session for this tab
          const agentSessionId = this.#tabToAgentSession.get(message.tabId);
          if (!agentSessionId) {
            console.error(`No agent session found for tab ${message.tabId}`);
            return;
          }

          // Get the configuration and actor for this tab
          const tabConfig = this.#tabToConfig.get(message.tabId);
          if (!tabConfig) {
            console.error(`No configuration found for tab ${message.tabId}`);
            return;
          }

          const tabActor = this.#configToActor.get(tabConfig.key());
          if (!tabActor) {
            console.error(
              `No actor found for configuration ${tabConfig.key()}`,
            );
            return;
          }

          console.log(`Sending prompt to agent session ${agentSessionId}`);

          // Send prompt to agent (responses come via callbacks)
          try {
            await tabActor.sendPrompt(agentSessionId, message.prompt);
          } catch (err) {
            console.error("Failed to send prompt:", err);
            // TODO: Send error message to webview
          }
          break;

        case "webview-ready":
          // Webview is initialized and ready to receive messages
          console.log("Webview ready - replaying queued messages");
          this.#replayQueuedMessages();
          break;

        case "log":
          // Webview sending a log message
          logger.info("webview", message.message, message.data);
          break;

        case "approval-response":
          // User responded to approval request
          const pending = this.#pendingApprovals.get(message.approvalId);
          if (pending) {
            this.#pendingApprovals.delete(message.approvalId);

            // Handle "bypass all" option - update settings for this agent
            if (message.bypassAll) {
              const vsConfig = vscode.workspace.getConfiguration("symposium");
              const agents = vsConfig.get<Record<string, any>>("agents", {});

              // Update the agent's bypassPermissions setting
              if (agents[pending.agentName]) {
                agents[pending.agentName].bypassPermissions = true;
                await vsConfig.update(
                  "agents",
                  agents,
                  vscode.ConfigurationTarget.Global,
                );
                logger.info("approval", "Bypass permissions enabled by user", {
                  agent: pending.agentName,
                });
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

    console.log(
      `Acked message ${ackedIndex} for tab ${tabId}, ${remaining.length} messages remain in queue`,
    );
  }

  async #requestUserApproval(
    params: acp.RequestPermissionRequest,
    agentName: string,
  ): Promise<acp.RequestPermissionResponse> {
    // Generate unique approval ID
    const approvalId = uuidv4();

    // Find the tab for this session (we don't have sessionId in params, so we'll send to all tabs)
    // For now, we'll use the first tab - TODO: improve this to target the right tab
    const tabIds = Array.from(this.#tabToAgentSession.keys());
    if (tabIds.length === 0) {
      logger.error("approval", "No tabs available for approval request");
      // Fallback: deny
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

    const tabId = tabIds[0]; // Use first tab for now

    logger.info("approval", "Requesting user approval", {
      approvalId,
      tabId,
      agent: agentName,
      toolCall: params.toolCall,
    });

    // Create a promise that will be resolved when user responds
    const approvalPromise = new Promise<acp.RequestPermissionResponse>(
      (resolve, reject) => {
        this.#pendingApprovals.set(approvalId, { resolve, reject, agentName });
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
        console.log(`Replaying message ${message.index} for tab ${tabId}`);
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
      console.error("Message missing tabId:", message);
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
      console.log(`Sending message ${index} for tab ${tabId}`);
      this.#view.webview.postMessage(indexedMessage);
    } else {
      console.log(`Queued message ${index} for tab ${tabId} (webview hidden)`);
    }
  }

  #onWebviewVisible() {
    // Visibility change detected - webview will send "webview-ready" when initialized
    console.log("Webview became visible");
  }

  #onWebviewHidden() {
    // Nothing to do - messages stay queued until acked
    console.log("Webview became hidden");
  }

  #getHtmlForWebview(webview: vscode.Webview) {
    const scriptUri = webview.asWebviewUri(
      vscode.Uri.joinPath(this.#extensionUri, "out", "webview.js"),
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
    </script>
    <script src="${scriptUri}"></script>
</body>
</html>`;
  }

  dispose() {
    // Dispose all actors
    for (const actor of this.#configToActor.values()) {
      actor.dispose();
    }
    this.#configToActor.clear();
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
            title: config.agentName,
          });

          const actor = await this.#getOrCreateActor(config);
          const agentSessionId = await actor.createSession(
            config.workspaceFolder.uri.fsPath,
          );
          this.#tabToAgentSession.set(message.tabId, agentSessionId);
          this.#agentSessionToTab.set(agentSessionId, message.tabId);

          logger.info("agent", "Agent session created", {
            tabId: message.tabId,
            agentSessionId,
            agentName: config.agentName,
            components: config.components,
          });
        } catch (err) {
          console.error("Failed to create agent session:", err);
        }
        break;

      case "prompt":
        try {
          logger.info("agent", "Received prompt", { tabId: message.tabId });

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

          logger.info("agent", "Sending prompt to agent", {
            tabId: message.tabId,
            agentSessionId,
          });

          // Send prompt to agent (responses come via callbacks)
          await tabActor.sendPrompt(agentSessionId, message.prompt);
        } catch (err) {
          logger.error("agent", "Failed to send prompt", { error: err });
        }
        break;

      case "approval-response":
        // User responded to approval request
        const pending = this.#pendingApprovals.get(message.approvalId);
        if (pending) {
          this.#pendingApprovals.delete(message.approvalId);

          // Handle "bypass all" option - update settings for this agent
          if (message.bypassAll) {
            const vsConfig = vscode.workspace.getConfiguration("symposium");
            const agents = vsConfig.get<Record<string, any>>("agents", {});

            // Update the agent's bypassPermissions setting
            if (agents[pending.agentName]) {
              agents[pending.agentName].bypassPermissions = true;
              await vsConfig.update(
                "agents",
                agents,
                vscode.ConfigurationTarget.Global,
              );
              logger.info("approval", "Bypass permissions enabled by user", {
                agent: pending.agentName,
              });
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
  }
}
