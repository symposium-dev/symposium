/**
 * AcpAgentActor - Real ACP agent integration
 *
 * Spawns an ACP agent process (e.g., elizacp, Claude Code) and manages
 * communication via the Agent Client Protocol over stdio.
 */

import { spawn, ChildProcess, SpawnOptions } from "child_process";
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
  onUserText: (agentSessionId: string, text: string) => void;
  onAgentComplete: (agentSessionId: string) => void;
  onStartupSlow?: (context: StartupWatchdogContext) => void;
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

const DEFAULT_STARTUP_WATCHDOG = {
  slowThresholdMs: 10000,
  hardTimeoutMs: 30000,
} as const;

type StartupFailureReason =
  | "initialize-rejected"
  | "hard-timeout"
  | "process-exit"
  | "stdout-close"
  | "process-error";

export interface StartupWatchdogFailureDetails {
  reason: StartupFailureReason;
  phase: "initialize";
  elapsedMs: number;
  slowThresholdMs: number;
  hardTimeoutMs: number;
  slowThresholdExceeded: boolean;
  command: string;
  args: string[];
  pid?: number;
  exitCode?: number | null;
  signal?: NodeJS.Signals | null;
  initializeError?: string;
  killIssued?: boolean;
}

type StartupWatchdogDiagnosticReason =
  | "initialize_error"
  | "hard_timeout"
  | "process_exit"
  | "stdout_close"
  | "process_error";

export interface StartupWatchdogDiagnostics {
  reason: StartupWatchdogDiagnosticReason;
  phase: "initialize";
  elapsedMs: number;
  slowThresholdMs: number;
  hardTimeoutMs: number;
  command: string;
  args: string[];
  pid?: number;
  exitCode?: number | null;
  signal?: NodeJS.Signals | null;
  stream?: "stdout";
  errorMessage?: string;
}

function toStartupWatchdogDiagnostics(
  details: StartupWatchdogFailureDetails,
): StartupWatchdogDiagnostics {
  const reasonMap: Record<StartupFailureReason, StartupWatchdogDiagnosticReason> = {
    "initialize-rejected": "initialize_error",
    "hard-timeout": "hard_timeout",
    "process-exit": "process_exit",
    "stdout-close": "stdout_close",
    "process-error": "process_error",
  };

  return {
    reason: reasonMap[details.reason],
    phase: details.phase,
    elapsedMs: details.elapsedMs,
    slowThresholdMs: details.slowThresholdMs,
    hardTimeoutMs: details.hardTimeoutMs,
    command: details.command,
    args: details.args,
    pid: details.pid,
    exitCode: details.exitCode,
    signal: details.signal,
    stream: details.reason === "stdout-close" ? "stdout" : undefined,
    errorMessage: details.initializeError,
  };
}

export interface StartupWatchdogContext {
  phase: "initialize";
  elapsedMs: number;
  slowThresholdMs: number;
  hardTimeoutMs: number;
  command: string;
  args: string[];
  pid?: number;
}

export class StartupWatchdogError extends Error {
  readonly details: StartupWatchdogFailureDetails;
  readonly diagnostics: StartupWatchdogDiagnostics;

  constructor(details: StartupWatchdogFailureDetails) {
    super(formatStartupWatchdogError(details));
    this.name = "StartupWatchdogError";
    this.details = details;
    this.diagnostics = toStartupWatchdogDiagnostics(details);
  }
}

interface StartupWatchdogConfig {
  slowThresholdMs: number;
  hardTimeoutMs: number;
}

interface RunStartupWatchdogOptions<T> {
  phase: "initialize";
  command: string;
  args: string[];
  process: ChildProcess;
  slowThresholdMs: number;
  hardTimeoutMs: number;
  initialize: () => Promise<T>;
  onSlowThreshold?: (context: StartupWatchdogContext) => void;
  onHardTimeout?: (context: StartupWatchdogContext) => void;
}

type AcpConnection = Pick<
  acp.ClientSideConnection,
  "initialize" | "newSession" | "prompt" | "cancel"
>;

type SpawnProcessFn = (
  command: string,
  args: string[],
  options: SpawnOptions,
) => ChildProcess;

type CreateConnectionFn = (
  client: SymposiumClient,
  input: WritableStream<Uint8Array>,
  output: ReadableStream<Uint8Array>,
) => AcpConnection;

export interface AcpAgentActorDependencies {
  spawnProcess?: SpawnProcessFn;
  createConnection?: CreateConnectionFn;
  startupWatchdog?: StartupWatchdogConfig;
}

function formatStartupWatchdogError(
  details: StartupWatchdogFailureDetails,
): string {
  const segments = [
    "ACP startup failed",
    `reason=${details.reason}`,
    `phase=${details.phase}`,
    `elapsedMs=${details.elapsedMs}`,
    `slowThresholdMs=${details.slowThresholdMs}`,
    `hardTimeoutMs=${details.hardTimeoutMs}`,
  ];

  if (details.exitCode !== undefined) {
    segments.push(`exitCode=${details.exitCode}`);
  }
  if (details.signal !== undefined) {
    segments.push(`signal=${details.signal}`);
  }
  if (details.initializeError) {
    segments.push(`initializeError=${details.initializeError}`);
  }

  return segments.join(" ");
}

export async function runStartupWatchdog<T>(
  options: RunStartupWatchdogOptions<T>,
): Promise<T> {
  const startedAt = Date.now();
  let slowThresholdExceeded = false;
  let settled = false;

  const buildContext = (): StartupWatchdogContext => ({
    phase: options.phase,
    elapsedMs: Date.now() - startedAt,
    slowThresholdMs: options.slowThresholdMs,
    hardTimeoutMs: options.hardTimeoutMs,
    command: options.command,
    args: options.args,
    pid: options.process.pid,
  });

  const buildDetails = (
    reason: StartupFailureReason,
    extras: Partial<StartupWatchdogFailureDetails> = {},
  ): StartupWatchdogFailureDetails => ({
    reason,
    phase: options.phase,
    elapsedMs: Date.now() - startedAt,
    slowThresholdMs: options.slowThresholdMs,
    hardTimeoutMs: options.hardTimeoutMs,
    slowThresholdExceeded,
    command: options.command,
    args: options.args,
    pid: options.process.pid,
    ...extras,
  });

  const toErrorMessage = (error: unknown): string => {
    if (error instanceof Error) {
      return error.message;
    }
    if (typeof error === "string") {
      return error;
    }
    if (
      error &&
      typeof error === "object" &&
      "message" in error &&
      typeof (error as Record<string, unknown>).message === "string"
    ) {
      return (error as Record<string, string>).message;
    }

    return JSON.stringify(error);
  };

  return new Promise<T>((resolve, reject) => {
    let slowTimer: NodeJS.Timeout | undefined;
    let hardTimer: NodeJS.Timeout | undefined;

    const cleanup = () => {
      if (slowTimer) {
        clearTimeout(slowTimer);
        slowTimer = undefined;
      }
      if (hardTimer) {
        clearTimeout(hardTimer);
        hardTimer = undefined;
      }
      options.process.off("exit", onProcessExit);
      options.process.off("error", onProcessError);
      options.process.stdout?.off("close", onStdoutClose);
    };

    const fail = (
      reason: StartupFailureReason,
      extras: Partial<StartupWatchdogFailureDetails> = {},
      failOptions: { killProcess?: boolean; triggerHardTimeout?: boolean } = {},
    ) => {
      if (settled) {
        return;
      }

      settled = true;

      if (failOptions.triggerHardTimeout) {
        options.onHardTimeout?.(buildContext());
      }

      if (failOptions.killProcess && !options.process.killed) {
        options.process.kill();
        extras.killIssued = true;
      }

      cleanup();
      reject(new StartupWatchdogError(buildDetails(reason, extras)));
    };

    const onProcessExit = (
      code: number | null,
      signal: NodeJS.Signals | null,
    ) => {
      fail("process-exit", { exitCode: code, signal });
    };
    const onStdoutClose = () => {
      fail("stdout-close");
    };
    const onProcessError = (processError: Error) => {
      fail("process-error", { initializeError: processError.message });
    };

    options.process.once("exit", onProcessExit);
    options.process.once("error", onProcessError);
    options.process.stdout?.once("close", onStdoutClose);

    slowTimer = setTimeout(() => {
      if (settled) {
        return;
      }
      slowThresholdExceeded = true;
      options.onSlowThreshold?.(buildContext());
    }, options.slowThresholdMs);

    hardTimer = setTimeout(() => {
      fail("hard-timeout", {}, { triggerHardTimeout: true, killProcess: false });
    }, options.hardTimeoutMs);

    setImmediate(() => {
      if (settled) {
        return;
      }

      options
        .initialize()
        .then((result) => {
          if (settled) {
            return;
          }
          settled = true;
          cleanup();
          resolve(result);
        })
        .catch((initializeError: unknown) => {
          fail("initialize-rejected", {
            initializeError: toErrorMessage(initializeError),
          });
        });
    });
  });
}

/**
 * Implementation of the ACP Client interface
 */
export class SymposiumClient implements acp.Client {
  // Cache tool call state so updates that omit fields still render correctly.
  private toolCalls: Map<string, ToolCallInfo> = new Map();

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
        {
          const status = update.status ?? "in_progress";
          const toolCall: ToolCallInfo = {
            toolCallId: update.toolCallId,
            title: update.title,
            status,
            kind: update.kind,
            rawInput: update.rawInput,
            rawOutput: update.rawOutput,
          };

          logger.debug("agent", "Tool call", {
            toolCallId: toolCall.toolCallId,
            title: toolCall.title,
            status: toolCall.status,
          });

          this.toolCalls.set(update.toolCallId, toolCall);
          this.callbacks.onToolCall?.(params.sessionId, toolCall);
        }
        break;
      case "tool_call_update": {
        const previous = this.toolCalls.get(update.toolCallId);

        const title = update.title ?? previous?.title ?? "";
        const status = update.status ?? previous?.status ?? "in_progress";

        const toolCall: ToolCallInfo = {
          toolCallId: update.toolCallId,
          title,
          status,
          kind: update.kind ?? previous?.kind,
          rawInput: update.rawInput ?? previous?.rawInput,
          rawOutput: update.rawOutput ?? previous?.rawOutput,
        };

        logger.debug("agent", "Tool call update", {
          toolCallId: toolCall.toolCallId,
          title: toolCall.title,
          status: toolCall.status,
        });

        this.toolCalls.set(update.toolCallId, toolCall);
        this.callbacks.onToolCallUpdate?.(params.sessionId, toolCall);

        // Clean up cache when tool call completes
        if (toolCall.status === "completed" || toolCall.status === "failed") {
          this.toolCalls.delete(update.toolCallId);
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
      case "user_message_chunk": {
        if (update.content.type === "text") {
          const text = update.content.text;
          logger.debug("user", "Text chunk", {
            length: text.length,
            text: text.length > 50 ? text.slice(0, 50) + "..." : text,
          });
          this.callbacks.onUserText(params.sessionId, update.content.text);
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
  private connection?: AcpConnection;
  private agentProcess?: ChildProcess;
  private callbacks: AcpAgentCallbacks;
  private readonly spawnProcess: SpawnProcessFn;
  private readonly createConnection: CreateConnectionFn;
  private readonly startupWatchdog: StartupWatchdogConfig;
  private readonly hasStartupWatchdogOverride: boolean;

  constructor(
    callbacks: AcpAgentCallbacks,
    dependencies: AcpAgentActorDependencies = {},
  ) {
    this.callbacks = callbacks;
    this.spawnProcess = dependencies.spawnProcess ?? spawn;
    this.createConnection =
      dependencies.createConnection ??
      ((client, input, output) => {
        const stream = acp.ndJsonStream(input, output);
        return new acp.ClientSideConnection((_agent) => client, stream);
      });
    this.hasStartupWatchdogOverride = dependencies.startupWatchdog !== undefined;
    this.startupWatchdog =
      dependencies.startupWatchdog ?? DEFAULT_STARTUP_WATCHDOG;
  }

  private resolveStartupWatchdogFromSettings(
    config: vscode.WorkspaceConfiguration,
  ): StartupWatchdogConfig {
    if (this.hasStartupWatchdogOverride) {
      return this.startupWatchdog;
    }

    const slowThresholdMs = config.get<number>(
      "startupSlowThresholdMs",
      DEFAULT_STARTUP_WATCHDOG.slowThresholdMs,
    );
    const hardTimeoutMs = config.get<number>(
      "startupHardTimeoutMs",
      DEFAULT_STARTUP_WATCHDOG.hardTimeoutMs,
    );

    if (!Number.isInteger(slowThresholdMs) || slowThresholdMs <= 0) {
      throw new Error(
        "Invalid symposium.startupSlowThresholdMs setting. Expected a positive integer (milliseconds).",
      );
    }

    if (!Number.isInteger(hardTimeoutMs) || hardTimeoutMs <= 0) {
      throw new Error(
        "Invalid symposium.startupHardTimeoutMs setting. Expected a positive integer (milliseconds).",
      );
    }

    if (hardTimeoutMs <= slowThresholdMs) {
      throw new Error(
        "Invalid startup timeout settings: symposium.startupHardTimeoutMs must be greater than symposium.startupSlowThresholdMs.",
      );
    }

    return { slowThresholdMs, hardTimeoutMs };
  }

  /**
   * Initialize the ACP connection by spawning the agent process
   * @param config - Agent configuration (just workspace folder now)
   * @param conductorCommand - Path to the conductor/agent binary
   */
  async initialize(
    config: AgentConfiguration,
    conductorCommand: string,
  ): Promise<void> {
    // Read settings to build the command
    const vsConfig = vscode.workspace.getConfiguration("symposium");
    const startupWatchdog = this.resolveStartupWatchdogFromSettings(vsConfig);

    // Get log level if configured
    let agentLogLevel = vsConfig.get<string>("agentLogLevel", "");
    if (!agentLogLevel) {
      const generalLogLevel = vsConfig.get<string>("logLevel", "error");
      if (generalLogLevel === "debug") {
        agentLogLevel = "debug";
      }
    }

    // Build the spawn command and args - just use "run" mode
    // Symposium's ConfigAgent handles agent selection and mods
    const spawnArgs: string[] = ["run"];

    if (agentLogLevel) {
      spawnArgs.push("--log", agentLogLevel);
    }

    const traceDir = vsConfig.get<string>("traceDir", "");
    if (traceDir) {
      spawnArgs.push("--trace-dir", traceDir);
    }

    const proxySpawnArgs = vsConfig.get<string[]>("proxySpawnArgs", []);
    for (const arg of proxySpawnArgs) {
      spawnArgs.push(arg);
    }

    logger.important("agent", "Spawning ACP agent", {
      command: conductorCommand,
      args: spawnArgs,
    });

    // Spawn the agent process
    this.agentProcess = this.spawnProcess(conductorCommand, spawnArgs, {
      stdio: ["pipe", "pipe", "pipe"],
      env: process.env,
      cwd: config.workspaceFolder.uri.fsPath,
    });

    if (!this.agentProcess.stdin || !this.agentProcess.stdout) {
      throw new Error("ACP agent process missing stdio pipes");
    }

    // Capture stderr and pipe to logger
    if (this.agentProcess.stderr) {
      this.agentProcess.stderr.on("data", (data: Buffer) => {
        const lines = data
          .toString()
          .split("\n")
          .filter((line) => line.trim());
        for (const line of lines) {
          logger.info("agent-stderr", line);
        }
      });
    }

    // Create streams for communication
    const input = Writable.toWeb(this.agentProcess.stdin);
    const output = Readable.toWeb(
      this.agentProcess.stdout,
    ) as ReadableStream<Uint8Array>;

    // Create the client connection
    const client = new SymposiumClient(this.callbacks);
    this.connection = this.createConnection(client, input, output);

    const initializeRequest: acp.InitializeRequest = {
      protocolVersion: acp.PROTOCOL_VERSION,
      clientCapabilities: {
        fs: {
          readTextFile: false, // TODO: Enable when implemented
          writeTextFile: false,
        },
      },
    };

    try {
      const initResult = await this.initializeConnectionWithWatchdog(
        () => this.connection!.initialize(initializeRequest),
        {
          command: conductorCommand,
          args: spawnArgs,
        },
        startupWatchdog,
      );

      logger.important("agent", "Connected to ACP agent", {
        protocolVersion: initResult.protocolVersion,
      });
    } catch (error) {
      this.connection = undefined;
      if (
        this.agentProcess &&
        this.agentProcess.exitCode === null &&
        this.agentProcess.signalCode === null &&
        !this.agentProcess.killed
      ) {
        this.agentProcess.kill();
      }
      this.agentProcess = undefined;
      throw error;
    }
  }

  private async initializeConnectionWithWatchdog(
    runInitialize: () => Promise<acp.InitializeResponse>,
    startupContext: { command: string; args: string[] },
    startupWatchdog: StartupWatchdogConfig,
  ): Promise<acp.InitializeResponse> {
    if (!this.agentProcess) {
      throw new Error("ACP agent process not started");
    }

    const processForInit = this.agentProcess;

    try {
      return await runStartupWatchdog({
        phase: "initialize",
        command: startupContext.command,
        args: startupContext.args,
        process: processForInit,
        slowThresholdMs: startupWatchdog.slowThresholdMs,
        hardTimeoutMs: startupWatchdog.hardTimeoutMs,
        initialize: runInitialize,
        onSlowThreshold: (context) => {
          logger.warn("agent", "ACP startup exceeded slow threshold", context);
          this.callbacks.onStartupSlow?.(context);
        },
        onHardTimeout: () => {
          if (
            processForInit.exitCode === null &&
            processForInit.signalCode === null &&
            !processForInit.killed
          ) {
            processForInit.kill();
          }
        },
      });
    } catch (error) {
      if (error instanceof StartupWatchdogError) {
        logger.error("agent", "ACP startup watchdog failure", error.details);
      }
      throw error;
    }
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
   * Cancel an ongoing prompt turn for a session.
   *
   * Sends a session/cancel notification to the agent. The agent should:
   * - Stop all language model requests as soon as possible
   * - Abort all tool call invocations in progress
   * - Respond to the original prompt with stopReason: "cancelled"
   *
   * @param agentSessionId - Agent session identifier
   */
  async cancelSession(agentSessionId: string): Promise<void> {
    if (!this.connection) {
      throw new Error("ACP connection not initialized");
    }

    logger.debug("agent", "Cancelling session", { agentSessionId });
    await this.connection.cancel({ sessionId: agentSessionId });
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
