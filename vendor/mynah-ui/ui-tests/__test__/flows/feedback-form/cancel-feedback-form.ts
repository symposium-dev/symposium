import { expect, Page } from 'playwright/test';
import { getSelector, waitForAnimationEnd } from '../../helpers';
import testIds from '../../../../src/helper/test-ids';
import { renderDownvoteResult } from './render-downvote-result';

export const cancelFeedbackForm = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  await renderDownvoteResult(page, true);
  await waitForAnimationEnd(page);

  const reportButton = page.locator(getSelector(testIds.chatItem.vote.reportButton));
  expect(reportButton).toBeDefined();
  await reportButton.click();
  await waitForAnimationEnd(page);

  const cancelButton = page.locator(getSelector(testIds.feedbackForm.cancelButton));
  expect(cancelButton).toBeDefined();
  await cancelButton.click();
  await waitForAnimationEnd(page);

  if (skipScreenshots !== true) {
    expect(await page.screenshot()).toMatchSnapshot();
  }
};
