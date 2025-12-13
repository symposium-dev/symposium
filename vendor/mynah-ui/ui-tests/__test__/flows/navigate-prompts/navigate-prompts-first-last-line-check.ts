import { expect, Page } from 'playwright/test';
import { getSelector, justWait, waitForAnimationEnd } from '../../helpers';
import testIds from '../../../../src/helper/test-ids';

export const navigatePromptsFirstLastLineCheck = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  const firstPrompt = 'This is the first user prompt';
  const secondPrompt = 'This is the second user prompt.\nIt spans two separate lines.';

  const promptInput = page.locator(getSelector(testIds.prompt.input));
  const sendButton = page.locator(getSelector(testIds.prompt.send)).nth(1);

  await promptInput.fill(firstPrompt);
  await sendButton.click();
  await waitForAnimationEnd(page);

  await promptInput.fill(secondPrompt);
  await waitForAnimationEnd(page);

  // The input should start as the input with two lines
  expect(await promptInput.innerText()).toBe(secondPrompt);

  // Input should remain the same as it is multiline
  await promptInput.press('ArrowDown');
  await justWait(100);
  expect(await promptInput.innerText()).toBe(secondPrompt);

  // Go back to the first line
  await promptInput.press('ArrowUp');
  // Go to the beginning of the line
  await promptInput.press('ArrowUp');
  await justWait(100);

  // Now that we're in the first line again, it should navigate to the first user prompt
  await promptInput.press('ArrowUp');
  await justWait(100);
  expect(await promptInput.innerText()).toBe(firstPrompt);

  // Given that this input only has one line, we should be able to go down to prompt 2 immediately again, after going to the end of the text
  await promptInput.press('ArrowDown');
  await justWait(100);
  await promptInput.press('ArrowDown');
  await justWait(100);

  // The explicit \n is lost at the end, so we account for that as it is expected
  expect(await promptInput.innerText()).toBe(secondPrompt.replace('\n', ''));
};
