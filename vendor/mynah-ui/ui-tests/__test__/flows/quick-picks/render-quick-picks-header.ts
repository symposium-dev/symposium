import { expect, Page } from 'playwright/test';
import { getSelector, waitForAnimationEnd } from '../../helpers';
import testIds from '../../../../src/helper/test-ids';

export const renderQuickPicksHeader = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  await waitForAnimationEnd(page);

  // Update store to add quickActionCommandsHeader
  await page.evaluate(() => {
    (window as any).mynahUI.updateStore('tab-1', {
      quickActionCommands: [
        {
          groupName: 'Code Generation',
          commands: [
            {
              command: '/dev',
              description: 'Generate code for your task',
              placeholder: 'Describe what you want to build'
            }
          ]
        }
      ],
      quickActionCommandsHeader: {
        status: 'warning',
        icon: 'info',
        title: 'New agentic capabilities',
        description: 'You can now ask Q directly in the chat. You don\'t need to explicitly use /dev, /test, or /doc commands anymore.'
      }
    });
  });

  await waitForAnimationEnd(page);

  // Clear the input
  const input = page.locator(getSelector(testIds.prompt.input));
  await input.clear();
  await waitForAnimationEnd(page);

  // Press '/' in the input to trigger quick picks
  await input.press('/');
  await waitForAnimationEnd(page);

  // Check that the command selector is opened and visible
  const commandSelector = page.locator(getSelector(testIds.prompt.quickPicksWrapper)).nth(-1);
  expect(commandSelector).toBeDefined();
  expect(await commandSelector.isVisible()).toBeTruthy();

  // Check that the header is present and contains expected content
  const headerElement = commandSelector.locator('.mynah-chat-prompt-quick-picks-header').first();
  expect(await headerElement.isVisible()).toBeTruthy();

  // Verify header content
  const headerText = await headerElement.textContent();
  expect(headerText).toContain('New agentic capabilities');
  expect(headerText).toContain('You can now ask Q directly in the chat');

  // Verify header status styling (warning status should add appropriate class)
  const headerClasses = await headerElement.getAttribute('class');
  expect(headerClasses).toContain('status-warning');

  // Verify icon is present
  const headerIcon = headerElement.locator('.mynah-ui-title-description-icon-icon').first();
  expect(await headerIcon.isVisible()).toBeTruthy();

  if (skipScreenshots !== true) {
    expect(await page.screenshot()).toMatchSnapshot();
  }

  // Clean up - close the quick picks by pressing Backspace
  await input.press('Backspace');
  await waitForAnimationEnd(page);

  // Verify quick picks are closed
  expect(await commandSelector.isVisible()).toBeFalsy();
};
