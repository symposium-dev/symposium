import { expect, Page } from 'playwright/test';
import { getSelector, justWait, waitForAnimationEnd } from '../../helpers';
import testIds from '../../../../src/helper/test-ids';

export const navigatePromptsDown = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  await page.locator(getSelector(testIds.prompt.input)).fill('This is the first user prompt');
  await page.locator(getSelector(testIds.prompt.send)).nth(1).click();
  await waitForAnimationEnd(page);

  await page.locator(getSelector(testIds.prompt.input)).fill('This is the second user prompt');
  await page.locator(getSelector(testIds.prompt.send)).nth(1).click();
  await waitForAnimationEnd(page);

  const promptInput = page.locator(getSelector(testIds.prompt.input));
  await promptInput.press('ArrowUp');
  await waitForAnimationEnd(page);

  await promptInput.press('ArrowUp');
  await justWait(100);

  await promptInput.press('ArrowDown');
  await justWait(100);

  await promptInput.press('ArrowDown');
  await justWait(100);

  expect(await promptInput.innerText()).toBe('This is the second user prompt');

  if (skipScreenshots !== true) {
    expect(await promptInput.screenshot()).toMatchSnapshot();
  }
};
