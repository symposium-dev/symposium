import { Page } from 'playwright';
import { getSelector, waitForAnimationEnd } from '../../helpers';
import testIds from '../../../../src/helper/test-ids';
import { expect } from 'playwright/test';

export const renderFileDetails = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  await page.evaluate(() => {
    const selectedTabId = window.mynahUI.getSelectedTabId();
    if (selectedTabId != null) {
      window.mynahUI.updateStore(selectedTabId, {
        chatItems: [],
      });

      window.mynahUI.addChatItem(selectedTabId,
        {
          type: 'answer' as any,
          fileList: {
            rootFolderTitle: 'Folder',
            filePaths: [ './package.json', 'src/game.test.ts' ],
            deletedFiles: [
              './README.md'
            ],
            details: {
              './package.json': {
                icon: 'ok-circled' as any,
                description: 'a configuration file',
                label: 'configuration added',
                status: 'success'
              },
              'src/game.test.ts': {
                status: 'error',
                icon: 'error' as any,
                label: 'tests failed',
              }
            },
          },
        }
      );
    }
  });
  await waitForAnimationEnd(page);

  const fileWrapperLocator = page.locator(getSelector(testIds.chatItem.fileTree.wrapper));

  expect(await fileWrapperLocator.count()).toEqual(1);
  if (skipScreenshots !== true) {
    expect(await fileWrapperLocator.screenshot()).toMatchSnapshot();
  }
};
