import { expect, Page } from 'playwright/test';
import { getSelector, waitForAnimationEnd } from '../helpers';
import testIds from '../../../src/helper/test-ids';

export const renderTabbedCard = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  await page.evaluate(() => {
    const selectedTabId = window.mynahUI.getSelectedTabId();
    if (selectedTabId != null) {
      window.mynahUI.updateStore(selectedTabId, {
        chatItems: [],
      });

      window.mynahUI.addChatItem(selectedTabId, {
        messageId: new Date().getTime().toString(),
        type: 'answer' as any,
        body: '### Feature Development\nGenerate code across files with a task description.',
        buttons: [
          {
            id: 'quick-start',
            text: 'Quick start with \'**/dev**\'',
            icon: 'right-open' as any
          }
        ],
        tabbedContent: [
          {
            value: 'overview',
            label: 'Overview',
            icon: 'comment' as any,
            selected: true,
            content: {
              body: 'Overview content'
            }
          },
          {
            value: 'examples',
            label: 'Examples',
            icon: 'play' as any,
            selected: false,
            content: {
              body: 'Examples content'
            }
          }
        ]
      }
      );
    }
  });
  await waitForAnimationEnd(page);

  const answerCardSelector = getSelector(testIds.chatItem.type.answer);
  const answerCard = await page.waitForSelector(answerCardSelector);
  await answerCard.scrollIntoViewIfNeeded();

  if (skipScreenshots !== true) {
    expect(await page.screenshot()).toMatchSnapshot();
  }

  // Change selected item, check for content change
  const locator = page.locator(`${getSelector(`${testIds.chatItem.tabbedCard.tabs}-option-wrapper`)}`).last();
  await locator.click();
  await waitForAnimationEnd(page);

  if (skipScreenshots !== true) {
    expect(await page.screenshot()).toMatchSnapshot();
  }
};
