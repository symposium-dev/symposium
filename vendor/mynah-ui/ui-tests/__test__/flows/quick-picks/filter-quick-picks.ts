import { expect, Page } from 'playwright/test';
import { getSelector, waitForAnimationEnd } from '../../helpers';
import testIds from '../../../../src/helper/test-ids';

export const filterQuickPicks = async (page: Page, mode?: 'command' | 'context', skipScreenshots?: boolean): Promise<void> => {
  // Clear the input
  const input = page.locator(getSelector(testIds.prompt.input));
  await input.clear();
  await input.press('Backspace');
  await waitForAnimationEnd(page);

  // Type a '/' character, followed by a first character
  await input.press(mode === 'context' ? '@' : '/');
  await waitForAnimationEnd(page);
  await input.press(mode === 'context' ? 'w' : 'h');
  await waitForAnimationEnd(page);

  // Check that the command selector is opened, and visible
  const commandSelector = page.locator(getSelector(testIds.prompt.quickPicksWrapper)).nth(-1);
  expect(commandSelector).toBeDefined();
  expect(await commandSelector.isVisible()).toBeTruthy();

  // Check that there is only one suggestion
  const quickPickItem = page.locator(getSelector(testIds.prompt.quickPickItem));
  expect(await quickPickItem.count()).toBe(1);

  // Check that the suggestions are what we expect from the first character
  const innerTexts = (await quickPickItem.allInnerTexts());
  expect(innerTexts[0]).toContain(mode === 'context' ? '@workspace' : '/help');
  expect(innerTexts[0]).not.toContain(mode === 'context' ? '@file' : '/clear');

  if (skipScreenshots !== true) {
    expect(await page.screenshot()).toMatchSnapshot();
  }
};
