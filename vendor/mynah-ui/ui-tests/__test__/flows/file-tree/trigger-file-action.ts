
import { Page } from 'playwright';
import testIds from '../../../../src/helper/test-ids';
import { showFileTree } from './show-file-tree';
import { getSelector, waitForAnimationEnd } from '../../helpers';
import { expect } from 'playwright/test';

export const triggerFileActions = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  await showFileTree(page, true);

  // Click on a file to trigger default action
  const fileLocator = page.locator(getSelector(testIds.chatItem.fileTree.file));
  expect(await fileLocator.count()).toEqual(4);
  await fileLocator.nth(1).click();
  await page.mouse.move(0, 0);
  await waitForAnimationEnd(page);

  if (skipScreenshots !== true) {
    expect(await page.screenshot()).toMatchSnapshot();
  }

  // Hover over a file to show sub actions
  await fileLocator.nth(1).hover();
  const fileActionLocator = page.locator(getSelector(testIds.chatItem.fileTree.fileAction));
  expect(await fileActionLocator.count()).toEqual(2);
  await waitForAnimationEnd(page);

  if (skipScreenshots !== true) {
    expect(await page.screenshot()).toMatchSnapshot();
  }

  // Click on a file action button to trigger sub action
  await fileActionLocator.nth(1).click();
  await page.mouse.move(0, 0);
  await waitForAnimationEnd(page);

  if (skipScreenshots !== true) {
    expect(await page.screenshot()).toMatchSnapshot();
  }
};
