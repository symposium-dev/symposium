// This file runs in the webview context (browser environment)
import { MynahUI, ChatItem, ChatItemType } from "@aws/mynah-ui";

// Browser API declarations for webview context
declare const acquireVsCodeApi: any;
declare const window: any;

const vscode = acquireVsCodeApi();

let messageIdCounter = 0;
let mynahUI: MynahUI;

// Request saved state from extension
vscode.postMessage({ type: "request-saved-state" });

// Function to initialize mynah-ui with optional saved state
function initializeMynahUI(savedTabs?: any) {
  const config: any = {
    rootSelector: "#mynah-root",
    loadStyles: true,
    config: {
      texts: {
        mainTitle: "Symposium",
        noTabsOpen: "### Join the symposium by opening a tab",
      },
    },
    defaults: {
      store: {
        tabTitle: "Symposium",
      },
    },
    onChatPrompt: (tabId: string, prompt: any) => {
      // User sent a prompt - send it to the extension
      const messageId = `msg-${messageIdCounter++}`;

      vscode.postMessage({
        type: "prompt",
        messageId: messageId,
        prompt: prompt.prompt,
      });

      // Add the user's prompt to the chat
      mynahUI.addChatItem(tabId, {
        type: ChatItemType.PROMPT,
        body: prompt.prompt,
      });

      // Add a placeholder for the streaming answer
      mynahUI.addChatItem(tabId, {
        type: ChatItemType.ANSWER_STREAM,
        messageId: messageId,
        body: "",
      });
    },
  };

  // If we have saved tabs, initialize with them
  if (savedTabs) {
    config.tabs = savedTabs;
    console.log("Restoring mynah-ui with saved tabs:", savedTabs);
  }

  mynahUI = new MynahUI(config);
  console.log("MynahUI initialized:", mynahUI);
}

// Handle messages from the extension
window.addEventListener("message", (event: MessageEvent) => {
  const message = event.data;

  if (message.type === "restore-state") {
    // Initialize mynah-ui with saved state (or undefined if no saved state)
    initializeMynahUI(message.state);
    return;
  }

  // Handle streaming messages
  if (!mynahUI) {
    console.warn("MynahUI not initialized yet, ignoring message:", message);
    return;
  }

  // Find the active tab
  const tabs = mynahUI.getAllTabs();
  const tabId = Object.keys(tabs).find((id) => tabs[id].isSelected);

  if (!tabId) {
    return;
  }

  if (message.type === "response-chunk") {
    // Update the streaming answer with the new chunk
    mynahUI.updateChatAnswerWithMessageId(tabId, message.messageId, {
      body: message.chunk,
    });
  } else if (message.type === "response-complete") {
    // Mark the stream as complete
    mynahUI.endMessageStream(tabId, message.messageId);

    // Save state after each completed response
    const state = mynahUI.getAllTabs();
    console.log("Auto-saving state after response:", state);
    vscode.postMessage({
      type: "save-state",
      state: state,
    });
  }
});
