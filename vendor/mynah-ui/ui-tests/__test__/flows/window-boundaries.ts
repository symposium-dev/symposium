import { expect, Page } from 'playwright/test';
import testIds from '../../../src/helper/test-ids';
import { DEFAULT_VIEWPORT, getOffsetHeight, getSelector, justWait } from '../helpers';
import { clickToFollowup } from './click-followup';

export const checkContentInsideWindowBoundaries = async (page: Page): Promise<void> => {
  await page.mouse.move(0, 0);
  const chatItemsContainer = await page.waitForSelector(getSelector(testIds.chat.chatItemsContainer));
  const footerPanel = await page.waitForSelector(getSelector(testIds.prompt.footerInfo));
  expect(footerPanel).toBeDefined();
  expect(getOffsetHeight(await footerPanel.boundingBox())).toBeLessThanOrEqual(page.viewportSize()?.height ?? 0);

  // Add content to create a scroll area
  await clickToFollowup(page, true);

  await justWait(500);
  chatItemsContainer.evaluate(elm => {
    elm.scrollTop = 1000000;
  });

  // Check if the footer element exceeds from bottom
  expect(getOffsetHeight(await footerPanel.boundingBox())).toBeLessThanOrEqual(page.viewportSize()?.height ?? 0);

  // Snap
  expect(await page.screenshot()).toMatchSnapshot();

  // Scroll to top to the init message
  await (await page.waitForSelector(`${getSelector(testIds.chatItem.type.answer)}[messageid="welcome-message"]`)).scrollIntoViewIfNeeded();

  // Check if the footer element exceeds from bottom
  expect(getOffsetHeight(await footerPanel.boundingBox())).toBeLessThanOrEqual(page.viewportSize()?.height ?? 0);

  // Update viewport size
  await page.setViewportSize({
    width: 350,
    height: 500
  });

  await justWait(100);

  // Check if the footer element exceeds from bottom
  expect(getOffsetHeight(await footerPanel.boundingBox())).toBeLessThanOrEqual(page.viewportSize()?.height ?? 0);

  // Snap
  expect(await page.screenshot()).toMatchSnapshot();

  // Set viewport size to
  await page.setViewportSize({
    width: 1,
    height: 1
  });
  // We don't need to wait here, we're just checking if the viewport width is changed or not
  expect(page.viewportSize()?.width).toBeLessThanOrEqual(1);

  // Revert viewport size
  await page.setViewportSize(DEFAULT_VIEWPORT);

  await justWait(100);

  // Check if the footer element exceeds from bottom
  expect(getOffsetHeight(await footerPanel.boundingBox())).toBeLessThanOrEqual(page.viewportSize()?.height ?? 0);

  await justWait(500);
  await chatItemsContainer.evaluate(node => { node.scrollTop = 0; });

  await justWait(100);
  // Snap
  expect(await page.screenshot()).toMatchSnapshot();
};
