import { expect, Page } from 'playwright/test';
import { getSelector, justWait, waitForAnimationEnd } from '../../helpers';
import testIds from '../../../../src/helper/test-ids';

export const renderPromptTopBar = async (page: Page): Promise<void> => {
  const promptTopBarSelector = getSelector(testIds.prompt.topBar);

  // Set up the prompt top bar with a title
  await page.evaluate(() => {
    const selectedTabId = window.mynahUI.getSelectedTabId();
    if (selectedTabId != null) {
      window.mynahUI.updateStore(selectedTabId, {
        promptTopBarTitle: 'Test Title',
      });
    }
  });

  // Wait for the top bar to be visible
  const promptTopBarWrapper = await page.waitForSelector(promptTopBarSelector);
  expect(await promptTopBarWrapper.isVisible()).toBeTruthy();
  await waitForAnimationEnd(page);

  // Take a screenshot of the top bar with just a title
  expect(await promptTopBarWrapper.screenshot()).toMatchSnapshot();

  // Add context items to the top bar
  await page.evaluate(() => {
    const selectedTabId = window.mynahUI.getSelectedTabId();
    if (selectedTabId != null) {
      window.mynahUI.updateStore(selectedTabId, {
        promptTopBarContextItems: [
          {
            id: 'context-1',
            command: '@context1',
            icon: 'file',
            description: 'First context item'
          },
          {
            id: 'context-2',
            command: '@context2',
            icon: 'folder',
            description: 'Second context item'
          }
        ]
      });
    }
  });

  await waitForAnimationEnd(page);

  // Take a screenshot of the top bar with title and context items
  expect(await promptTopBarWrapper.screenshot()).toMatchSnapshot();

  // Add a top bar button
  await page.evaluate(() => {
    const selectedTabId = window.mynahUI.getSelectedTabId();
    if (selectedTabId != null) {
      window.mynahUI.updateStore(selectedTabId, {
        promptTopBarButton: {
          id: 'top-bar-button',
          text: 'Actions',
          icon: 'menu'
        }
      });
    }
  });

  await waitForAnimationEnd(page);

  // Take a screenshot of the complete top bar
  expect(await promptTopBarWrapper.screenshot()).toMatchSnapshot();

  // Test removing a context item by clicking on it
  const firstContextPill = page.locator(getSelector(testIds.prompt.topBarContextPill)).first();
  await firstContextPill.hover();
  await waitForAnimationEnd(page);

  // Take a screenshot of the hover state
  expect(await firstContextPill.screenshot()).toMatchSnapshot();

  // Click to remove the context item
  await firstContextPill.click();
  await waitForAnimationEnd(page);

  // Verify only one context item remains
  expect(await page.locator(getSelector(testIds.prompt.topBarContextPill)).count()).toBe(1);

  // Test overflow behavior by adding more context items
  await page.evaluate(() => {
    const selectedTabId = window.mynahUI.getSelectedTabId();
    if (selectedTabId != null) {
      window.mynahUI.updateStore(selectedTabId, {
        promptTopBarContextItems: [
          {
            id: 'context-2',
            command: 'context2',
            icon: 'magic',
            description: 'Second context item'
          },
          {
            id: 'context-3',
            command: 'context3',
            icon: 'file',
            description: 'Third context item'
          },
          {
            id: 'context-4',
            command: 'context4',
            icon: 'folder',
            description: 'Fourth context item'
          },
          {
            id: 'context-5',
            command: 'context5',
            icon: 'file',
            description: 'Fifth context item'
          },
          {
            id: 'context-6',
            command: 'context6',
            icon: 'file',
            description: 'Sixth context item'
          }
        ]
      });
    }
  });

  await waitForAnimationEnd(page);

  // Check if overflow button appears
  const overflowButton = page.locator(getSelector(testIds.prompt.topBarOverflowPill));
  expect(await overflowButton.isVisible()).toBeTruthy();

  // Take a screenshot with overflow button
  expect(await promptTopBarWrapper.screenshot()).toMatchSnapshot();

  // Click the overflow button to show the overlay
  await overflowButton.click();
  await waitForAnimationEnd(page);

  // Wait for overlay to appear
  await justWait(1000);

  // Take a screenshot of the overflow overlay
  const overflowOverlay = page.locator(getSelector(testIds.prompt.topBarOverflowOverlay));
  expect(await overflowOverlay.screenshot()).toMatchSnapshot();

  // Click outside to close the overlay
  await page.mouse.click(10, 10);
  await waitForAnimationEnd(page);

  // Test the top bar button click
  const topBarButton = page.locator(getSelector(testIds.prompt.topBarButton));
  await topBarButton.click();
  await waitForAnimationEnd(page);

  // Test hiding the top bar by clearing the title
  await page.evaluate(() => {
    const selectedTabId = window.mynahUI.getSelectedTabId();
    if (selectedTabId != null) {
      window.mynahUI.updateStore(selectedTabId, {
        promptTopBarTitle: '',
      });
    }
  });

  await waitForAnimationEnd(page);

  // Verify the top bar is hidden
  expect(await page.locator(getSelector(testIds.prompt.topBar) + '.hidden').count()).toBe(1);
};
