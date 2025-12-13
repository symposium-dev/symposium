import { expect, Page } from 'playwright/test';
import { getSelector, waitForAnimationEnd } from '../../helpers';
import testIds from '../../../../src/helper/test-ids';
import { renderFeedbackForm } from './render-feedback-form';

export const renderDownvoteResult = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  await renderFeedbackForm(page, true);
  await waitForAnimationEnd(page);

  const thumbsDown = page.locator(getSelector(testIds.chatItem.vote.downvoteLabel));
  expect(thumbsDown).toBeDefined();
  await thumbsDown.click();
  await waitForAnimationEnd(page);

  if (skipScreenshots !== true) {
    expect(await page.screenshot()).toMatchSnapshot();
  }
};
