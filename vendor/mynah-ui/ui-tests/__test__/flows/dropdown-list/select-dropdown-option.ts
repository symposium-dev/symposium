import { expect, Page } from 'playwright/test';
import { getSelector, waitForAnimationEnd } from '../../helpers';
import testIds from '../../../../src/helper/test-ids';
import { ChatItemType } from '../../../../dist/static';

export const selectDropdownOption = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  await page.evaluate(() => {
    const selectedTabId = window.mynahUI.getSelectedTabId();
    if (selectedTabId != null) {
      window.mynahUI.updateStore(selectedTabId, {
        chatItems: [],
      });

      window.mynahUI.addChatItem(selectedTabId, {
        type: 'answer' as ChatItemType,
        snapToTop: true,
        body: 'Test dropdown option selection:',
        quickSettings: {
          type: 'select',
          tabId: selectedTabId,
          messageId: 'dropdown-select-test-message',
          options: [
            { id: 'opt1', label: 'First Choice', value: 'opt1', selected: true },
            { id: 'opt2', label: 'Second Choice', value: 'opt2', selected: false },
            { id: 'opt3', label: 'Third Choice', value: 'opt3', selected: false }
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
    await expect(dropdownButton.locator('.mynah-button-label')).toHaveText('First Choice');

    // Take initial screenshot
    expect(await page.screenshot()).toMatchSnapshot('dropdown-select-initial.png');

    // Open dropdown
    await dropdownButton.click();
    await waitForAnimationEnd(page);

    // Verify dropdown portal is created and visible
    const dropdownPortal = page.locator(getSelector(testIds.dropdownList.portal));
    expect(dropdownPortal).toBeDefined();

    // Take screenshot of open dropdown
    expect(await page.screenshot()).toMatchSnapshot('dropdown-select-open.png');

    // Verify first option is selected (has check mark)
    const firstOption = dropdownPortal.locator(`${getSelector(testIds.dropdownList.option)}[data-item-id="opt1"]`);
    expect(firstOption).toBeDefined();
    const firstCheckIcon = firstOption.locator(getSelector(testIds.dropdownList.checkIcon));
    expect(firstCheckIcon).toBeDefined();

    // Click on the second option
    const secondOption = dropdownPortal.locator(`${getSelector(testIds.dropdownList.option)}[data-item-id="opt2"]`);
    expect(secondOption).toBeDefined();
    await secondOption.click();
    await waitForAnimationEnd(page);

    // Verify dropdown is closed after selection
    await expect(dropdownPortal).toHaveCount(0);

    // Verify button text updated to show new selection
    await expect(dropdownButton.locator('.mynah-button-label')).toHaveText('Second Choice');

    // Take final screenshot
    expect(await page.screenshot()).toMatchSnapshot('dropdown-select-final.png');

    // Open dropdown again to verify selection persisted
    await dropdownButton.click();
    await waitForAnimationEnd(page);

    // Verify second option is now selected
    const secondOptionSelected = dropdownPortal.locator(`${getSelector(testIds.dropdownList.option)}[data-item-id="opt2"]`);
    expect(secondOptionSelected).toBeDefined();
    const secondCheckIcon = secondOptionSelected.locator(getSelector(testIds.dropdownList.checkIcon));
    expect(secondCheckIcon).toBeDefined();

    // Close dropdown by clicking outside
    await page.click('body', { position: { x: 10, y: 10 } });
    await waitForAnimationEnd(page);
  }
};
