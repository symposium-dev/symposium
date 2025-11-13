import { MynahUI, ChatItem, ChatItemType, ChatPrompt } from "@aws/mynah-ui";
import { ExtensionConnector } from "./connector";

console.log("[Webview] Starting initialization");

// Check if root element exists
const rootElement = document.getElementById("mynah-root");
console.log("[Webview] Root element:", rootElement);
if (!rootElement) {
  console.error("[Webview] ERROR: #mynah-root element not found!");
  document.body.innerHTML =
    '<div style="padding: 20px; color: red;">ERROR: Root element not found!</div>';
}

// Create the connector
const connector = new ExtensionConnector();
console.log("[Webview] Connector created");

// Create MynahUI instance
console.log("[Webview] Creating MynahUI");
let mynahUI: MynahUI;
try {
  mynahUI = new MynahUI({
    rootSelector: "#mynah-root",
    loadStyles: true,
    defaults: {
      store: {
        tabTitle: "Symposium Chat",
        quickActionCommands: [],
        promptInputPlaceholder: "Ask Symposium...",
      },
    },
    tabs: {
      default: {
        isSelected: true,
        store: {
          tabTitle: "Chat",
          chatItems: [],
        },
      },
    },
    config: {
      maxTabs: 1,
    },
    onChatPrompt: (tabId: string, prompt: ChatPrompt, eventId?: string) => {
      handlePrompt(tabId, prompt);
    },
  });
  console.log("[Webview] MynahUI instance created successfully:", mynahUI);
} catch (error) {
  console.error("[Webview] ERROR creating MynahUI:", error);
  document.body.innerHTML = `<div style="padding: 20px; color: red;">ERROR: ${error}</div>`;
  throw error;
}

function handlePrompt(tabId: string, prompt: ChatPrompt) {
  console.log("[Webview] handlePrompt called:", { tabId, prompt });

  // Add user message to chat
  const userMessageId = `user-${Date.now()}`;
  mynahUI.addChatItem(tabId, {
    type: ChatItemType.PROMPT,
    messageId: userMessageId,
    body: prompt.prompt || "",
  });
  console.log("[Webview] Added user message:", userMessageId);

  // Add streaming answer placeholder
  const assistantMessageId = `assistant-${Date.now()}`;
  mynahUI.addChatItem(tabId, {
    type: ChatItemType.ANSWER_STREAM,
    messageId: assistantMessageId,
    body: "",
  });
  console.log(
    "[Webview] Added assistant message placeholder:",
    assistantMessageId,
  );

  // Request answer from extension via connector
  console.log("[Webview] Requesting answer from connector");
  connector.requestGenerativeAIAnswer(
    [
      {
        messageId: assistantMessageId,
        body: prompt.prompt || "",
      },
    ],
    (chatItem: Partial<ChatItem>, progress: number) => {
      // Update the streaming answer
      mynahUI.updateChatAnswerWithMessageId(tabId, assistantMessageId, {
        body: chatItem.body || "",
      });
      return false; // Don't stop streaming
    },
    () => {
      // Stream ended - finalize the answer
      mynahUI.endMessageStream(tabId, assistantMessageId);
    },
  );
}

// Log that we're ready
console.log("[Webview] Symposium webview initialized successfully");
