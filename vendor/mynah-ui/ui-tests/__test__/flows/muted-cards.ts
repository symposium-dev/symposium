import { expect, Page } from 'playwright/test';
import { getSelector, waitForAnimationEnd } from '../helpers';
import testIds from '../../../src/helper/test-ids';
import { ChatItemType } from '../../../dist/main';

export const renderMutedCards = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  await page.evaluate((body) => {
    const selectedTabId = window.mynahUI.getSelectedTabId();
    if (selectedTabId != null) {
      window.mynahUI.updateStore(selectedTabId, {
        chatItems: [],
      });
      window.mynahUI.addChatItem(selectedTabId, {
        type: 'answer' as ChatItemType,
        messageId: new Date().getTime().toString(),
        muted: true,
        body: 'This is an extended card with an icon and a different border color. It also includes some action buttons.',
        status: 'error',
        icon: 'error',
        buttons: [
          {
            text: 'I Understand',
            id: 'understood',
            status: 'error',
            icon: 'ok',
          },
        ],
      });
      window.mynahUI.addChatItem(selectedTabId, {
        type: 'answer' as ChatItemType,
        messageId: new Date().getTime().toString(),
        fullWidth: true,
        padding: false,
        muted: true,
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
    }
  });
  await waitForAnimationEnd(page);

  const answerCardSelector = getSelector(testIds.chatItem.type.answer);
  const locator1 = page.locator(answerCardSelector).nth(0);
  await locator1.scrollIntoViewIfNeeded();
  if (skipScreenshots !== true) {
    expect(await locator1.screenshot()).toMatchSnapshot();
  }

  const locator2 = page.locator(answerCardSelector).nth(1);
  await locator2.scrollIntoViewIfNeeded();
  if (skipScreenshots !== true) {
    expect(await locator2.screenshot()).toMatchSnapshot();
  }
};
