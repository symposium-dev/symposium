### How to start using MynahUI

First things first. Since MynahUI is not dependent on any framework you can use it in any web project. Simply grab it from `npm`.

```console
npm install @aws/mynah-ui
```

Now you have it in your node_modules.

And for the first tab let's create an initial chat bubble to let users explore. And put them into a file named `static.ts`.

```typescript
export const initTabs = {
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
};
```