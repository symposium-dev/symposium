"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.ExtensionConnector = void 0;
/**
 * ExtensionConnector wraps VSCode message passing to look like mynah-ui's Connector pattern.
 * This keeps the webview main.ts code similar to mynah-ui examples while handling
 * the VSCode extension ↔ webview communication internally.
 */
class ExtensionConnector {
    vscode;
    messageListeners = new Map();
    constructor() {
        this.vscode = acquireVsCodeApi();
        // Listen for messages from the extension
        window.addEventListener("message", (event) => {
            const message = event.data;
            this._handleExtensionMessage(message);
        });
    }
    /**
     * Request a generative AI answer - mimics mynah-ui example pattern
     */
    async requestGenerativeAIAnswer(streamingChatItems, onStreamUpdate, onStreamEnd) {
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
    _handleExtensionMessage(message) {
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
                    const shouldStop = listener.onUpdate({ body: listener.streamedContent }, 50);
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
    _cleanupListener(messageId) {
        this.messageListeners.delete(messageId);
    }
}
exports.ExtensionConnector = ExtensionConnector;
//# sourceMappingURL=connector.js.map