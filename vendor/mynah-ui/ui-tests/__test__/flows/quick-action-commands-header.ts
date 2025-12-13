import { expect, Page } from 'playwright/test';
import { getSelector, waitForAnimationEnd } from '../helpers';
import testIds from '../../../src/helper/test-ids';

export const renderQuickActionCommandsHeader = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  await waitForAnimationEnd(page);

  // Clear the input
  const input = page.locator(getSelector(testIds.prompt.input));
  await input.clear();
  await waitForAnimationEnd(page);

  // Press '/' to trigger quick action commands
  await input.press('/');
  await waitForAnimationEnd(page);

  // Check that the quick picks wrapper is visible
  const quickPicksWrapper = page.locator(getSelector(testIds.prompt.quickPicksWrapper)).nth(-1);
  expect(quickPicksWrapper).toBeDefined();
  expect(await quickPicksWrapper.isVisible()).toBeTruthy();

  // Check that the quick action commands header is present (it's rendered in an overlay)
  const headerElement = page.locator('.mynah-chat-prompt-quick-picks-header').first();
  expect(headerElement).toBeDefined();
  expect(await headerElement.isVisible()).toBeTruthy();

  // Verify header contains expected elements
  const headerIcon = headerElement.locator('.mynah-ui-icon').first();
  const headerTitle = headerElement.locator('[data-testid$="-title"]').first();
  const headerDescription = headerElement.locator('[data-testid$="-description"]').first();

  expect(await headerIcon.isVisible()).toBeTruthy();
  expect(await headerTitle.isVisible()).toBeTruthy();
  expect(await headerDescription.isVisible()).toBeTruthy();

  // Verify header content
  const titleText = await headerTitle.textContent();
  const descriptionText = await headerDescription.textContent();

  expect(titleText).toContain('Q Developer agentic capabilities');
  expect(descriptionText).toContain('You can now ask Q directly in the chat');

  // Check for status-specific styling (warning status in the example)
  const hasWarningStatus = await headerElement.evaluate((el) =>
    el.classList.contains('status-warning')
  );
  expect(hasWarningStatus).toBeTruthy();

  if (skipScreenshots !== true) {
    expect(await page.screenshot()).toMatchSnapshot('quick-action-commands-header.png');
  }

  // Clean up - press Backspace to close the quick picks
  await input.press('Backspace');
  await waitForAnimationEnd(page);
};

export const verifyQuickActionCommandsHeaderInteraction = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  await waitForAnimationEnd(page);

  // Clear the input
  const input = page.locator(getSelector(testIds.prompt.input));
  await input.clear();
  await waitForAnimationEnd(page);

  // Press '/' to trigger quick action commands
  await input.press('/');
  await waitForAnimationEnd(page);

  const headerElement = page.locator('.mynah-chat-prompt-quick-picks-header').first();

  // Hover over the header to check for any hover effects
  await headerElement.hover();
  await waitForAnimationEnd(page);

  if (skipScreenshots !== true) {
    expect(await page.screenshot()).toMatchSnapshot('quick-action-commands-header-hover.png');
  }

  // Verify the header remains visible during interaction
  expect(await headerElement.isVisible()).toBeTruthy();

  // Clean up
  await input.press('Backspace');
  await waitForAnimationEnd(page);
};

export const verifyQuickActionCommandsHeaderWithoutData = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  // This test would verify behavior when quickActionCommandsHeader is null or undefined
  // For now, we'll simulate a scenario where the header should not be displayed

  await waitForAnimationEnd(page);

  // Clear the input
  const input = page.locator(getSelector(testIds.prompt.input));
  await input.clear();
  await waitForAnimationEnd(page);

  // Press '@' to trigger context commands (which typically don't have the header)
  await input.press('@');
  await waitForAnimationEnd(page);

  const quickPicksWrapper = page.locator(getSelector(testIds.prompt.quickPicksWrapper)).nth(-1);
  expect(await quickPicksWrapper.isVisible()).toBeTruthy();

  // Verify that the quick action commands header is NOT present for context commands
  const headerElements = page.locator('.mynah-chat-prompt-quick-picks-header');
  const headerCount = await headerElements.count();
  expect(headerCount).toBe(0);

  if (skipScreenshots !== true) {
    expect(await page.screenshot()).toMatchSnapshot('quick-action-commands-header-not-present.png');
  }

  // Clean up
  await input.press('Backspace');
  await waitForAnimationEnd(page);
};

export const verifyQuickActionCommandsHeaderStatusVariations = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  // This test would verify different status variations (warning, error, success, etc.)
  // Since we can't easily change the status in the test, we'll verify the current status styling

  await waitForAnimationEnd(page);

  const input = page.locator(getSelector(testIds.prompt.input));
  await input.clear();
  await waitForAnimationEnd(page);

  await input.press('/');
  await waitForAnimationEnd(page);

  const headerElement = page.locator('.mynah-chat-prompt-quick-picks-header').first();

  // Check for various possible status classes
  const statusClasses = [ 'status-warning', 'status-error', 'status-success', 'status-info', 'status-default' ];
  let foundStatusClass = false;

  for (const statusClass of statusClasses) {
    const hasStatus = await headerElement.evaluate((el, className) =>
      el.classList.contains(className), statusClass
    );
    if (hasStatus) {
      foundStatusClass = true;
      console.log(`Found status class: ${statusClass}`);
      break;
    }
  }

  expect(foundStatusClass).toBeTruthy();

  if (skipScreenshots !== true) {
    expect(await page.screenshot()).toMatchSnapshot('quick-action-commands-header-status.png');
  }

  // Clean up
  await input.press('Backspace');
  await waitForAnimationEnd(page);
};
