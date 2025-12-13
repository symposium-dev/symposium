import { expect, Page } from 'playwright/test';
import { getSelector, waitForAnimationEnd } from '../../helpers';
import testIds from '../../../../src/helper/test-ids';

export const renderFeedbackForm = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  await page.evaluate(() => {
    const selectedTabId = window.mynahUI.getSelectedTabId();
    if (selectedTabId != null) {
      window.mynahUI.updateStore(selectedTabId, {
        chatItems: [],
      });

      window.mynahUI.addChatItem(selectedTabId, {
        type: 'answer' as any,
        snapToTop: true,
        body: 'This message is votable.',
        canBeVoted: true,
      });
    }
  });

  if (skipScreenshots !== true) {
    const thumbsDown = page.locator(getSelector(testIds.chatItem.vote.downvoteLabel));
    expect(thumbsDown).toBeDefined();
    await thumbsDown.click();
    await waitForAnimationEnd(page);

    const reportButton = page.locator(getSelector(testIds.chatItem.vote.reportButton));
    expect(reportButton).toBeDefined();
    await reportButton.click();
    await waitForAnimationEnd(page);

    const commentInput = page.locator(getSelector(testIds.feedbackForm.comment));
    expect(commentInput).toBeDefined();
    await commentInput.fill('This is some feedback comment');
    await waitForAnimationEnd(page);

    const submitButton = page.locator(getSelector(testIds.feedbackForm.submitButton));
    expect(submitButton).toBeDefined();

    const cancelButton = page.locator(getSelector(testIds.feedbackForm.cancelButton));
    expect(cancelButton).toBeDefined();

    expect(await page.screenshot()).toMatchSnapshot();
  }
};
