import { expect, Page } from 'playwright/test';
import { getSelector, waitForAnimationEnd } from '../helpers';
import testIds from '../../../src/helper/test-ids';

export const renderInformationCard = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  await page.evaluate(() => {
    const selectedTabId = window.mynahUI.getSelectedTabId();
    if (selectedTabId != null) {
      window.mynahUI.updateStore(selectedTabId, {
        chatItems: [],
      });

      window.mynahUI.addChatItem(selectedTabId, {
        messageId: new Date().getTime().toString(),
        type: 'answer' as any,
        informationCard: {
          title: 'Information card',
          description: 'With a description below the title and success status.',
          icon: 'bug' as any,
          content: {
            body: 'This is a list below with some code and bolds inside\n- **Bold** Text with some `inline code`.\n- Also with some code blocks ```const a = 5;```\n\nEnd of the list.\nList with numbers.\n1. **Item1** This is the first list item.\n2. **Item2** This is the second list item.\n3. **Item3** This is the third list item.\n4. **Item4** This is the fourth list item. And it also has a [LINK](#) inside.',
          },
          status: {
            status: 'success',
            icon: 'thumbs-up' as any,
            body: 'Successfully completed this task!'
          }
        },
      });
    }
  });
  await waitForAnimationEnd(page);

  const answerCardSelector = getSelector(testIds.chatItem.type.answer);
  const answerCard = await page.waitForSelector(answerCardSelector);
  await answerCard.scrollIntoViewIfNeeded();

  if (skipScreenshots !== true) {
    expect(await page.screenshot()).toMatchSnapshot();
  }
};
