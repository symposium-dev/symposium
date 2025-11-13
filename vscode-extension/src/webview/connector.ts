import { ChatItem } from "@aws/mynah-ui";

/**
 * VSCode API interface (available in webview context)
 */
interface VSCodeAPI {
  postMessage(message: any): void;
}

declare const acquireVsCodeApi: () => VSCodeAPI;

/**
 * Message types for webview ↔ extension communication
 */
interface ExtensionMessage {
  type: "streamStart" | "streamChunk" | "streamEnd";
  tabId: string;
  messageId: string;
  content?: string;
}

/**
 * ExtensionConnector wraps VSCode message passing to look like mynah-ui's Connector pattern.
 * This keeps the webview main.ts code similar to mynah-ui examples while handling
 * the VSCode extension ↔ webview communication internally.
 */
export class ExtensionConnector {
  private readonly vscode: VSCodeAPI;
  private messageListeners = new Map<
    string,
    {
      onUpdate: (chatItem: Partial<ChatItem>, progress: number) => boolean;
      onEnd: () => void;
      streamedContent: string;
    }
  >();

  constructor() {
    this.vscode = acquireVsCodeApi();

    // Listen for messages from the extension
    (window as any).addEventListener("message", (event: any) => {
      const message: ExtensionMessage = event.data;
      this._handleExtensionMessage(message);
    });
  }

  /**
   * Request a generative AI answer - mimics mynah-ui example pattern
   */
  async requestGenerativeAIAnswer(
    streamingChatItems: Partial<ChatItem>[],
    onStreamUpdate: (
      chatItem: Partial<ChatItem>,
      progressPercentage: number,
    ) => boolean,
    onStreamEnd: () => void,
  ): Promise<boolean> {
    return new Promise((resolve) => {
      const firstItem = streamingChatItems[0];
      if (!firstItem?.messageId) {
        console.error("No messageId in streaming chat item");
        resolve(false);
        return;
      }

      const messageId = firstItem.messageId;

      // Store the callbacks for this message
      this.messageListeners.set(messageId, {
        onUpdate: onStreamUpdate,
        onEnd: onStreamEnd,
        streamedContent: "",
      });

      // Send prompt to extension
      // Note: We assume the first item contains the prompt info
      // The actual prompt text should be in the chat item
      const tabId = "default"; // For now, single tab
      const prompt = firstItem.body || "";

      this.vscode.postMessage({
        type: "sendPrompt",
        tabId,
        prompt,
        messageId,
      });

      resolve(true);
    });
  }

  private _handleExtensionMessage(message: ExtensionMessage) {
    const { messageId } = message;
    const listener = this.messageListeners.get(messageId);

    if (!listener) {
      console.warn("Received message for unknown messageId:", messageId);
      return;
    }

    switch (message.type) {
      case "streamStart":
        // Initialize - nothing to do yet
        break;

      case "streamChunk":
        if (message.content) {
          listener.streamedContent += message.content;

          // Call the update callback with accumulated content
          const shouldStop = listener.onUpdate(
            { body: listener.streamedContent },
            50, // Progress percentage (we don't track this precisely yet)
          );

          if (shouldStop) {
            // User requested stop - send message to extension?
            // For now, just clean up
            this._cleanupListener(messageId);
          }
        }
        break;

      case "streamEnd":
        // Call end callback and clean up
        listener.onEnd();
        this._cleanupListener(messageId);
        break;
    }
  }

  private _cleanupListener(messageId: string) {
    this.messageListeners.delete(messageId);
  }
}
