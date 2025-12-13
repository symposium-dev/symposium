import { expect, Page } from 'playwright/test';
import { getSelector, waitForAnimationEnd } from '../helpers';
import testIds from '../../../src/helper/test-ids';

export const initRender = async (page: Page): Promise<void> => {
  const welcomeCardSelector = `${getSelector(testIds.chatItem.type.answer)}[messageid="welcome-message"]`;
  const welcomeCard = await page.waitForSelector(welcomeCardSelector);
  await waitForAnimationEnd(page);

  expect(welcomeCard).toBeDefined();

  expect(await page.screenshot()).toMatchSnapshot();
};
