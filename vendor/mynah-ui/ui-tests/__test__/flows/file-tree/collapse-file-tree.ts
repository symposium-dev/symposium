
import { Page } from 'playwright';
import testIds from '../../../../src/helper/test-ids';
import { showFileTree } from './show-file-tree';
import { getSelector, waitForAnimationEnd } from '../../helpers';
import { expect } from 'playwright/test';

export const collapseExpandFileTree = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  await showFileTree(page, true);

  const fileWrapperLocator = page.locator(getSelector(testIds.chatItem.fileTree.wrapper));
  const folderLocator = page.locator(getSelector(testIds.chatItem.fileTree.folder));
  await waitForAnimationEnd(page);
  expect(await folderLocator.count()).toEqual(3);

  // Collapse a nested folder
  await folderLocator.nth(1).click();
  await page.mouse.move(0, 0);
  await waitForAnimationEnd(page);

  if (skipScreenshots !== true) {
    expect(await fileWrapperLocator.screenshot()).toMatchSnapshot();
  }

  // Collapse the outermost folder
  await folderLocator.nth(0).click();
  await page.mouse.move(0, 0);
  await waitForAnimationEnd(page);

  if (skipScreenshots !== true) {
    expect(await fileWrapperLocator.screenshot()).toMatchSnapshot();
  }

  // Expand the outermost folder
  await folderLocator.nth(0).click();
  await page.mouse.move(0, 0);
  await waitForAnimationEnd(page);

  if (skipScreenshots !== true) {
    expect(await fileWrapperLocator.screenshot()).toMatchSnapshot();
  }
};
