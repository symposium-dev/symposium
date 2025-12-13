import { Page } from 'playwright';
import { getSelector, waitForAnimationEnd } from '../helpers';
import testIds from '../../../src/helper/test-ids';
import { closeTab } from './close-tab';
import { expect } from 'playwright/test';

export const welcomeMode = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  await closeTab(page, false, true);
  const welcomeCardId = 'welcome-screen-card';

  const newTabId = await page.evaluate((cardId) => {
    return window.mynahUI.updateStore('', {
      tabTitle: 'Welcome',
      promptInputPlaceholder: 'Placeholder',
      promptInputInfo: 'Footer',
      chatItems: [
        {
          type: 'answer' as any,
          body: 'Welcome card',
          messageId: cardId,
          icon: 'asterisk' as any,
        }
      ]
    }) as string;
  }, welcomeCardId);

  await page.mouse.move(0, 0);
  const chatWrapperSelector = `${getSelector(testIds.chat.wrapper)}[mynah-tab-id="${String(newTabId)}"]`;
  const chatWrapper = await page.waitForSelector(chatWrapperSelector);
  expect(chatWrapper).toBeDefined();

  const welcomeCardSelector = `${getSelector(testIds.chatItem.type.answer)}[messageid="${welcomeCardId}"]`;
  const welcomeCard = await page.waitForSelector(welcomeCardSelector);
  await waitForAnimationEnd(page);
  expect(welcomeCard).toBeDefined();

  if (skipScreenshots !== true) {
    // snap
    expect(await chatWrapper.screenshot()).toMatchSnapshot();
  }

  await page.evaluate((tabId) => {
    window.mynahUI.updateStore(tabId, {
      tabBackground: true,
      compactMode: true,
      tabHeaderDetails: {
        icon: 'q' as any,
        title: 'Amazon Q Developer',
        description: 'Welcome to Amazon Q Developer'
      },
      promptInputLabel: 'Ask your question'
    });
  }, newTabId);
  await waitForAnimationEnd(page);
  const chatWrapperClasses = await (await page.waitForSelector(chatWrapperSelector)).evaluate(el => Array.from(el.classList));
  expect(chatWrapperClasses).toContain('with-background');

  expect(await page.waitForSelector(getSelector(testIds.prompt.label))).toBeDefined();
  expect(await page.waitForSelector(getSelector(testIds.chat.header))).toBeDefined();
  expect(await page.waitForSelector(getSelector(`${testIds.chat.header}-icon`))).toBeDefined();
  expect(await page.waitForSelector(getSelector(`${testIds.chat.header}-title`))).toBeDefined();
  expect(await page.waitForSelector(getSelector(`${testIds.chat.header}-description`))).toBeDefined();

  if (skipScreenshots !== true) {
    // snap
    expect(await chatWrapper.screenshot()).toMatchSnapshot();
  }
};
