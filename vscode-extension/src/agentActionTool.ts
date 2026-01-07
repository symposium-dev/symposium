/**
 * Agent Action Tool
 *
 * This tool is invoked when an ACP agent requests permission to execute an
 * internal tool. VS Code's confirmation UI handles user approval, and the
 * result is communicated back through the message history.
 */

import * as vscode from "vscode";

/**
 * Input schema for the symposium-agent-action tool.
 * Matches the ACP RequestPermissionRequest.tool_call fields.
 *
 * Note: toolCallId is NOT part of the input - VS Code provides it
 * separately through the LanguageModelToolCallPart structure.
 */
export interface AgentActionInput {
  title?: string;
  kind?: string;
  // Raw input varies by tool - we don't know its structure
  raw_input?: Record<string, unknown>;
}

/**
 * Tool implementation for agent action permission requests.
 *
 * When the agent wants to execute an internal tool (like bash or file edit),
 * we emit a LanguageModelToolCallPart for this tool. VS Code shows its
 * confirmation UI, and when approved, invoke() is called.
 */
export class AgentActionTool
  implements vscode.LanguageModelTool<AgentActionInput>
{
  /**
   * Prepare the invocation - customize the confirmation UI.
   */
  async prepareInvocation(
    options: vscode.LanguageModelToolInvocationPrepareOptions<AgentActionInput>,
    _token: vscode.CancellationToken,
  ): Promise<vscode.PreparedToolInvocation> {
    const { title, kind, raw_input } = options.input;

    // Build a descriptive message for the confirmation dialog
    const actionTitle = title || kind || "execute an action";

    // Some tools (e.g., Claude Code's bash) include a description field
    const description =
      raw_input?.description && typeof raw_input.description === "string"
        ? raw_input.description
        : undefined;

    // Build markdown message with optional description
    let messageText = `Allow the agent to **${actionTitle}**?`;
    if (description) {
      messageText += `\n\n${description}`;
    }

    return {
      invocationMessage: `Executing: ${actionTitle}`,
      confirmationMessages: {
        title: "Agent Action",
        message: new vscode.MarkdownString(messageText),
      },
    };
  }

  /**
   * Invoke the tool - called after user approves.
   *
   * The actual tool execution happens on the agent side. We return
   * a simple confirmation message so VS Code doesn't show an error.
   */
  async invoke(
    _options: vscode.LanguageModelToolInvocationOptions<AgentActionInput>,
    _token: vscode.CancellationToken,
  ): Promise<vscode.LanguageModelToolResult> {
    return new vscode.LanguageModelToolResult([
      new vscode.LanguageModelTextPart("Approved"),
    ]);
  }
}
