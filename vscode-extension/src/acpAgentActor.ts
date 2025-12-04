/**
 * AcpAgentActor - Real ACP agent integration
 *
 * Spawns an ACP agent process (e.g., elizacp, Claude Code) and manages
 * communication via the Agent Client Protocol over stdio.
 */

import { spawn, ChildProcess } from "child_process";
import { Writable, Readable } from "stream";
import * as acp from "@agentclientprotocol/sdk";
import * as vscode from "vscode";
import { AgentConfiguration } from "./agentConfiguration";
import { logger } from "./extension";

/**
 * Tool call information passed to callbacks
 */
export interface ToolCallInfo {
  toolCallId: string;
  title: string;
  status: acp.ToolCallStatus;
  kind?: acp.ToolKind;
  rawInput?: Record<string, unknown>;
  rawOutput?: Record<string, unknown>;
}

/**
 * Slash command information passed to callbacks
 */
export interface SlashCommandInfo {
  name: string;
  description: string;
  inputHint?: string;
}

/**
 * Callback interface for agent events
 */
export interface AcpAgentCallbacks {
  onAgentText: (agentSessionId: string, text: string) => void;
  onAgentComplete: (agentSessionId: string) => void;
  onRequestPermission?: (
    params: acp.RequestPermissionRequest,
  ) => Promise<acp.RequestPermissionResponse>;
  onToolCall?: (agentSessionId: string, toolCall: ToolCallInfo) => void;
  onToolCallUpdate?: (agentSessionId: string, toolCall: ToolCallInfo) => void;
  onAvailableCommands?: (
    agentSessionId: string,
    commands: SlashCommandInfo[],
  ) => void;
}

/**
 * Implementation of the ACP Client interface
 */
class SymposiumClient implements acp.Client {
  // Cache tool call titles since updates don't include them
  private toolCallTitles: Map<string, string> = new Map();

  constructor(private callbacks: AcpAgentCallbacks) {}

  async requestPermission(
    params: acp.RequestPermissionRequest,
  ): Promise<acp.RequestPermissionResponse> {
    if (this.callbacks.onRequestPermission) {
      return this.callbacks.onRequestPermission(params);
    }

    // Default: auto-approve read operations, reject everything else
    logger.debug("approval", "Permission requested (default handler)", {
      title: params.toolCall.title,
      kind: params.toolCall.kind,
    });

    if (params.toolCall.kind === "read") {
      const allowOption = params.options.find(
        (opt) => opt.kind === "allow_once",
      );
      if (allowOption) {
        return {
          outcome: { outcome: "selected", optionId: allowOption.optionId },
        };
      }
    }

    const rejectOption = params.options.find(
      (opt) => opt.kind === "reject_once",
    );
    if (rejectOption) {
      return {
        outcome: { outcome: "selected", optionId: rejectOption.optionId },
      };
    }

    // Fallback: cancel
    return { outcome: { outcome: "cancelled" } };
  }

  async sessionUpdate(params: acp.SessionNotification): Promise<void> {
    const update = params.update;

    switch (update.sessionUpdate) {
      case "agent_message_chunk":
        if (update.content.type === "text") {
          const text = update.content.text;
          logger.debug("agent", "Text chunk", {
            length: text.length,
            text: text.length > 50 ? text.slice(0, 50) + "..." : text,
          });
          this.callbacks.onAgentText(params.sessionId, update.content.text);
        }
        break;
      case "tool_call":
        logger.debug("agent", "Tool call", {
          toolCallId: update.toolCallId,
          title: update.title,
          status: update.status,
        });
        // Cache the title for later updates
        this.toolCallTitles.set(update.toolCallId, update.title);
        if (this.callbacks.onToolCall && update.status) {
          this.callbacks.onToolCall(params.sessionId, {
            toolCallId: update.toolCallId,
            title: update.title,
            status: update.status,
            kind: update.kind,
            rawInput: update.rawInput,
            rawOutput: update.rawOutput,
          });
        }
        break;
      case "tool_call_update": {
        // Look up cached title since updates don't include it
        const cachedTitle =
          update.title ?? this.toolCallTitles.get(update.toolCallId) ?? "";
        logger.debug("agent", "Tool call update", {
          toolCallId: update.toolCallId,
          title: cachedTitle,
          status: update.status,
        });
        if (this.callbacks.onToolCallUpdate && update.status) {
          this.callbacks.onToolCallUpdate(params.sessionId, {
            toolCallId: update.toolCallId,
            title: cachedTitle,
            status: update.status,
            rawInput: update.rawInput,
            rawOutput: update.rawOutput,
          });
        }
        // Clean up cache when tool call completes
        if (update.status === "completed" || update.status === "failed") {
          this.toolCallTitles.delete(update.toolCallId);
        }
        break;
      }
      case "available_commands_update": {
        const commands: SlashCommandInfo[] = update.availableCommands.map(
          (cmd) => ({
            name: cmd.name,
            description: cmd.description,
            inputHint: cmd.input?.hint,
          }),
        );
        logger.debug("agent", "Available commands update", {
          count: commands.length,
          commands: commands.map((c) => c.name),
        });
        if (this.callbacks.onAvailableCommands) {
          this.callbacks.onAvailableCommands(params.sessionId, commands);
        }
        break;
      }
    }
  }

  async readTextFile(
    params: acp.ReadTextFileRequest,
  ): Promise<acp.ReadTextFileResponse> {
    // TODO: Implement file reading through VSCode APIs
    logger.warn("fs", "Read file requested but not implemented", {
      path: params.path,
    });
    throw new Error("File reading not yet implemented");
  }

  async writeTextFile(
    params: acp.WriteTextFileRequest,
  ): Promise<acp.WriteTextFileResponse> {
    // TODO: Implement file writing through VSCode APIs
    logger.warn("fs", "Write file requested but not implemented", {
      path: params.path,
    });
    throw new Error("File writing not yet implemented");
  }
}

export class AcpAgentActor {
  private connection?: acp.ClientSideConnection;
  private agentProcess?: ChildProcess;
  private callbacks: AcpAgentCallbacks;

  constructor(callbacks: AcpAgentCallbacks) {
    this.callbacks = callbacks;
  }

  /**
   * Initialize the ACP connection by spawning the agent process
   */
  async initialize(config: AgentConfiguration): Promise<void> {
    // Read settings to build the command
    const vsConfig = vscode.workspace.getConfiguration("symposium");

    // Get conductor command
    const conductorCommand = vsConfig.get<string>(
      "conductor",
      "sacp-conductor",
    );

    // Get the agent definition
    const agents = vsConfig.get<
      Record<
        string,
        { command: string; args?: string[]; env?: Record<string, string> }
      >
    >("agents", {});
    const agent = agents[config.agentName];

    if (!agent) {
      throw new Error(
        `Agent "${config.agentName}" not found in configured agents`,
      );
    }

    // Build the agent command string (command + args)
    const agentCmd = agent.command;
    const agentArgs = agent.args || [];
    const agentCommandStr =
      agentArgs.length > 0 ? `${agentCmd} ${agentArgs.join(" ")}` : agentCmd;

    // Build conductor arguments: agent <component1> <component2> ... <agent-command>
    const conductorArgs = ["agent", ...config.components, agentCommandStr];

    logger.important("agent", "Spawning ACP agent", {
      command: conductorCommand,
      args: conductorArgs,
    });

    // Merge environment variables
    const env = agent.env ? { ...process.env, ...agent.env } : process.env;

    // Spawn the agent process
    this.agentProcess = spawn(conductorCommand, conductorArgs, {
      stdio: ["pipe", "pipe", "inherit"],
      env: env as NodeJS.ProcessEnv,
    });

    // Create streams for communication
    const input = Writable.toWeb(this.agentProcess.stdin!);
    const output = Readable.toWeb(
      this.agentProcess.stdout!,
    ) as ReadableStream<Uint8Array>;

    // Create the client connection
    const client = new SymposiumClient(this.callbacks);
    const stream = acp.ndJsonStream(input, output);
    this.connection = new acp.ClientSideConnection((_agent) => client, stream);

    // Initialize the connection
    const initResult = await this.connection.initialize({
      protocolVersion: acp.PROTOCOL_VERSION,
      clientCapabilities: {
        fs: {
          readTextFile: false, // TODO: Enable when implemented
          writeTextFile: false,
        },
      },
    });

    logger.important("agent", "Connected to ACP agent", {
      protocolVersion: initResult.protocolVersion,
    });
  }

  /**
   * Create a new agent session
   * @param workspaceFolder - Workspace folder to use as working directory
   * @returns Agent session ID
   */
  async createSession(workspaceFolder: string): Promise<string> {
    if (!this.connection) {
      throw new Error("ACP connection not initialized");
    }

    // Create a session with the ACP agent
    const result = await this.connection.newSession({
      cwd: workspaceFolder,
      mcpServers: [],
    });

    logger.important("agent", "Created agent session", {
      sessionId: result.sessionId,
      cwd: workspaceFolder,
    });
    return result.sessionId;
  }

  /**
   * Send a prompt to an agent session
   * This returns immediately - responses come via callbacks
   *
   * @param agentSessionId - Agent session identifier
   * @param prompt - User prompt text
   */
  async sendPrompt(
    agentSessionId: string,
    prompt: string | acp.ContentBlock[],
  ): Promise<void> {
    if (!this.connection) {
      throw new Error("ACP connection not initialized");
    }

    // Build content blocks
    const contentBlocks: acp.ContentBlock[] =
      typeof prompt === "string" ? [{ type: "text", text: prompt }] : prompt;

    // Log the prompt (truncate text for logging)
    const textContent = contentBlocks
      .filter((b) => b.type === "text")
      .map((b) => (b as { type: "text"; text: string }).text)
      .join("");
    const truncatedPrompt =
      textContent.length > 100
        ? textContent.slice(0, 100) + "..."
        : textContent;
    const resourceCount = contentBlocks.filter(
      (b) => b.type === "resource",
    ).length;

    logger.debug("agent", "Sending prompt to agent session", {
      agentSessionId,
      promptLength: textContent.length,
      prompt: truncatedPrompt,
      resourceCount,
    });

    // Send the prompt (this will complete when agent finishes)
    const promptResult = await this.connection.prompt({
      sessionId: agentSessionId,
      prompt: contentBlocks,
    });

    logger.debug("agent", "Prompt completed", {
      stopReason: promptResult.stopReason,
    });

    // Notify completion
    this.callbacks.onAgentComplete(agentSessionId);
  }

  /**
   * Cleanup - kill the agent process
   */
  dispose(): void {
    if (this.agentProcess) {
      this.agentProcess.kill();
      this.agentProcess = undefined;
    }
  }
}
