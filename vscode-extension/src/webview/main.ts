import { MynahUI, ChatItem, ChatItemType, ChatPrompt } from '@aws/mynah-ui';
import { ExtensionConnector } from './connector';

// Create the connector
const connector = new ExtensionConnector();

// Create MynahUI instance
const mynahUI = new MynahUI({
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
    onChatPrompt: (tabId: string, prompt: ChatPrompt, eventId?: string) => {
        handlePrompt(tabId, prompt);
    }
});

function handlePrompt(tabId: string, prompt: ChatPrompt) {
    // Add user message to chat
    const userMessageId = `user-${Date.now()}`;
    mynahUI.addChatItem(tabId, {
        type: ChatItemType.PROMPT,
        messageId: userMessageId,
        body: prompt.prompt || ''
    });

    // Add streaming answer placeholder
    const assistantMessageId = `assistant-${Date.now()}`;
    mynahUI.addChatItem(tabId, {
        type: ChatItemType.ANSWER_STREAM,
        messageId: assistantMessageId,
        body: ''
    });

    // Request answer from extension via connector
    connector.requestGenerativeAIAnswer(
        [{
            messageId: assistantMessageId,
            body: prompt.prompt || ''
        }],
        (chatItem: Partial<ChatItem>, progress: number) => {
            // Update the streaming answer
            mynahUI.updateChatAnswerWithMessageId(tabId, assistantMessageId, {
                body: chatItem.body || ''
            });
            return false; // Don't stop streaming
        },
        () => {
            // Stream ended - finalize the answer
            mynahUI.endMessageStream(tabId, assistantMessageId);
        }
    );
}

// Log that we're ready
console.log('Symposium webview initialized');
