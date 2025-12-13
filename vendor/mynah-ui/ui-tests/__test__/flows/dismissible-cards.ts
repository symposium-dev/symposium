import { expect, Page } from 'playwright/test';
import { getSelector, waitForAnimationEnd } from '../helpers';
import testIds from '../../../src/helper/test-ids';
import { ChatItemType } from '../../../dist/main';

export const renderAndDismissCard = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  await page.evaluate((body) => {
    const selectedTabId = window.mynahUI.getSelectedTabId();
    if (selectedTabId != null) {
      window.mynahUI.updateStore(selectedTabId, {
        chatItems: [],
      });
      window.mynahUI.addChatItem(selectedTabId, {
        type: 'answer' as ChatItemType,
        messageId: new Date().getTime().toString(),
        title: 'SAVE THE DATE',
        header: {
          icon: 'calendar',
          iconStatus: 'primary',
          body: '## Soon, a new version will be released!',
        },
        fullWidth: true,
        canBeDismissed: true,
        body: "We're improving the performance, adding new features or making new UX changes every week. Save the date for new updates!.",
      }
      );
    }
  });
  await waitForAnimationEnd(page);

  const answerCardSelector = getSelector(testIds.chatItem.type.answer);
  const locator = page.locator(answerCardSelector).nth(0);
  await locator.scrollIntoViewIfNeeded();
  if (skipScreenshots !== true) {
    expect(await locator.screenshot()).toMatchSnapshot();
  }

  // Click the dismiss button to remove the card
  const dismissButtonSelector = getSelector(testIds.chatItem.dismissButton);
  await page.locator(dismissButtonSelector).nth(0).click();
  await waitForAnimationEnd(page);

  // Verify that the card is now gone
  const cardSelector = getSelector(testIds.chatItem.type.answer);
  const cardCount = await page.locator(cardSelector).count();
  expect(cardCount).toBe(0);
};
