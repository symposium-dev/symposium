"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
const mynah_ui_1 = require("@aws/mynah-ui");
const connector_1 = require("./connector");
// Create the connector
const connector = new connector_1.ExtensionConnector();
// Create MynahUI instance
const mynahUI = new mynah_ui_1.MynahUI({
    rootSelector: '#mynah-root',
    defaults: {
        store: {
            tabTitle: 'Symposium Chat',
            quickActionCommands: [],
            promptInputPlaceholder: 'Ask Symposium...',
        }
    },
    tabs: {
        'default': {
            isSelected: true,
            store: {
                tabTitle: 'Chat',
                chatItems: []
            }
        }
    },
    config: {
        maxTabs: 1,
    },
    onChatPrompt: (tabId, prompt, eventId) => {
        handlePrompt(tabId, prompt);
    }
});
function handlePrompt(tabId, prompt) {
    // Add user message to chat
    const userMessageId = `user-${Date.now()}`;
    mynahUI.addChatItem(tabId, {
        type: mynah_ui_1.ChatItemType.PROMPT,
        messageId: userMessageId,
        body: prompt.prompt || ''
    });
    // Add streaming answer placeholder
    const assistantMessageId = `assistant-${Date.now()}`;
    mynahUI.addChatItem(tabId, {
        type: mynah_ui_1.ChatItemType.ANSWER_STREAM,
        messageId: assistantMessageId,
        body: ''
    });
    // Request answer from extension via connector
    connector.requestGenerativeAIAnswer([{
            messageId: assistantMessageId,
            body: prompt.prompt || ''
        }], (chatItem, progress) => {
        // Update the streaming answer
        mynahUI.updateChatAnswerWithMessageId(tabId, assistantMessageId, {
            body: chatItem.body || ''
        });
        return false; // Don't stop streaming
    }, () => {
        // Stream ended - finalize the answer
        mynahUI.endMessageStream(tabId, assistantMessageId);
    });
}
// Log that we're ready
console.log('Symposium webview initialized');
//# sourceMappingURL=main.js.map