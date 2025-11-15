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

/**
 * Callback interface for agent events
 */
export interface AcpAgentCallbacks {
  onAgentText: (agentSessionId: string, text: string) => void;
  onAgentComplete: (agentSessionId: string) => void;
  onRequestPermission?: (
    params: acp.RequestPermissionRequest,
  ) => Promise<acp.RequestPermissionResponse>;
}

/**
 * Implementation of the ACP Client interface
 */
class SymposiumClient implements acp.Client {
  constructor(private callbacks: AcpAgentCallbacks) {}

  async requestPermission(
    params: acp.RequestPermissionRequest,
  ): Promise<acp.RequestPermissionResponse> {
    if (this.callbacks.onRequestPermission) {
      return this.callbacks.onRequestPermission(params);
    }

    // Default: auto-approve read operations, reject everything else
    console.log(
      `Permission requested: ${params.toolCall.title} (${params.toolCall.kind})`,
    );

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
          this.callbacks.onAgentText(params.sessionId, update.content.text);
        }
        break;
      case "tool_call":
        console.log(`Tool call: ${update.title} (${update.status})`);
        break;
      case "tool_call_update":
        console.log(
          `Tool call update: ${update.toolCallId} - ${update.status}`,
        );
        break;
    }
  }

  async readTextFile(
    params: acp.ReadTextFileRequest,
  ): Promise<acp.ReadTextFileResponse> {
    // TODO: Implement file reading through VSCode APIs
    console.log(`Read file requested: ${params.path}`);
    throw new Error("File reading not yet implemented");
  }

  async writeTextFile(
    params: acp.WriteTextFileRequest,
  ): Promise<acp.WriteTextFileResponse> {
    // TODO: Implement file writing through VSCode APIs
    console.log(`Write file requested: ${params.path}`);
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

    console.log(
      `Spawning ACP agent: ${conductorCommand} ${conductorArgs.join(" ")}`,
    );

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

    console.log(
      `✅ Connected to ACP agent (protocol v${initResult.protocolVersion})`,
    );
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

    console.log(
      `Created agent session: ${result.sessionId} (cwd: ${workspaceFolder})`,
    );
    return result.sessionId;
  }

  /**
   * Send a prompt to an agent session
   * This returns immediately - responses come via callbacks
   *
   * @param agentSessionId - Agent session identifier
   * @param prompt - User prompt text
   */
  async sendPrompt(agentSessionId: string, prompt: string): Promise<void> {
    if (!this.connection) {
      throw new Error("ACP connection not initialized");
    }

    console.log(`Sending prompt to agent session ${agentSessionId}`);

    // Send the prompt (this will complete when agent finishes)
    const promptResult = await this.connection.prompt({
      sessionId: agentSessionId,
      prompt: [
        {
          type: "text",
          text: prompt,
        },
      ],
    });

    console.log(
      `Prompt completed with stop reason: ${promptResult.stopReason}`,
    );

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
