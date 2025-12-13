import { Page } from 'playwright';
import { getSelector, waitForAnimationEnd } from '../../helpers';
import testIds from '../../../../src/helper/test-ids';
import { expect } from 'playwright/test';

export const showFileTree = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
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
            filePaths: [ './package.json', './tsconfig.json', 'src/game.ts', 'tests/game.test.ts' ],
            deletedFiles: [],
            details: {
              './package.json': {
                description: 'a configuration file',
              },
            },
            actions: {
              './package.json': [
                {
                  icon: 'cancel-circle' as any,
                  status: 'error',
                  name: 'reject-change',
                  description: 'Reject change',
                },
              ],
              './tsconfig.json': [
                {
                  icon: 'cancel-circle' as any,
                  status: 'error',
                  name: 'reject-change',
                  description: 'Reject change',
                },
              ]
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
