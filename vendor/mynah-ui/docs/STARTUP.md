# How to install and start using Mynah UI
Simply install it from npm with your favorite package manager.
```console
npm install @aws/mynah-ui
```

When you import `@aws/mynah-ui` it provides you the main `MynahUI` class to generate a new all-in-one object to create and render the UI inside a desired DOM element. While creating the UI, you can provide the initial and default datas to be shown. You can also connect to the user interaction events through the initial properties. Additionally you can also configure the texts depending on your language preferences.

``` typescript
import { MynahUI } from '@aws/mynah-ui';

// Assign it to a variable to be able call functions.
const mynahUI = new MynahUI({
    // All props are optional
    // so even without providing anything
    // it will create the UI
    rootSelector: ...,
    defaults: ...,
    tabs: ...,
    config: ...,
    onShowMoreWebResultsClick: ...,
    onReady: ...,
    onVote: ...,
    onStopChatResponse: ...,
    onResetStore: ...,
    onChatPrompt: ...,
    onFollowUpClicked: ...,
    onBodyActionClicked: ...,
    onTabChange: ...,
    onTabAdd: ...,
    onContextSelected: ...,
    onTabRemove: ...,
    onChatItemEngagement: ...,
    onCopyCodeToClipboard: ...,
    onCodeInsertToCursorPosition: ...,
    onSourceLinkClick: ...,
    onLinkClick: ...,
    onInfoLinkClick: ...,
    onSendFeedback: ...,
    onOpenDiff: ...,
});
```

You're ready to go, now you have a Chat UI generated and rendered into place you set.

# What's Next?
- Take a look to the **[Constructor Properties](./PROPERTIES.md)**
- Take a look to the **[Configuration](./CONFIG.md)**
- Take a look to the **[How to use MynahUI](./USAGE.md)**
- Take a look to the **[Data Model](./DATAMODEL.md)**
- Take a look to the **[Styling Configuration](./STYLING.md)**

