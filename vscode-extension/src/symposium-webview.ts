// This file runs in the webview context (browser environment)
import { MynahUI, ChatItem, ChatItemType } from "@aws/mynah-ui";

// Browser API declarations for webview context
declare const acquireVsCodeApi: any;
declare const window: any & {
  SYMPOSIUM_EXTENSION_ACTIVATION_ID: string;
  SYMPOSIUM_REQUIRE_MODIFIER_TO_SEND: boolean;
  SYMPOSIUM_NO_TABS_IMAGE_URI: string;
};

// Import uuid - note: webpack will bundle this for browser
import { v4 as uuidv4 } from "uuid";

const vscode = acquireVsCodeApi();

let mynahUI: MynahUI;

// Track accumulated agent response per tab (for current text segment)
const tabAgentResponses: { [tabId: string]: string } = {};

// Track current ANSWER_STREAM message ID per tab
const tabCurrentMessageId: { [tabId: string]: string } = {};

// Track what type of content the current stream is: "text" or "tool"
const tabCurrentStreamType: { [tabId: string]: "text" | "tool" } = {};

// Track active tool calls per tab: toolCallId → messageId
const tabToolCalls: { [tabId: string]: { [toolCallId: string]: string } } = {};

// Tool call status type (matches ACP)
type ToolCallStatus = "pending" | "running" | "completed" | "failed";

// Tool call info from extension
interface ToolCallInfo {
  toolCallId: string;
  title: string;
  status: ToolCallStatus;
  kind?: string;
  rawInput?: Record<string, unknown>;
  rawOutput?: Record<string, unknown>;
}

// Slash command info from extension
interface SlashCommandInfo {
  name: string;
  description: string;
  inputHint?: string;
}

// Track which messages we've seen per tab and mynah UI state
interface WebviewState {
  extensionActivationId: string;
  lastSeenIndex: { [tabId: string]: number };
  mynahTabs?: any; // Mynah UI tabs state
}

// Get extension activation ID from window (embedded by extension)
const currentExtensionActivationId = window.SYMPOSIUM_EXTENSION_ACTIVATION_ID;
console.log(`Extension activation ID: ${currentExtensionActivationId}`);

// Load saved state and check if we need to clear it
vscode.postMessage({
  type: "log",
  message: "Getting saved state",
  data: { extensionActivationId: currentExtensionActivationId },
});

const savedState = vscode.getState() as WebviewState | undefined;
let lastSeenIndex: { [tabId: string]: number } = {};
let mynahTabs: any = undefined;

if (
  !savedState ||
  !savedState.extensionActivationId ||
  savedState.extensionActivationId !== currentExtensionActivationId
) {
  if (savedState) {
    vscode.postMessage({
      type: "log",
      message: "Extension activation ID mismatch - clearing state",
      data: {
        savedId: savedState.extensionActivationId,
        currentId: currentExtensionActivationId,
      },
    });
  } else {
    vscode.postMessage({
      type: "log",
      message: "No saved state found - starting fresh",
    });
  }
  // Clear persisted state
  vscode.setState(undefined);
  // Start fresh
  lastSeenIndex = {};
  mynahTabs = undefined;
} else {
  // Keep existing state - extension activation ID matches
  lastSeenIndex = savedState.lastSeenIndex ?? {};
  mynahTabs = savedState.mynahTabs;
  const tabCount = mynahTabs ? Object.keys(mynahTabs).length : 0;
  vscode.postMessage({
    type: "log",
    message: "Restoring state from previous session",
    data: { tabCount, hasLastSeenIndex: Object.keys(lastSeenIndex).length },
  });
}

// Handle approval request button clicks
function handleApprovalResponse(
  tabId: string,
  approvalId: string,
  action: "approve" | "deny" | "bypass-all",
  options: any[],
) {
  let response: any;
  let bypassAll = false;

  if (action === "approve") {
    // Find the "allow_once" option
    const allowOption = options.find((opt: any) => opt.kind === "allow_once");
    if (allowOption) {
      response = {
        outcome: { outcome: "selected", optionId: allowOption.optionId },
      };
    } else {
      response = { outcome: { outcome: "cancelled" } };
    }
  } else if (action === "deny") {
    // Find the "reject_once" option
    const rejectOption = options.find((opt: any) => opt.kind === "reject_once");
    if (rejectOption) {
      response = {
        outcome: { outcome: "selected", optionId: rejectOption.optionId },
      };
    } else {
      response = { outcome: { outcome: "cancelled" } };
    }
  } else if (action === "bypass-all") {
    // Approve this time and enable bypass
    const allowOption = options.find((opt: any) => opt.kind === "allow_once");
    if (allowOption) {
      response = {
        outcome: { outcome: "selected", optionId: allowOption.optionId },
      };
      bypassAll = true;
    } else {
      response = { outcome: { outcome: "cancelled" } };
    }
  }

  // Send response to extension
  vscode.postMessage({
    type: "approval-response",
    approvalId,
    response,
    bypassAll,
  });
}

// Get status icon for tool call
function getToolStatusIcon(status: ToolCallStatus): string {
  switch (status) {
    case "pending":
      return "⏳";
    case "running":
      return "⚙️";
    case "completed":
      return "✓";
    case "failed":
      return "✗";
    default:
      return "•";
  }
}

// Get MynahUI status for tool call
function getToolStatus(
  status: ToolCallStatus,
): "info" | "success" | "warning" | "error" | undefined {
  switch (status) {
    case "completed":
      return "success";
    case "failed":
      return "error";
    case "running":
      return "info";
    default:
      return undefined;
  }
}

// Format tool output for display
function formatToolOutput(
  rawOutput: Record<string, unknown> | undefined,
): string {
  if (!rawOutput) return "";
  try {
    return "```json\n" + JSON.stringify(rawOutput, null, 2) + "\n```";
  } catch {
    return String(rawOutput);
  }
}

// Format tool input for display
function formatToolInput(
  rawInput: Record<string, unknown> | undefined,
): string {
  if (!rawInput) return "";
  try {
    return "```json\n" + JSON.stringify(rawInput, null, 2) + "\n```";
  } catch {
    return String(rawInput);
  }
}

// Build the tool call details content
function buildToolDetails(toolCall: ToolCallInfo): string {
  let details = "";
  if (toolCall.rawInput) {
    details += "**Input:**\n" + formatToolInput(toolCall.rawInput) + "\n\n";
  }
  if (toolCall.rawOutput) {
    details += "**Output:**\n" + formatToolOutput(toolCall.rawOutput);
  }
  return details;
}

// Handle tool call from extension
function handleToolCall(tabId: string, toolCall: ToolCallInfo) {
  // Initialize tool tracking for this tab if needed
  if (!tabToolCalls[tabId]) {
    tabToolCalls[tabId] = {};
  }

  // Check if we already have a card for this tool call
  const existingMessageId = tabToolCalls[tabId][toolCall.toolCallId];
  if (existingMessageId) {
    // Update existing card via messageId (works even if it's not the last card)
    updateToolCallCard(tabId, existingMessageId, toolCall);
    return;
  }

  // Create new ANSWER_STREAM card for this tool call
  // This automatically ends any previous stream (text or tool)
  const messageId = `tool-${toolCall.toolCallId}`;
  tabToolCalls[tabId][toolCall.toolCallId] = messageId;
  tabCurrentMessageId[tabId] = messageId;
  tabCurrentStreamType[tabId] = "tool";

  const icon = getToolStatusIcon(toolCall.status);
  const details = buildToolDetails(toolCall);
  const displayTitle = toolCall.title.replace(/`/g, "'");

  // Use ANSWER_STREAM so this becomes the :last-child and spinner works
  mynahUI.addChatItem(tabId, {
    type: ChatItemType.ANSWER_STREAM,
    messageId,
    status: getToolStatus(toolCall.status),
    body: `${icon} \`${displayTitle}\``,
    summary: details
      ? {
          isCollapsed: true,
          content: {
            body: details,
          },
        }
      : undefined,
  });
}

// Update an existing tool call card
function updateToolCallCard(
  tabId: string,
  messageId: string,
  toolCall: ToolCallInfo,
) {
  const icon = getToolStatusIcon(toolCall.status);
  const shimmer =
    toolCall.status === "running" || toolCall.status === "pending";
  const details = buildToolDetails(toolCall);

  // Replace backticks with single quotes for display
  const displayTitle = toolCall.title.replace(/`/g, "'");

  mynahUI.updateChatAnswerWithMessageId(tabId, messageId, {
    status: getToolStatus(toolCall.status),
    shimmer,
    body: `${icon} \`${displayTitle}\``,
    summary: details
      ? {
          isCollapsed: true,
          content: {
            body: details,
          },
        }
      : undefined,
  });
}

// Handle approval request from extension
function handleApprovalRequest(message: any) {
  const { tabId, approvalId, toolCall, options } = message;

  // Log what we received for debugging
  console.log("Approval request received:", { toolCall, options });

  // Extract tool information with fallbacks
  const toolName = toolCall.title || toolCall.toolCallId || "Unknown Tool";

  // Format tool parameters for display
  let paramsDisplay = "";
  if (toolCall.rawInput && typeof toolCall.rawInput === "object") {
    paramsDisplay =
      "```json\n" + JSON.stringify(toolCall.rawInput, null, 2) + "\n```";
  }

  // Create approval card
  const messageId = `approval-${approvalId}`;
  mynahUI.addChatItem(tabId, {
    type: ChatItemType.ANSWER,
    messageId,
    body: `### Tool Permission Request\n\n**Tool:** \`${toolName}\`\n\n${paramsDisplay ? "**Parameters:**\n" + paramsDisplay : ""}`,
    buttons: [
      {
        id: "approve",
        text: "Approve",
        status: "success",
        keepCardAfterClick: false,
      },
      {
        id: "deny",
        text: "Deny",
        status: "error",
        keepCardAfterClick: false,
      },
      {
        id: "bypass-all",
        text: "Bypass Permissions",
        status: "warning",
        keepCardAfterClick: false,
      },
    ],
  });

  // Store approval context for button handler
  (window as any)[`approval_${messageId}`] = { approvalId, options };
}

const config: any = {
  rootSelector: "#mynah-root",
  loadStyles: true,
  config: {
    texts: {
      mainTitle: "Symposium",
      noTabsOpen: "### Join the symposium by opening a tab",
      spinnerText: "Discussing with the Symposium...",
    },
    // Custom image for the "no tabs open" screen (uses actual img element, not CSS mask)
    noTabsImage: window.SYMPOSIUM_NO_TABS_IMAGE_URI,
    noTabsImageOpacity: 0.6,
    // When true, Enter adds newline and Shift/Cmd+Enter sends
    // When false (default), Enter sends and Shift+Enter adds newline
    requireModifierToSendPrompt: window.SYMPOSIUM_REQUIRE_MODIFIER_TO_SEND,
  },
  defaults: {
    store: {
      tabTitle: "Symposium",
    },
  },
  onInBodyButtonClicked: (tabId: string, messageId: string, action: any) => {
    // Check if this is an approval button
    const approvalContext = (window as any)[`approval_${messageId}`];
    if (approvalContext) {
      handleApprovalResponse(
        tabId,
        approvalContext.approvalId,
        action.id,
        approvalContext.options,
      );
      // Clean up context
      delete (window as any)[`approval_${messageId}`];
    }
  },
  onTabAdd: (tabId: string) => {
    // Notify extension that a new tab was created
    console.log("New tab created:", tabId);
    vscode.postMessage({
      type: "new-tab",
      tabId: tabId,
    });
    // Save state when tab is added
    saveState();
  },
  onTabRemove: (tabId: string) => {
    // Save state when tab is closed
    console.log("Tab removed:", tabId);
    saveState();
  },
  onContextSelected: (contextItem: any, tabId: string) => {
    // User selected a file from the @ context menu
    // The command field contains the file path
    console.log("Context selected:", contextItem.command, "for tab:", tabId);

    // Return true to let MynahUI insert the command text into the prompt
    // The file path will be inserted as-is (user can type more after)
    return true;
  },
  onChatPrompt: (tabId: string, prompt: any) => {
    console.log("onChatPrompt received:", JSON.stringify(prompt, null, 2));

    // Build the full prompt text including command if present
    let promptText = prompt.prompt || "";
    if (prompt.command) {
      promptText = prompt.command + (promptText ? " " + promptText : "");
    }

    // Extract context (file, symbol, and selection references from @ mentions)
    // context can be string[] or QuickActionCommand[]
    const contextFiles: string[] = [];
    const contextSymbols: Array<{
      name: string;
      location: string;
      range: {
        startLine: number;
        startChar: number;
        endLine: number;
        endChar: number;
      };
    }> = [];
    const contextSelections: Array<{
      filePath: string;
      relativePath: string;
      startLine: number;
      endLine: number;
      text: string;
    }> = [];

    if (prompt.context && Array.isArray(prompt.context)) {
      for (const item of prompt.context) {
        if (typeof item === "string") {
          // Plain string - treat as file path
          contextFiles.push(item);
        } else if (item.id) {
          // Has id field - check if it's an encoded reference
          try {
            const decoded = JSON.parse(atob(item.id));
            if (decoded.type === "symbol") {
              contextSymbols.push({
                name: decoded.name,
                location: decoded.location,
                range: decoded.range,
              });
            } else if (decoded.type === "selection") {
              contextSelections.push({
                filePath: decoded.filePath,
                relativePath: decoded.relativePath,
                startLine: decoded.startLine,
                endLine: decoded.endLine,
                text: decoded.text,
              });
            } else {
              // Unknown type, treat command as file
              if (item.command) contextFiles.push(item.command);
            }
          } catch {
            // Not valid base64/JSON, treat command as file
            if (item.command && !item.command.startsWith("#")) {
              contextFiles.push(item.command);
            }
          }
        } else if (item.command && !item.command.startsWith("#")) {
          // No id, command doesn't start with # - treat as file path
          contextFiles.push(item.command);
        }
      }
    }

    console.log("Sending prompt text:", promptText);
    if (contextFiles.length > 0) {
      console.log("With context files:", contextFiles);
    }
    if (contextSymbols.length > 0) {
      console.log("With context symbols:", contextSymbols);
    }
    if (contextSelections.length > 0) {
      console.log("With context selections:", contextSelections);
    }

    // Send prompt to extension with tabId and context
    vscode.postMessage({
      type: "prompt",
      tabId: tabId,
      prompt: promptText,
      contextFiles: contextFiles.length > 0 ? contextFiles : undefined,
      contextSymbols: contextSymbols.length > 0 ? contextSymbols : undefined,
      contextSelections:
        contextSelections.length > 0 ? contextSelections : undefined,
    });

    // Show loading/thinking indicator
    mynahUI.updateStore(tabId, {
      loadingChat: true,
    });

    // Add the user's prompt to the chat
    mynahUI.addChatItem(tabId, {
      type: ChatItemType.PROMPT,
      body: promptText,
    });

    // Initialize empty response for this tab
    tabAgentResponses[tabId] = "";

    // Generate message ID for MynahUI tracking
    const messageId = uuidv4();
    tabCurrentMessageId[tabId] = messageId;
    tabCurrentStreamType[tabId] = "text";

    // Add placeholder for the streaming answer
    mynahUI.addChatItem(tabId, {
      type: ChatItemType.ANSWER_STREAM,
      messageId: messageId,
      body: "",
    });

    // Save state when prompt is sent
    saveState();
  },
};

// If we have saved tabs, initialize with them
if (mynahTabs) {
  config.tabs = mynahTabs;
  console.log("Initializing MynahUI with restored tabs");
}

mynahUI = new MynahUI(config);
console.log("MynahUI initialized");

// Tell extension we're ready to receive messages
vscode.postMessage({ type: "webview-ready" });

// Save state helper
function saveState() {
  // Sync current prompt input text to store for each tab before saving
  // This captures any text the user is currently typing
  const allTabs = mynahUI?.getAllTabs();
  if (allTabs) {
    for (const tabId of Object.keys(allTabs)) {
      const currentPromptText = mynahUI.getPromptInputText(tabId);
      if (currentPromptText) {
        // Update the store with the current prompt input text
        mynahUI.updateStore(tabId, { promptInputText: currentPromptText });
      }
    }
  }

  // Get current tabs from mynah UI (now includes synced prompt input text)
  const currentTabs = mynahUI?.getAllTabs();

  const state: WebviewState = {
    extensionActivationId: currentExtensionActivationId,
    lastSeenIndex,
    mynahTabs: currentTabs,
  };
  vscode.setState(state);

  vscode.postMessage({
    type: "log",
    message: "Saved state",
    data: {
      extensionActivationId: currentExtensionActivationId,
      tabCount: currentTabs ? Object.keys(currentTabs).length : 0,
      lastSeenIndexCount: Object.keys(lastSeenIndex).length,
    },
  });
}

// Handle messages from the extension
window.addEventListener("message", (event: MessageEvent) => {
  const message = event.data;
  const receiveTime = Date.now();

  // Handle request/response messages (not indexed)
  if (message.type === "get-selected-tab") {
    const selectedTabId = mynahUI.getSelectedTabId();
    vscode.postMessage({
      type: "selected-tab-response",
      requestId: message.requestId,
      tabId: selectedTabId,
    });
    return;
  }

  if (message.type === "create-tab") {
    // updateStore with empty string tabId creates a new tab and returns the ID
    const newTabId = mynahUI.updateStore("", {});
    console.log("Created new tab:", newTabId);
    vscode.postMessage({
      type: "selected-tab-response",
      requestId: message.requestId,
      tabId: newTabId,
    });
    return;
  }

  // Check if we've already seen this message
  const currentLastSeen = lastSeenIndex[message.tabId] ?? -1;
  if (message.index <= currentLastSeen) {
    console.log(
      `Ignoring duplicate message ${message.index} for tab ${message.tabId}`,
    );
    return;
  }

  // Process the message
  if (message.type === "agent-text") {
    const extensionDelay = message.timestamp
      ? receiveTime - message.timestamp
      : "unknown";
    console.log(
      `[PERF] Webview received chunk at ${receiveTime}, extension->webview delay=${extensionDelay}ms, length=${message.text.length}`,
    );

    // Check if we need to start a new text stream
    // (either no current stream, or current stream is a tool card)
    if (
      !tabCurrentMessageId[message.tabId] ||
      tabCurrentStreamType[message.tabId] !== "text"
    ) {
      // Start a new text stream - this ends any previous stream (tool card)
      const newMessageId = uuidv4();
      tabCurrentMessageId[message.tabId] = newMessageId;
      tabCurrentStreamType[message.tabId] = "text";
      tabAgentResponses[message.tabId] = ""; // Reset accumulated text for new segment

      mynahUI.addChatItem(message.tabId, {
        type: ChatItemType.ANSWER_STREAM,
        messageId: newMessageId,
        body: "",
      });
    }

    // Append text to accumulated response
    const appendStart = Date.now();
    tabAgentResponses[message.tabId] =
      (tabAgentResponses[message.tabId] || "") + message.text;
    const appendEnd = Date.now();

    // Update the chat UI with accumulated text
    const uiUpdateStart = Date.now();
    mynahUI.updateLastChatAnswer(message.tabId, {
      body: tabAgentResponses[message.tabId],
    });
    const uiUpdateEnd = Date.now();

    console.log(
      `[PERF] Append: ${appendEnd - appendStart}ms, UI update: ${uiUpdateEnd - uiUpdateStart}ms, total: ${uiUpdateEnd - receiveTime}ms`,
    );
  } else if (message.type === "agent-complete") {
    // Hide loading/thinking indicator
    mynahUI.updateStore(message.tabId, {
      loadingChat: false,
    });

    // Mark the stream as complete using the message ID
    const messageId = tabCurrentMessageId[message.tabId];
    if (messageId) {
      mynahUI.endMessageStream(message.tabId, messageId);
    }

    // Clear accumulated response, message ID, and stream type
    delete tabAgentResponses[message.tabId];
    delete tabCurrentMessageId[message.tabId];
    delete tabCurrentStreamType[message.tabId];
  } else if (message.type === "set-tab-title") {
    // Update the tab title
    mynahUI.updateStore(message.tabId, {
      tabTitle: message.title,
    });
  } else if (message.type === "approval-request") {
    // Display approval request UI
    handleApprovalRequest(message);
  } else if (message.type === "tool-call") {
    // Handle tool call notification
    handleToolCall(message.tabId, message.toolCall);
  } else if (message.type === "tool-call-update") {
    // Handle tool call update
    handleToolCall(message.tabId, message.toolCall);
  } else if (message.type === "available-commands") {
    // Convert ACP commands to MynahUI quickActionCommands format
    const commands = message.commands as SlashCommandInfo[];
    const quickActionCommands = [
      {
        commands: commands.map((cmd) => ({
          command: `/${cmd.name}`,
          description: cmd.description,
          placeholder: cmd.inputHint,
        })),
      },
    ];

    console.log("Setting quick action commands:", quickActionCommands);

    // Update the tab store with the commands
    mynahUI.updateStore(message.tabId, {
      quickActionCommands,
    });
  } else if (message.type === "available-context") {
    // Convert file list and symbols to MynahUI contextCommands format
    const files = message.files as string[];
    const symbols = (message.symbols || []) as Array<{
      name: string;
      kind: number;
      location: string;
      containerName?: string;
      range: {
        startLine: number;
        startChar: number;
        endLine: number;
        endChar: number;
      };
    }>;
    const selection = message.selection as {
      filePath: string;
      relativePath: string;
      startLine: number;
      endLine: number;
      text: string;
    } | null;

    // Symbol kind names (subset of vscode.SymbolKind)
    const symbolKindNames: Record<number, string> = {
      4: "class",
      5: "method",
      6: "property",
      8: "field",
      11: "function",
      12: "variable",
      13: "constant",
      22: "struct",
      23: "enum",
      10: "interface",
      24: "type",
    };

    // Build context command groups
    const contextCommands = [];

    // Selection group (shown first when available)
    if (selection) {
      const lineRange =
        selection.startLine === selection.endLine
          ? `L${selection.startLine}`
          : `L${selection.startLine}-${selection.endLine}`;
      // Encode selection info for resolution
      const selectionRef = JSON.stringify({
        type: "selection",
        filePath: selection.filePath,
        relativePath: selection.relativePath,
        startLine: selection.startLine,
        endLine: selection.endLine,
        text: selection.text,
      });
      contextCommands.push({
        groupName: "Selection",
        commands: [
          {
            command: "Current Selection",
            description: `${selection.relativePath}:${lineRange}`,
            id: btoa(selectionRef),
          },
        ],
      });
    }

    // Files group
    if (files.length > 0) {
      contextCommands.push({
        groupName: "Files",
        commands: files.map((filePath) => ({
          command: filePath,
          description: filePath,
        })),
      });
    }

    // Symbols group
    if (symbols.length > 0) {
      contextCommands.push({
        groupName: "Symbols",
        commands: symbols.map((sym) => {
          const kindName = symbolKindNames[sym.kind] || "symbol";
          const displayName = sym.containerName
            ? `${sym.containerName}::${sym.name}`
            : sym.name;
          // Encode symbol info as JSON for resolution
          const symbolRef = JSON.stringify({
            type: "symbol",
            name: sym.name,
            location: sym.location,
            range: sym.range,
          });
          return {
            // Use # prefix to distinguish symbols from files
            command: `#${sym.name}`,
            label: displayName,
            description: `${kindName} in ${sym.location}`,
            // Store full info for resolution (base64 to avoid special chars)
            id: btoa(symbolRef),
          };
        }),
      });
    }

    console.log(
      `Setting context commands: ${files.length} files, ${symbols.length} symbols, selection: ${selection !== null}`,
    );

    // Update the tab store with the context commands
    mynahUI.updateStore(message.tabId, {
      contextCommands,
    });
  } else if (message.type === "add-context-to-prompt") {
    // Add a frozen selection as context to the prompt input
    const selection = message.selection as {
      filePath: string;
      relativePath: string;
      startLine: number;
      endLine: number;
      text: string;
    };

    // Encode selection info the same way we do for "Current Selection"
    const lineRange =
      selection.startLine === selection.endLine
        ? `L${selection.startLine}`
        : `L${selection.startLine}-${selection.endLine}`;

    const selectionRef = JSON.stringify({
      type: "selection",
      filePath: selection.filePath,
      relativePath: selection.relativePath,
      startLine: selection.startLine,
      endLine: selection.endLine,
      text: selection.text,
    });

    const contextItem = {
      command: `Selection from ${selection.relativePath}`,
      description: `${selection.relativePath}:${lineRange}`,
      id: btoa(selectionRef),
    };

    console.log("Adding custom context to prompt:", contextItem);
    mynahUI.addCustomContextToPrompt(message.tabId, [contextItem]);
  } else if (message.type === "agent-error") {
    // Display error message and stop loading
    mynahUI.updateStore(message.tabId, {
      loadingChat: false,
    });

    // Add error card to chat - try to pretty print if it contains JSON
    let errorBody = message.error;
    const jsonMatch = errorBody.match(/\{[\s\S]*\}/);
    if (jsonMatch) {
      try {
        const parsed = JSON.parse(jsonMatch[0]);
        const prefix = errorBody.slice(0, jsonMatch.index).trim();
        errorBody = prefix
          ? `${prefix}\n\n\`\`\`json\n${JSON.stringify(parsed, null, 2)}\n\`\`\``
          : `\`\`\`json\n${JSON.stringify(parsed, null, 2)}\n\`\`\``;
      } catch {
        errorBody = `\`\`\`\n${errorBody}\n\`\`\``;
      }
    } else {
      errorBody = `\`\`\`\n${errorBody}\n\`\`\``;
    }

    mynahUI.addChatItem(message.tabId, {
      type: ChatItemType.ANSWER,
      body: `### Error\n\n${errorBody}`,
      status: "error",
    });

    // Clear any pending response state
    delete tabAgentResponses[message.tabId];
    delete tabCurrentMessageId[message.tabId];
    delete tabCurrentStreamType[message.tabId];
  }

  // Update lastSeenIndex and save state
  lastSeenIndex[message.tabId] = message.index;
  saveState();

  // Send acknowledgment
  vscode.postMessage({
    type: "message-ack",
    tabId: message.tabId,
    index: message.index,
  });
});
