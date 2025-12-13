### Refactoring process on `index.ts`

Changes will be applied _between line 42 and 70_.

```diff-typescript
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
-                promptInputPlaceholder: 'Write your question',
+                promptInputPlaceholder: 'Type your question',
            }
        }
    },
-    onChatPrompt: () => {},
+    onChatPrompt: (tabId: string, prompt: ChatPrompt) => {
+        mynahUI.addChatItem(tabId, {
+            type: ChatItemType.PROMPT,
+            messageId: new Date().getTime().toString(),
+            body: prompt.escapedPrompt
+        });
+        // call your genAI action
+    }
});
```