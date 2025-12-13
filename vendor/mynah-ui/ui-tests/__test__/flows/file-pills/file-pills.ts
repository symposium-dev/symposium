import { Page } from 'playwright';
import { waitForAnimationEnd } from '../../helpers';
import { expect } from 'playwright/test';
import type { ChatItemType } from '@aws/mynah-ui';

export const showFilePills = async (page: Page): Promise<void> => {
  await page.evaluate(() => {
    const selectedTabId = window.mynahUI.getSelectedTabId();
    if (selectedTabId != null) {
      window.mynahUI.updateStore(selectedTabId, { chatItems: [] });

      window.mynahUI.addChatItem(selectedTabId, {
        type: 'answer' as ChatItemType,
        fullWidth: true,
        padding: false,
        header: {
          icon: 'eye',
          body: 'Reading files',
          fileList: {
            filePaths: [ 'package.json', 'README.md', 'src/app.ts', 'src/components/Button.tsx' ],
            details: {
              'package.json': { visibleName: 'package.json', description: 'Package configuration file' },
              'README.md': { visibleName: 'README.md', description: 'Project documentation' },
              'src/app.ts': { visibleName: 'app.ts', description: 'Main application file' },
              'src/components/Button.tsx': { visibleName: 'Button.tsx', description: 'Button component' }
            },
            renderAsPills: true
          }
        }
      });
    }
  });

  await waitForAnimationEnd(page);

  const filePillsLocator = page.locator('.mynah-chat-item-tree-file-pill');
  expect(await filePillsLocator.count()).toEqual(4);

  const pillTexts = await filePillsLocator.allTextContents();
  expect(pillTexts).toEqual([ 'package.json', 'README.md', 'app.ts', 'Button.tsx' ]);

  expect(await page.screenshot({ fullPage: true })).toMatchSnapshot('file-pills-basic.png');
};

export const showFilePillsWithDeletedFiles = async (page: Page): Promise<void> => {
  await page.evaluate(() => {
    const selectedTabId = window.mynahUI.getSelectedTabId();
    if (selectedTabId != null) {
      window.mynahUI.updateStore(selectedTabId, { chatItems: [] });

      window.mynahUI.addChatItem(selectedTabId, {
        type: 'answer' as ChatItemType,
        fullWidth: true,
        padding: false,
        header: {
          icon: 'check-list',
          body: 'Files processed',
          fileList: {
            filePaths: [ 'src/old-component.ts', 'src/new-component.ts', 'src/updated-file.ts' ],
            deletedFiles: [ 'src/old-component.ts' ],
            details: {
              'src/old-component.ts': { visibleName: 'old-component.ts', description: 'This file was deleted' },
              'src/new-component.ts': { visibleName: 'new-component.ts', description: 'This file was created' },
              'src/updated-file.ts': { visibleName: 'updated-file.ts', description: 'This file was modified' }
            },
            renderAsPills: true
          }
        }
      });
    }
  });

  await waitForAnimationEnd(page);

  const filePillsLocator = page.locator('.mynah-chat-item-tree-file-pill');
  expect(await filePillsLocator.count()).toEqual(3);

  const deletedPill = filePillsLocator.filter({ hasText: 'old-component.ts' });
  expect(await deletedPill.getAttribute('class')).toContain('mynah-chat-item-tree-file-pill-deleted');

  expect(await page.screenshot({ fullPage: true })).toMatchSnapshot('file-pills-with-deleted-files.png');
};
