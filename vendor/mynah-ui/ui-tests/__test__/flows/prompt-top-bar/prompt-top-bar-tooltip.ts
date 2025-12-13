import { expect, Page } from 'playwright/test';
import { getSelector, justWait, waitForAnimationEnd } from '../../helpers';
import testIds from '../../../../src/helper/test-ids';

export const promptTopBarTooltip = async (page: Page): Promise<void> => {
  const promptTopBarSelector = getSelector(testIds.prompt.topBar);

  // Set up the prompt top bar with a title and context items
  await page.evaluate(() => {
    const selectedTabId = window.mynahUI.getSelectedTabId();
    if (selectedTabId != null) {
      window.mynahUI.updateStore(selectedTabId, {
        promptTopBarTitle: 'Context Items',
        promptTopBarContextItems: [
          {
            id: 'context-1',
            command: '@workspace',
            icon: 'folder',
            description: 'Use workspace context for better answers'
          }
        ]
      });
    }
  });

  // Wait for the top bar to be visible
  await page.waitForSelector(promptTopBarSelector);
  await waitForAnimationEnd(page);

  // Hover over the context pill to show the tooltip
  const contextPill = page.locator(getSelector(testIds.prompt.topBarContextPill)).first();
  await contextPill.hover();

  // Wait for tooltip to be displayed
  await justWait(1000);

  // Check if tooltip is visible
  const tooltip = page.locator(getSelector(testIds.prompt.topBarContextTooltip));
  expect(await tooltip.isVisible()).toBeTruthy();

  // Take a screenshot of the tooltip
  expect(await tooltip.screenshot()).toMatchSnapshot();

  // Move mouse away to hide tooltip
  await page.mouse.move(0, 0);
  await waitForAnimationEnd(page);

  // Verify tooltip is hidden
  expect(await page.locator(getSelector(testIds.prompt.topBarContextTooltip)).count()).toBe(0);
};
