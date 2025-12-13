const mynahUI = new MynahUI({
    tabs: {
        'tab-1': {
            isSelected: true,
            store: {
                tabTitle: 'Chat',
                chatItems: [
                    {
                        type: ChatItemType.ANSWER,
                        body: 'Welcome to our chat!',
                        messageId: 'welcome-message'
                    },
                ],
                promptInputPlaceholder: 'Type your question',
            }
        }
    },
    onChatPrompt: (tabId: string, prompt: ChatPrompt) => {
        mynahUI.addChatItem(tabId, {
            type: ChatItemType.PROMPT,
            messageId: new Date().getTime().toString(),
            body: prompt.escapedPrompt
        });
        // call your genAI action
    }
});

// Sample triple backtick code block text
```
This is a code block
```

// And an inline one
`this is inline`

Backticks inside code blocks are ok!
