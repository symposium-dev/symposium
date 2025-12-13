import { expect, Page } from 'playwright/test';
import { getSelector, waitForAnimationEnd } from '../helpers';
import testIds from '../../../src/helper/test-ids';

export const hoverOverLink = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  const mockSource =
    {
      url: 'https://github.com/aws/mynah-ui',
      title: 'Mock Source 1',
      body: `## Perque adacto fugio

Invectae moribundo et eripiet sine, adventu tolli *liquidas* satiatur Perseus;
**locus**, nato! More dei timeas dextra Granico neu corpus simul *operique*!
Fecit mea, sua, hoc vias proles pallebant illa est populosque festa manetque
clamato nescisse.`,
    };

  await page.evaluate((source) => {
    const selectedTabId = window.mynahUI.getSelectedTabId();
    if (selectedTabId != null) {
      window.mynahUI.updateStore(selectedTabId, {
        chatItems: [],
      });

      window.mynahUI.addChatItem(selectedTabId, {
        type: 'answer' as any,
        body: 'Text',
        relatedContent: {
          content: [ source ],
          title: 'Sources',
        },
      });
    }
  }, mockSource);
  await waitForAnimationEnd(page);

  if (skipScreenshots !== true) {
    expect(await page.screenshot()).toMatchSnapshot();
  }

  const linkWrapperLocator = page.locator(getSelector(testIds.chatItem.relatedLinks.linkWrapper));
  await linkWrapperLocator.dispatchEvent('mouseenter', { bubbles: true, cancelable: true });

  await waitForAnimationEnd(page);

  if (skipScreenshots !== true) {
    expect(await page.screenshot()).toMatchSnapshot();
  }
  expect(await page.locator(getSelector(testIds.chatItem.relatedLinks.linkPreviewOverlay)).count()).toEqual(1);
  expect(await page.locator(getSelector(testIds.chatItem.relatedLinks.linkPreviewOverlayCard)).count()).toEqual(1);

  await linkWrapperLocator.first().dispatchEvent('mouseleave');
  await waitForAnimationEnd(page);

  if (skipScreenshots !== true) {
    expect(await page.screenshot()).toMatchSnapshot();
  }
  expect(await page.locator(getSelector(testIds.chatItem.relatedLinks.linkPreviewOverlay)).count()).toEqual(0);
  expect(await page.locator(getSelector(testIds.chatItem.relatedLinks.linkPreviewOverlayCard)).count()).toEqual(0);
};
