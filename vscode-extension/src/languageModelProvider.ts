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
  getEffectiveAgents,
  getAgentById,
  resolveDistribution,
  ResolvedCommand,
  AgentConfig,
  fetchRegistry,
  addAgentFromRegistry,
  RegistryEntry,
} from "./agentRegistry";

/**
 * Tool definition passed in request options.
 * Matches VS Code's tool format in LanguageModelChatRequestOptions.
 */
interface ToolDefinition {
  name: string;
  description: string;
  inputSchema: object;
}

/**
 * Tool mode enum matching VS Code's LanguageModelChatToolMode.
 */
type ToolMode = "auto" | "required";

/**
 * Options for chat requests.
 * Matches VS Code's LanguageModelChatRequestOptions.
 */
interface ChatRequestOptions {
  tools?: ToolDefinition[];
  toolMode?: ToolMode;
}

/**
 * Content parts for messages and streaming responses.
 * Unified type matching VS Code's LanguageModel API naming.
 */
type ContentPart =
  | { type: "text"; value: string }
  | {
      type: "tool_call";
      toolCallId: string;
      toolName: string;
      parameters: object;
    }
  | {
      type: "tool_result";
      toolCallId: string;
      result: unknown;
    };

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
// eslint-disable-next-line @typescript-eslint/naming-convention -- matches Rust serde naming
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

  // eslint-disable-next-line @typescript-eslint/naming-convention -- matches Rust serde naming
  return { mcp_server: { name, command, args, env: envArray } };
}

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
      progress?: vscode.Progress<vscode.LanguageModelResponsePart>;
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
      const params = msg.params as { requestId: number; part: ContentPart };
      const pending = this.pendingRequests.get(params.requestId);
      if (pending?.progress) {
        const part = params.part;
        if (part.type === "text") {
          pending.progress.report(new vscode.LanguageModelTextPart(part.value));
        } else if (part.type === "tool_call") {
          pending.progress.report(
            new vscode.LanguageModelToolCallPart(
              part.toolCallId,
              part.toolName,
              part.parameters,
            ),
          );
        }
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
   * Send a JSON-RPC notification (no response expected)
   */
  private sendNotification(method: string, params: unknown): void {
    const proc = this.ensureProcess();

    const notification: JsonRpcMessage = {
      jsonrpc: "2.0",
      method,
      params,
    };

    const json = JSON.stringify(notification);
    logger.debug("lm-provider", `sending notification: ${json}`);
    proc.stdin?.write(json + "\n");
  }

  /**
   * Send a JSON-RPC request and wait for response
   *
   * @param method - The JSON-RPC method name
   * @param params - The request parameters
   * @param progress - Optional progress reporter for streaming responses
   * @param token - Optional cancellation token. If provided and cancelled,
   *                sends lm/cancel and throws CancellationError.
   */
  private async sendRequest(
    method: string,
    params: unknown,
    progress?: vscode.Progress<vscode.LanguageModelResponsePart>,
    token?: vscode.CancellationToken,
  ): Promise<unknown> {
    const proc = this.ensureProcess();
    const id = ++this.requestId;

    const request: JsonRpcMessage = {
      jsonrpc: "2.0",
      id,
      method,
      params,
    };

    // Set up cancellation handler
    let cancelDisposable: vscode.Disposable | undefined;
    if (token) {
      cancelDisposable = token.onCancellationRequested(() => {
        logger.debug("lm-provider", `cancellation requested for request ${id}`);
        this.sendNotification("lm/cancel", { requestId: id });
      });
    }

    try {
      return await new Promise((resolve, reject) => {
        this.pendingRequests.set(id, { resolve, reject, progress });

        const json = JSON.stringify(request);
        logger.debug("lm-provider", `sending: ${json}`);
        proc.stdin?.write(json + "\n");
      });
    } finally {
      cancelDisposable?.dispose();
    }
  }

  /**
   * Provide information about available language models.
   * Returns one model per effective agent, plus uninstalled agents from the registry.
   */
  async provideLanguageModelChatInformation(
    _options: { silent: boolean },
    _token: vscode.CancellationToken,
  ): Promise<vscode.LanguageModelChatInformation[]> {
    const effectiveAgents = getEffectiveAgents();
    const effectiveIds = new Set(effectiveAgents.map((a) => a.id));

    // Fetch registry agents (non-blocking failure - just use effective agents if fetch fails)
    let registryAgents: RegistryEntry[] = [];
    try {
      registryAgents = await fetchRegistry();
    } catch (error) {
      logger.warn("lm", `Failed to fetch agent registry: ${error}`);
    }

    // Filter registry to agents not already in effective list
    const uninstalledAgents = registryAgents.filter(
      (a) => !effectiveIds.has(a.id),
    );

    // Combine effective + uninstalled registry agents, sorted alphabetically
    const allAgents: Array<AgentConfig | RegistryEntry> = [
      ...effectiveAgents,
      ...uninstalledAgents,
    ].sort((a, b) => (a.name ?? a.id).localeCompare(b.name ?? b.id));

    return allAgents.map((agent) => ({
      id: agent.id,
      name: `${agent.name ?? agent.id} (ACP)`,
      family: "symposium",
      version: agent.version ?? "1.0.0",
      maxInputTokens: 100000,
      maxOutputTokens: 100000,
      capabilities: {
        toolCalling: true,
      },
    }));
  }

  /**
   * Provide a chat response from the language model
   */
  async provideLanguageModelChatResponse(
    model: vscode.LanguageModelChatInformation,
    messages: readonly vscode.LanguageModelChatRequestMessage[],
    options: vscode.ProvideLanguageModelChatResponseOptions,
    progress: vscode.Progress<vscode.LanguageModelTextPart>,
    token: vscode.CancellationToken,
  ): Promise<void> {
    // Look up the agent by the model ID (which is the agent ID)
    let agent = getAgentById(model.id);

    // If not found in effective agents, try to install from registry
    if (!agent) {
      agent = await this.installAgentFromRegistry(model.id);
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

    // Convert options to our format
    const convertedOptions: ChatRequestOptions = {
      tools: options.tools?.map((tool) => ({
        name: tool.name,
        description: tool.description,
        inputSchema: tool.inputSchema ?? {},
      })),
      toolMode: this.toolModeToString(options.toolMode),
    };

    logger.debug(
      "lm-provider",
      `provideLanguageModelChatResponse: agent=${agent.id}, tools=${convertedOptions.tools?.length ?? 0}`,
    );

    try {
      await this.sendRequest(
        "lm/provideLanguageModelChatResponse",
        {
          modelId: model.id,
          messages: convertedMessages,
          agent: agentDef,
          options: convertedOptions,
        },
        progress,
        token,
      );
    } catch (err) {
      // Check if this is a cancellation error from the backend
      if (
        err instanceof Error &&
        err.message.toLowerCase().includes("cancelled")
      ) {
        throw new vscode.CancellationError();
      }
      throw err;
    }
  }

  /**
   * Convert tool mode enum to string
   */
  private toolModeToString(
    mode: vscode.LanguageModelChatToolMode | undefined,
  ): ToolMode | undefined {
    if (mode === undefined) {
      return undefined;
    }
    switch (mode) {
      case vscode.LanguageModelChatToolMode.Auto:
        return "auto";
      case vscode.LanguageModelChatToolMode.Required:
        return "required";
      default:
        return "auto";
    }
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
  private contentToArray(content: ReadonlyArray<unknown>): ContentPart[] {
    return content.flatMap((part): ContentPart[] => {
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
  // eslint-disable-next-line @typescript-eslint/naming-convention -- UPPER_SNAKE_CASE for constants
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
   * Install an agent from the registry by ID.
   * Fetches the registry, finds the agent, and adds it to settings.
   * Returns the agent config after installation.
   */
  private async installAgentFromRegistry(
    agentId: string,
  ): Promise<AgentConfig> {
    logger.info("lm", `Installing agent from registry: ${agentId}`);

    // Fetch the registry
    let registryAgents: RegistryEntry[];
    try {
      registryAgents = await fetchRegistry();
    } catch (error) {
      throw new Error(
        `Failed to fetch agent registry: ${error instanceof Error ? error.message : String(error)}`,
      );
    }

    // Find the agent in the registry
    const registryEntry = registryAgents.find((a) => a.id === agentId);
    if (!registryEntry) {
      throw new Error(`Agent "${agentId}" not found in registry`);
    }

    // Add to settings
    await addAgentFromRegistry(registryEntry);
    logger.info("lm", `Installed agent: ${registryEntry.name}`);

    // Return as AgentConfig
    return {
      id: registryEntry.id,
      name: registryEntry.name,
      version: registryEntry.version,
      description: registryEntry.description,
      distribution: registryEntry.distribution,
      _source: "registry",
    };
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
