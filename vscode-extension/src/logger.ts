import * as vscode from "vscode";

export type LogLevel = "error" | "info" | "debug";

export interface LogEvent {
  timestamp: Date;
  level: LogLevel;
  category: string;
  message: string;
  data?: any;
}

// Log level priority (higher = more verbose)
const LOG_LEVEL_PRIORITY: Record<LogLevel, number> = {
  error: 0,
  info: 1,
  debug: 2,
};

/**
 * Structured logger that writes to Output channel and emits events for testing.
 * Respects the symposium.logLevel configuration setting.
 */
export class Logger {
  private outputChannel: vscode.OutputChannel;
  private eventEmitter = new vscode.EventEmitter<LogEvent>();

  constructor(name: string) {
    this.outputChannel = vscode.window.createOutputChannel(name);
  }

  public get onLog(): vscode.Event<LogEvent> {
    return this.eventEmitter.event;
  }

  /**
   * Get the configured log level from settings.
   */
  private getConfiguredLevel(): LogLevel {
    const config = vscode.workspace.getConfiguration("symposium");
    return config.get<LogLevel>("logLevel", "error");
  }

  /**
   * Check if a message at the given level should be logged.
   */
  private shouldLog(level: LogLevel): boolean {
    const configuredLevel = this.getConfiguredLevel();
    return LOG_LEVEL_PRIORITY[level] <= LOG_LEVEL_PRIORITY[configuredLevel];
  }

  /**
   * Log an info message (shown when logLevel is 'info' or 'debug').
   */
  public info(category: string, message: string, data?: any): void {
    this.log("info", category, message, data);
  }

  /**
   * Log a debug message (shown only when logLevel is 'debug').
   */
  public debug(category: string, message: string, data?: any): void {
    this.log("debug", category, message, data);
  }

  /**
   * Log a warning message (shown at 'info' level or above).
   */
  public warn(category: string, message: string, data?: any): void {
    this.log("info", category, `[WARN] ${message}`, data);
  }

  /**
   * Log an error message (always shown).
   */
  public error(category: string, message: string, data?: any): void {
    this.log("error", category, message, data);
  }

  /**
   * Log an important message that should always be shown regardless of level.
   * Use for things like agent connections, session starts, etc.
   */
  public important(category: string, message: string, data?: any): void {
    this.logAlways("info", category, message, data);
  }

  private log(
    level: LogLevel,
    category: string,
    message: string,
    data?: any,
  ): void {
    this.logInternal(level, category, message, data, false);
  }

  private logAlways(
    level: LogLevel,
    category: string,
    message: string,
    data?: any,
  ): void {
    this.logInternal(level, category, message, data, true);
  }

  private logInternal(
    level: LogLevel,
    category: string,
    message: string,
    data: any,
    alwaysOutput: boolean,
  ): void {
    const event: LogEvent = {
      timestamp: new Date(),
      level,
      category,
      message,
      data,
    };

    // Always emit event for testing
    this.eventEmitter.fire(event);

    // Only write to output channel if level allows or alwaysOutput is true
    if (!alwaysOutput && !this.shouldLog(level)) {
      return;
    }

    // Format for output channel
    const levelStr = `[${category}]`;
    let output = `${levelStr} ${message}`;

    if (data) {
      output += ` ${JSON.stringify(data)}`;
    }

    this.outputChannel.appendLine(output);

    // Also log to console for test output visibility
    console.log(`[${category}] ${message}`, data || "");
  }

  public show(): void {
    this.outputChannel.show();
  }

  public dispose(): void {
    this.outputChannel.dispose();
    this.eventEmitter.dispose();
  }
}
