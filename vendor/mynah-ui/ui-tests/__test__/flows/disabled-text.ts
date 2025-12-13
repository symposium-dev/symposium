import { expect, Page } from 'playwright/test';
import { getSelector, waitForAnimationEnd } from '../helpers';
import testIds from '../../../src/helper/test-ids';

export const renderDisabledText = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  await waitForAnimationEnd(page);

  // Modify existing context commands to add disabled text
  await page.evaluate(() => {
    const tabData = window.mynahUI.getTabData('tab-1');
    const contextCommands = tabData.getStore()?.contextCommands;

    if (contextCommands !== undefined) {
      // Modify the @workspace command to add disabledText
      const updatedCommands = contextCommands.map(group => {
        const updatedGroup = { ...group };
        if (group.commands !== undefined) {
          updatedGroup.commands = group.commands.map(cmd =>
            cmd.command === '@workspace'
              ? { ...cmd, disabledText: 'pending', disabled: true }
              : cmd
          );
        }
        return updatedGroup;
      });

      window.mynahUI.updateStore('tab-1', {
        contextCommands: updatedCommands
      });
    }
  });

  await waitForAnimationEnd(page);

  // Clear the input and open context selector
  const input = page.locator(getSelector(testIds.prompt.input));
  await input.clear();
  await input.press('@');
  await waitForAnimationEnd(page);

  // Wait for the context selector to be visible
  const contextSelector = page.locator(getSelector(testIds.prompt.quickPicksWrapper)).nth(-1);
  await expect(contextSelector).toBeVisible();

  if (skipScreenshots !== true) {
    expect(await page.screenshot()).toMatchSnapshot();
  }

  await input.press('Backspace');
  await waitForAnimationEnd(page);
};
