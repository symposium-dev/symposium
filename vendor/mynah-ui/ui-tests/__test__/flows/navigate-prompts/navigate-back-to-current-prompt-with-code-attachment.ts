import { expect, Page } from 'playwright/test';
import { getSelector, justWait, waitForAnimationEnd } from '../../helpers';
import testIds from '../../../../src/helper/test-ids';

export const navigateBackToCurrentPromptWithCodeAttachment = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  await page.locator(getSelector(testIds.prompt.input)).fill('This is the first user prompt');
  await page.locator(getSelector(testIds.prompt.send)).nth(1).click();
  await waitForAnimationEnd(page);

  await page.evaluate(() => {
    const selectedTabId = window.mynahUI.getSelectedTabId();
    if (selectedTabId != null) {
      window.mynahUI.addToUserPrompt(
        selectedTabId,
        'This is an unsent code attachment',
        'code',
      );
    }
  });
  await waitForAnimationEnd(page);

  const promptInput = page.locator(getSelector(testIds.prompt.input));
  await promptInput.press('ArrowUp');
  await justWait(100);

  await promptInput.press('ArrowDown');
  await justWait(100);

  await promptInput.press('ArrowDown');
  await justWait(100);

  // we add .trim() because webpack test was failing otherwise, as it adds a \n at the end, like 'This is an unsent code attachment\n'
  const codeAttachmentContent = (await page.locator(getSelector(testIds.prompt.attachmentWrapper)).innerText()).trim();
  expect(codeAttachmentContent).toBe('This is an unsent code attachment');

  // Move the mouse outside of the attachment
  await page.mouse.move(0, 0);
  await justWait(500);

  if (skipScreenshots !== true) {
    const wrapper = page.locator(getSelector(testIds.prompt.wrapper));
    expect(await wrapper.screenshot()).toMatchSnapshot();
  }
};
