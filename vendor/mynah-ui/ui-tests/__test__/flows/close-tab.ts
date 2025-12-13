import { expect, Page } from 'playwright/test';
import testIds from '../../../src/helper/test-ids';
import { getSelector } from '../helpers';

export const closeTab = async (page: Page, withDblClick?: boolean, skipScreenshots?: boolean): Promise<void> => {
  const firstTab = page.locator(`${getSelector(testIds.tabBar.tabOptionWrapper)}:nth-child(1)`);
  expect(firstTab).toBeDefined();

  if (withDblClick !== true) {
    await page.locator(getSelector(testIds.tabBar.tabOptionCloseButton)).click();
  } else {
    await page.locator(getSelector(testIds.tabBar.tabOptionLabel)).click({ position: { x: 10, y: 10 }, button: 'middle' });
  }
  await page.mouse.move(0, 0);

  if (skipScreenshots !== true) {
    // No tabs snap
    expect(await page.screenshot()).toMatchSnapshot();
  }
};
