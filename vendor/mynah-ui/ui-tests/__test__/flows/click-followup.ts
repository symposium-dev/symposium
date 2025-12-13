import { expect, Page } from 'playwright/test';
import { getSelector, justWait, waitForAnimationEnd } from '../helpers';
import testIds from '../../../src/helper/test-ids';

export const clickToFollowup = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  const followupMessageSelector = `${getSelector(testIds.chatItem.type.answer)}[messageid="mynah-ui-test-followup"]`;
  await page.waitForSelector(followupMessageSelector);
  await waitForAnimationEnd(page);

  await page.locator(`${getSelector(testIds.chatItem.chatItemFollowup.optionButton)}:nth-child(1)`).click();
  await page.mouse.move(0, 0);

  await waitForAnimationEnd(page);
  const userCard = await page.waitForSelector(getSelector(testIds.chatItem.type.prompt));
  expect(userCard).toBeDefined();
  await waitForAnimationEnd(page);
  await userCard.scrollIntoViewIfNeeded();
  await justWait(50);

  if (skipScreenshots !== true) {
    expect(await userCard.screenshot()).toMatchSnapshot();
  }
};
