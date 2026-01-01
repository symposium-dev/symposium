/**
 * VS Code Language Model Provider
 *
 * Exposes Symposium as a language model in VS Code's model picker.
 * This bridges the VS Code Language Model API (stateless) to the
 * Rust vscodelm backend which manages ACP sessions.
 */

import * as vscode from "vscode";
import * as cp from "child_process";
import { getConductorCommand } from "./binaryPath";
import { logger } from "./extension";
import {
  getCurrentAgent,
  resolveDistribution,
  ResolvedCommand,
} from "./agentRegistry";

interface ResponsePart {
  type: "text";
  value: string;
}

/**
 * MCP Server configuration matching sacp::schema::McpServerStdio
 */
interface McpServerStdio {
  name: string;
  command: string;
  args: string[];
  env: Array<{ name: string; value: string }>;
}

/**
 * Agent definition - matches session_actor::AgentDefinition in Rust.
 * The protocol supports both eliza and mcp_server variants, but the
 * extension always sends mcp_server (resolving builtins to the binary path).
 */
type AgentDefinition = { mcp_server: McpServerStdio };

/**
 * Convert a resolved agent command to AgentDefinition format.
 * Always produces mcp_server variant - symposium builtins are resolved
 * to the embedded binary path.
 */
function resolvedCommandToAgentDefinition(
  name: string,
  resolved: ResolvedCommand,
  context: vscode.ExtensionContext,
): AgentDefinition {
  let command: string;
  let args: string[];

  if (resolved.isSymposiumBuiltin) {
    // For symposium builtins, use the embedded binary with the subcommand
    command = getConductorCommand(context);
    args = [resolved.command, ...resolved.args];
  } else {
    command = resolved.command;
    args = resolved.args;
  }

  const envArray = resolved.env
    ? Object.entries(resolved.env).map(([k, v]) => ({ name: k, value: v }))
    : [];

  return {
    mcp_server: {
      name,
      command,
      args,
      env: envArray,
    },
  };
}

/**
 * Message content parts - discriminated union matching VS Code's LM API types
 */
type MessageContentPart =
  | { type: "text"; value: string }
  | {
      type: "tool_call";
      toolCallId: string;
      toolName: string;
      parameters: unknown;
    }
  | { type: "tool_result"; toolCallId: string; result: unknown };

interface JsonRpcMessage {
  jsonrpc: "2.0";
  id?: number | string;
  method?: string;
  params?: unknown;
  result?: unknown;
  error?: { code: number; message: string };
}

/**
 * Language Model Provider that connects to the Rust vscodelm backend
 */
export class SymposiumLanguageModelProvider
  implements vscode.LanguageModelChatProvider
{
  private context: vscode.ExtensionContext;
  private process: cp.ChildProcess | null = null;
  private requestId = 0;
  private pendingRequests: Map<
    number,
    {
      resolve: (value: unknown) => void;
      reject: (error: Error) => void;
      progress?: vscode.Progress<vscode.LanguageModelTextPart>;
    }
  > = new Map();
  private buffer = "";

  constructor(context: vscode.ExtensionContext) {
    this.context = context;
  }

  /**
   * Ensure the vscodelm process is running
   */
  private ensureProcess(): cp.ChildProcess {
    if (this.process && this.process.exitCode === null) {
      return this.process;
    }

    const command = getConductorCommand(this.context);

    // Build spawn args with logging options from settings
    const spawnArgs: string[] = [];

    const vsConfig = vscode.workspace.getConfiguration("symposium");
    let logLevel = vsConfig.get<string>("agentLogLevel", "");
    if (!logLevel) {
      const generalLogLevel = vsConfig.get<string>("logLevel", "error");
      if (generalLogLevel === "debug") {
        logLevel = "debug";
      }
    }
    if (logLevel) {
      spawnArgs.push("--log", logLevel);
    }

    const traceDir = vsConfig.get<string>("traceDir", "");
    if (traceDir) {
      spawnArgs.push("--trace-dir", traceDir);
    }

    spawnArgs.push("vscodelm");

    logger.important("lm-provider", "Spawning vscodelm process", {
      command,
      args: spawnArgs,
    });

    this.process = cp.spawn(command, spawnArgs, {
      stdio: ["pipe", "pipe", "pipe"],
    });

    logger.important("lm-provider", "vscodelm process started", {
      pid: this.process.pid,
    });

    this.process.stdout?.on("data", (data: Buffer) => {
      this.handleData(data.toString());
    });

    this.process.stderr?.on("data", (data: Buffer) => {
      const lines = data
        .toString()
        .split("\n")
        .filter((line) => line.trim());
      for (const line of lines) {
        logger.info("lm-stderr", line);
      }
    });

    this.process.on("exit", (code) => {
      logger.info("lm-provider", `vscodelm process exited with code ${code}`);
      this.process = null;
      // Reject any pending requests
      for (const [id, pending] of this.pendingRequests) {
        pending.reject(new Error(`Process exited with code ${code}`));
        this.pendingRequests.delete(id);
      }
    });

    this.process.on("error", (err) => {
      logger.error("lm-provider", `vscodelm process error: ${err.message}`);
    });

    return this.process;
  }

  /**
   * Handle incoming data from the process
   */
  private handleData(data: string): void {
    this.buffer += data;

    // Process complete lines
    let newlineIndex: number;
    while ((newlineIndex = this.buffer.indexOf("\n")) !== -1) {
      const line = this.buffer.slice(0, newlineIndex).trim();
      this.buffer = this.buffer.slice(newlineIndex + 1);

      if (line) {
        this.handleMessage(line);
      }
    }
  }

  /**
   * Handle a single JSON-RPC message
   */
  private handleMessage(line: string): void {
    logger.debug("lm-provider", `received: ${line}`);

    let msg: JsonRpcMessage;
    try {
      msg = JSON.parse(line);
    } catch (e) {
      logger.error("lm-provider", `Failed to parse JSON: ${line}`);
      return;
    }

    // Handle notifications (streaming responses)
    if (msg.method === "lm/responsePart") {
      const params = msg.params as { requestId: number; part: ResponsePart };
      const pending = this.pendingRequests.get(params.requestId);
      if (pending?.progress && params.part.type === "text") {
        pending.progress.report(
          new vscode.LanguageModelTextPart(params.part.value),
        );
      }
      return;
    }

    if (msg.method === "lm/responseComplete") {
      // Response streaming complete, but we wait for the actual response
      return;
    }

    // Handle responses
    if (msg.id !== undefined) {
      const id = typeof msg.id === "string" ? parseInt(msg.id, 10) : msg.id;
      const pending = this.pendingRequests.get(id);
      if (pending) {
        this.pendingRequests.delete(id);
        if (msg.error) {
          pending.reject(new Error(msg.error.message));
        } else {
          pending.resolve(msg.result);
        }
      }
    }
  }

  /**
   * Send a JSON-RPC request and wait for response
   */
  private async sendRequest(
    method: string,
    params: unknown,
    progress?: vscode.Progress<vscode.LanguageModelTextPart>,
  ): Promise<unknown> {
    const proc = this.ensureProcess();
    const id = ++this.requestId;

    const request: JsonRpcMessage = {
      jsonrpc: "2.0",
      id,
      method,
      params,
    };

    return new Promise((resolve, reject) => {
      this.pendingRequests.set(id, { resolve, reject, progress });

      const json = JSON.stringify(request);
      logger.debug("lm-provider", `sending: ${json}`);
      proc.stdin?.write(json + "\n");
    });
  }

  /**
   * Provide information about available language models
   */
  async provideLanguageModelChatInformation(
    _options: { silent: boolean },
    _token: vscode.CancellationToken,
  ): Promise<vscode.LanguageModelChatInformation[]> {
    const result = (await this.sendRequest(
      "lm/provideLanguageModelChatInformation",
      {},
    )) as {
      models: Array<{
        id: string;
        name: string;
        family: string;
        version: string;
        maxInputTokens: number;
        maxOutputTokens: number;
        capabilities: { toolCalling?: boolean };
      }>;
    };

    return result.models.map((info) => ({
      id: info.id,
      name: info.name,
      family: info.family,
      version: info.version,
      maxInputTokens: info.maxInputTokens,
      maxOutputTokens: info.maxOutputTokens,
      capabilities: {
        toolCalling: info.capabilities.toolCalling ?? false,
      },
    }));
  }

  /**
   * Provide a chat response from the language model
   */
  async provideLanguageModelChatResponse(
    model: vscode.LanguageModelChatInformation,
    messages: readonly vscode.LanguageModelChatRequestMessage[],
    _options: unknown,
    progress: vscode.Progress<vscode.LanguageModelTextPart>,
    token: vscode.CancellationToken,
  ): Promise<void> {
    // Get the current agent configuration
    const agent = getCurrentAgent();
    if (!agent) {
      throw new Error("No agent configured");
    }

    // Resolve the agent distribution to a spawn command
    const resolved = await resolveDistribution(agent);

    // Convert to AgentDefinition format (resolves builtins to binary path)
    const agentDef = resolvedCommandToAgentDefinition(
      agent.name ?? agent.id,
      resolved,
      this.context,
    );

    // Convert VS Code messages to our format
    const convertedMessages = messages.map((msg) => ({
      role: this.roleToString(msg.role),
      content: this.contentToArray(msg.content),
    }));

    logger.debug(
      "lm-provider",
      `provideLanguageModelChatResponse: agent=${agent.id}, messages=${JSON.stringify(convertedMessages)}`,
    );

    // Set up cancellation
    const abortController = new AbortController();
    token.onCancellationRequested(() => {
      abortController.abort();
      // TODO: Send cancellation to the process
    });

    await this.sendRequest(
      "lm/provideLanguageModelChatResponse",
      { modelId: model.id, messages: convertedMessages, agent: agentDef },
      progress,
    );
  }

  /**
   * Provide token count for text or a message
   */
  async provideTokenCount(
    model: vscode.LanguageModelChatInformation,
    text: string | vscode.LanguageModelChatRequestMessage,
    _token: vscode.CancellationToken,
  ): Promise<number> {
    const textStr =
      typeof text === "string" ? text : this.messageToString(text);
    const result = (await this.sendRequest("lm/provideTokenCount", {
      modelId: model.id,
      text: textStr,
    })) as number;
    return result;
  }

  /**
   * Convert role enum to string
   */
  private roleToString(role: vscode.LanguageModelChatMessageRole): string {
    switch (role) {
      case vscode.LanguageModelChatMessageRole.User:
        return "user";
      case vscode.LanguageModelChatMessageRole.Assistant:
        return "assistant";
      default:
        return "user";
    }
  }

  /**
   * Convert message content to array format
   */
  private contentToArray(
    content: ReadonlyArray<unknown>,
  ): MessageContentPart[] {
    return content.flatMap((part): MessageContentPart[] => {
      if (part instanceof vscode.LanguageModelTextPart) {
        return [{ type: "text", value: part.value }];
      }
      if (part instanceof vscode.LanguageModelToolCallPart) {
        return [
          {
            type: "tool_call",
            toolCallId: part.callId,
            toolName: part.name,
            parameters: part.input,
          },
        ];
      }
      if (part instanceof vscode.LanguageModelToolResultPart) {
        return [
          {
            type: "tool_result",
            toolCallId: part.callId,
            result: part.content,
          },
        ];
      }
      // Handle known-but-unsupported VS Code/Copilot internal types
      if (this.isKnownUnsupportedPart(part)) {
        return [];
      }
      // Log truly unknown parts as errors
      logger.error("lm", "Skipping unknown message part type", {
        type: part?.constructor?.name ?? typeof part,
        json: JSON.stringify(part, null, 2),
      });
      return [];
    });
  }

  /**
   * Known VS Code/Copilot internal message part mimeTypes that we ignore.
   * These are undocumented and not relevant to our use case.
   */
  private static readonly KNOWN_IGNORED_MIMETYPES = new Set([
    "cache_control", // Copilot cache hints (e.g., "ephemeral")
    "stateful_marker", // Copilot session tracking
  ]);

  /**
   * Check if a part is a known-but-unsupported VS Code/Copilot internal type.
   * These are logged at debug level and silently ignored.
   */
  private isKnownUnsupportedPart(part: unknown): boolean {
    if (typeof part !== "object" || part === null) {
      return false;
    }
    const mimeType = (part as { mimeType?: string }).mimeType;
    if (
      mimeType &&
      SymposiumLanguageModelProvider.KNOWN_IGNORED_MIMETYPES.has(mimeType)
    ) {
      logger.debug("lm", `Ignoring known unsupported part: ${mimeType}`);
      return true;
    }
    return false;
  }

  /**
   * Convert a message to string for token counting
   */
  private messageToString(msg: vscode.LanguageModelChatRequestMessage): string {
    return msg.content
      .map((part) => {
        if (part instanceof vscode.LanguageModelTextPart) {
          return part.value;
        }
        if (part instanceof vscode.LanguageModelToolCallPart) {
          return `[tool:${part.name}]`;
        }
        if (part instanceof vscode.LanguageModelToolResultPart) {
          return "[tool_result]";
        }
        // Skip unknown parts for token counting
        return "";
      })
      .join("");
  }

  /**
   * Clean up resources
   */
  dispose(): void {
    if (this.process) {
      this.process.kill();
      this.process = null;
    }
  }
}
