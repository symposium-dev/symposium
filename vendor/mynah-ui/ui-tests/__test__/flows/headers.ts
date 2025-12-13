import { expect, Page } from 'playwright/test';
import { getSelector, waitForAnimationEnd } from '../helpers';
import testIds from '../../../src/helper/test-ids';
import { ChatItemType } from '../../../dist/main';

export const renderHeaders = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  await page.evaluate((body) => {
    const selectedTabId = window.mynahUI.getSelectedTabId();
    if (selectedTabId != null) {
      window.mynahUI.updateStore(selectedTabId, {
        chatItems: [],
      });
      window.mynahUI.addChatItem(selectedTabId, {
        type: 'answer' as ChatItemType,
        messageId: new Date().getTime().toString(),
        fullWidth: true,
        padding: false,
        header: {
          icon: 'code-block',
          status: {
            icon: 'ok',
            text: 'Accepted',
            status: 'success',
          },
          fileList: {
            hideFileCount: true,
            fileTreeTitle: '',
            filePaths: [ 'package.json' ],
            details: {
              'package.json': {
                icon: null,
                label: 'Created',
                changes: {
                  added: 36,
                  deleted: 0,
                  total: 36,
                },
              },
            },
          },
        },
      });
      window.mynahUI.addChatItem(selectedTabId, {
        type: 'answer' as ChatItemType,
        messageId: new Date().getTime().toString(),
        fullWidth: true,
        padding: false,
        header: {
          icon: 'code-block',
          buttons: [
            {
              icon: 'cancel',
              status: 'clear',
              id: 'reject-file-change-on-header-card',
            },
            {
              icon: 'ok',
              status: 'clear',
              id: 'accept-file-change-on-header-card',
            },
          ],
          fileList: {
            hideFileCount: true,
            fileTreeTitle: '',
            filePaths: [ 'package.json' ],
            details: {
              'package.json': {
                icon: null,
                label: 'Created',
                changes: {
                  added: 36,
                  deleted: 0,
                  total: 36,
                },
              },
            },
          },
        },
        body: `
\`\`\`diff-typescript
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
\`\`\`
    `,
        codeBlockActions: {
          copy: null,
          'insert-to-cursor': null,
        },
      });

      window.mynahUI.addChatItem(selectedTabId, {
        type: 'answer' as ChatItemType,
        messageId: new Date().getTime().toString(),
        fullWidth: true,
        padding: false,
        header: {
          icon: 'code-block',
          body: 'Terminal command',
          status: {
            icon: 'warning',
            status: 'warning',
            description: 'This command may cause\nsignificant data loss or damage.',
          },
          buttons: [
            {
              status: 'clear',
              icon: 'play',
              text: 'Run',
              id: 'run-bash-command',
            },
          ],
        },
        body: `
\`\`\`bash
mkdir -p src/ lalalaaaa
\`\`\`
`,
        codeBlockActions: { copy: null, 'insert-to-cursor': null },
      });
    }
  });
  await waitForAnimationEnd(page);

  const answerCardSelector = getSelector(testIds.chatItem.type.answer);
  const locator1 = page.locator(answerCardSelector).nth(0);
  await locator1.scrollIntoViewIfNeeded();
  if (skipScreenshots !== true) {
    expect(await locator1.screenshot()).toMatchSnapshot();
  }

  const locator2 = page.locator(answerCardSelector).nth(2);
  await locator2.scrollIntoViewIfNeeded();
  if (skipScreenshots !== true) {
    expect(await locator2.screenshot()).toMatchSnapshot();
  }

  const locator3 = page.locator(answerCardSelector).nth(4);
  await locator3.scrollIntoViewIfNeeded();
  if (skipScreenshots !== true) {
    expect(await locator3.screenshot()).toMatchSnapshot();
  }
};
