import { expect, Page } from 'playwright/test';
import { getSelector, waitForAnimationEnd } from '../helpers';
import testIds from '../../../src/helper/test-ids';

export const renderButtons = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  await page.evaluate(() => {
    const selectedTabId = window.mynahUI.getSelectedTabId();
    if (selectedTabId != null) {
      window.mynahUI.updateStore(selectedTabId, {
        chatItems: [],
      });
      window.mynahUI.addChatItem(selectedTabId, {
        type: 'answer' as any,
        messageId: new Date().getTime().toString(),
        body: 'This is a card with actions inside!',
        buttons: [
          {
            text: 'Action 1',
            id: 'action-1',
            status: 'info',
            icon: 'chat' as any
          },
          {
            text: 'Action 2',
            description: 'This action will not remove the card!',
            id: 'action-2',
            keepCardAfterClick: false,
          },
          {
            text: 'Action 3',
            description: 'This is disabled for some reason!',
            id: 'action-3',
            disabled: true,
          },
          {
            text: 'Primary',
            description: 'This is colored!',
            id: 'action-3',
            status: 'primary',
          },
          {
            text: 'Main',
            description: 'This is more colored!',
            id: 'action-3',
            status: 'main',
          },
          {
            text: 'Clear',
            description: 'This is clear!',
            id: 'action-3',
            status: 'clear',
          },
        ],
      });

      window.mynahUI.addChatItem(selectedTabId, {
        type: 'answer' as any,
        messageId: new Date().getTime().toString(),
        body: 'Do you wish to continue?',
        buttons: [
          {
            id: 'confirmation-buttons-cancel',
            text: 'Cancel',
            status: 'error',
            icon: 'cancel-circle' as any,
            position: 'outside'
          },
          {
            id: 'confirmation-buttons-confirm',
            text: 'Confirm',
            status: 'success',
            icon: 'ok-circled' as any,
            position: 'outside'
          },
        ]
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
    await expect(await locator2.screenshot()).toMatchSnapshot();
  }
};
