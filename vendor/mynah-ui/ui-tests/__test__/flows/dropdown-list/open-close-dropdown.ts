import { expect, Page } from 'playwright/test';
import { getSelector, waitForAnimationEnd } from '../../helpers';
import testIds from '../../../../src/helper/test-ids';
import { ChatItemType } from '../../../../dist/static';

export const openCloseDropdown = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  await page.evaluate(() => {
    const selectedTabId = window.mynahUI.getSelectedTabId();
    if (selectedTabId != null) {
      window.mynahUI.updateStore(selectedTabId, {
        chatItems: [],
      });

      window.mynahUI.addChatItem(selectedTabId, {
        type: 'answer' as ChatItemType,
        snapToTop: true,
        body: 'Here is a test dropdown:',
        quickSettings: {
          type: 'select',
          description: 'Choose one of the following options',
          tabId: selectedTabId,
          messageId: 'dropdown-test-message',
          options: [
            { id: 'option1', label: 'Option 1', value: 'option1', selected: false },
            { id: 'option2', label: 'Option 2', value: 'option2', selected: true },
            { id: 'option3', label: 'Option 3', value: 'option3', selected: false }
          ]
        }
      });
    }
  });

  await waitForAnimationEnd(page);

  if (skipScreenshots !== true) {
    // Find the dropdown wrapper using test ID
    const dropdownWrapper = page.locator(getSelector(testIds.dropdownList.wrapper)).first();
    expect(dropdownWrapper).toBeDefined();

    // Find the dropdown button using test ID
    const dropdownButton = page.locator(getSelector(testIds.dropdownList.button));
    expect(dropdownButton).toBeDefined();

    // Verify initial button text shows selected option
    await expect(dropdownButton.locator('.mynah-button-label')).toHaveText('Option 2');

    // Take initial screenshot
    expect(await page.screenshot()).toMatchSnapshot('dropdown-initial.png');

    // Click to open dropdown
    await dropdownButton.click();
    await waitForAnimationEnd(page);

    // Verify dropdown portal is created and visible
    const dropdownPortal = page.locator(getSelector(testIds.dropdownList.portal));
    expect(dropdownPortal).toBeDefined();

    // Verify dropdown content
    const dropdownDescription = dropdownPortal.locator(getSelector(testIds.dropdownList.description));
    await expect(dropdownDescription).toHaveText('Choose one of the following options');

    // Verify options are present
    const options = dropdownPortal.locator(getSelector(testIds.dropdownList.option));
    await expect(options).toHaveCount(3);

    // Verify selected option has check mark
    const selectedOption = dropdownPortal.locator('.mynah-dropdown-list-option.selected');
    await expect(selectedOption).toHaveCount(1);
    await expect(selectedOption.locator(getSelector(testIds.dropdownList.optionLabel))).toContainText('Option 2');

    // Take screenshot of open dropdown
    expect(await page.screenshot()).toMatchSnapshot('dropdown-open.png');

    // Click outside to close dropdown
    await page.click('body', { position: { x: 10, y: 10 } });
    await waitForAnimationEnd(page);

    // Verify dropdown is closed
    await expect(dropdownPortal).toHaveCount(0);

    // Take final screenshot
    expect(await page.screenshot()).toMatchSnapshot('dropdown-closed.png');
  }
};
