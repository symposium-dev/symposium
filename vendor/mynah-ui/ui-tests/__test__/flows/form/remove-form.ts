import { expect, Page } from 'playwright/test';
import { getSelector, waitForAnimationEnd } from '../../helpers';
import testIds from '../../../../src/helper/test-ids';

export const removeForm = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  await page.evaluate(() => {
    const selectedTabId = window.mynahUI.getSelectedTabId();
    if (selectedTabId != null) {
      window.mynahUI.updateStore(selectedTabId, {
        chatItems: [],
      });

      window.mynahUI.addChatItem(selectedTabId, {
        type: 'answer' as any,
        snapToTop: true,
        body: 'Can you help us to improve our AI Assistant? Please fill the form below and hit **Submit** to send your feedback.',
        formItems: [
          {
            id: 'email',
            type: 'email',
            title: 'Email',
            placeholder: 'email',
          },
        ],
        buttons: [
          {
            id: 'submit',
            text: 'Submit',
            status: 'primary',
          },
          {
            id: 'cancel-feedback',
            text: 'Cancel',
            keepCardAfterClick: false,
            waitMandatoryFormItems: false,
          },
        ],
      });
    }
  });
  await waitForAnimationEnd(page);

  // Click the cancel button
  const cancelButton = page.locator(getSelector(testIds.chatItem.buttons.button)).nth(1);
  expect(cancelButton).toBeDefined();
  await cancelButton.click();

  await waitForAnimationEnd(page);

  if (skipScreenshots !== true) {
    expect(await page.screenshot()).toMatchSnapshot();
  }
};
