import { expect, Page } from 'playwright/test';
import { getSelector, waitForAnimationEnd } from '../helpers';
import testIds from '../../../src/helper/test-ids';
import { ChatItemType } from '../../../dist/static';

export const renderIcons = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  await page.evaluate((body) => {
    const selectedTabId = window.mynahUI.getSelectedTabId();
    if (selectedTabId != null) {
      window.mynahUI.updateStore(selectedTabId, {
        chatItems: [],
      });
      window.mynahUI.addChatItem(selectedTabId, {
        type: 'answer' as ChatItemType,
        messageId: new Date().getTime().toString(),
        fullWidth: true,
        padding: false,
        header: {
          body: 'Custom icon + colored (foreground)',
          icon: {
            name: 'javascript',
            base64Svg:
                    'data:image/svg+xml;base64,PHN2ZyB3aWR0aD0iMTIiIGhlaWdodD0iOCIgdmlld0JveD0iMCAwIDEyIDgiIGZpbGw9Im5vbmUiIHhtbG5zPSJodHRwOi8vd3d3LnczLm9yZy8yMDAwL3N2ZyI+CjxwYXRoIGQ9Ik0zLjEyIDAuMjRINC44MlY1QzQuODIgNi4wMjY2NyA0LjU4IDYuNzczMzMgNC4xIDcuMjRDMy42NiA3LjY2NjY3IDMgNy44OCAyLjEyIDcuODhDMS45MiA3Ljg4IDEuNyA3Ljg2NjY3IDEuNDYgNy44NEMxLjIyIDcuOCAxLjAyNjY3IDcuNzQ2NjcgMC44OCA3LjY4TDEuMDYgNi4zMkMxLjMxMzMzIDYuNDQgMS42MDY2NyA2LjUgMS45NCA2LjVDMi4zMTMzMyA2LjUgMi41OTMzMyA2LjQgMi43OCA2LjJDMy4wMDY2NyA1Ljk2IDMuMTIgNS41NiAzLjEyIDVWMC4yNFpNNi4zMiA2QzYuNTYgNi4xMzMzMyA2Ljg0IDYuMjQ2NjcgNy4xNiA2LjM0QzcuNTIgNi40NDY2NyA3Ljg2IDYuNSA4LjE4IDYuNUM4LjU4IDYuNSA4Ljg4IDYuNDMzMzMgOS4wOCA2LjNDOS4yOCA2LjE1MzMzIDkuMzggNS45NTMzMyA5LjM4IDUuN0M5LjM4IDUuNDQ2NjcgOS4yODY2NyA1LjI0NjY3IDkuMSA1LjFDOC45MTMzMyA0Ljk0IDguNTg2NjcgNC43OCA4LjEyIDQuNjJDNi43NDY2NyA0LjE0IDYuMDYgMy4zOTMzMyA2LjA2IDIuMzhDNi4wNiAxLjcxMzMzIDYuMzEzMzMgMS4xNzMzMyA2LjgyIDAuNzU5OTk5QzcuMzQgMC4zMzMzMzMgOC4wNDY2NyAwLjEyIDguOTQgMC4xMkM5LjY0NjY3IDAuMTIgMTAuMjkzMyAwLjI0NjY2NyAxMC44OCAwLjVMMTAuNSAxLjg4TDEwLjM4IDEuODJDMTAuMTQgMS43MjY2NyA5Ljk0NjY3IDEuNjYgOS44IDEuNjJDOS41MiAxLjU0IDkuMjMzMzMgMS41IDguOTQgMS41QzguNTggMS41IDguMyAxLjU3MzMzIDguMSAxLjcyQzcuOTEzMzMgMS44NTMzMyA3LjgyIDIuMDMzMzMgNy44MiAyLjI2QzcuODIgMi40ODY2NyA3LjkyNjY3IDIuNjczMzMgOC4xNCAyLjgyQzguMyAyLjk0IDguNjQ2NjcgMy4xMDY2NyA5LjE4IDMuMzJDOS44NDY2NyAzLjU3MzMzIDEwLjMzMzMgMy44OCAxMC42NCA0LjI0QzEwLjk2IDQuNiAxMS4xMiA1LjA0IDExLjEyIDUuNTZDMTEuMTIgNi4yMjY2NyAxMC44NjY3IDYuNzY2NjcgMTAuMzYgNy4xOEM5LjgxMzMzIDcuNjQ2NjcgOS4wNDY2NyA3Ljg4IDguMDYgNy44OEM3LjY3MzMzIDcuODggNy4yNjY2NyA3LjgyNjY3IDYuODQgNy43MkM2LjUwNjY3IDcuNjUzMzMgNi4yMDY2NyA3LjU2IDUuOTQgNy40NEw2LjMyIDZaIiBmaWxsPSIjQ0JDQjQxIi8+Cjwvc3ZnPgo=',
          },
          iconForegroundStatus: 'success'
        },
      });
      window.mynahUI.addChatItem(selectedTabId, {
        type: 'answer' as ChatItemType,
        messageId: new Date().getTime().toString(),
        fullWidth: true,
        padding: false,
        header: {
          body: 'Custom icon + colored (background)',
          icon: {
            name: 'typescript',
            base64Svg:
                    'data:image/svg+xml;base64,PD94bWwgdmVyc2lvbj0iMS4wIiBlbmNvZGluZz0idXRmLTgiPz48IS0tIFVwbG9hZGVkIHRvOiBTVkcgUmVwbywgd3d3LnN2Z3JlcG8uY29tLCBHZW5lcmF0b3I6IFNWRyBSZXBvIE1peGVyIFRvb2xzIC0tPgo8c3ZnIHdpZHRoPSI4MDBweCIgaGVpZ2h0PSI4MDBweCIgdmlld0JveD0iMCAwIDMyIDMyIiB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciPjx0aXRsZT5maWxlX3R5cGVfdHlwZXNjcmlwdDwvdGl0bGU+PHBhdGggZD0iTTIzLjgyNyw4LjI0M0E0LjQyNCw0LjQyNCwwLDAsMSwyNi4wNSw5LjUyNGE1Ljg1Myw1Ljg1MywwLDAsMSwuODUyLDEuMTQzYy4wMTEuMDQ1LTEuNTM0LDEuMDgzLTIuNDcxLDEuNjYyLS4wMzQuMDIzLS4xNjktLjEyNC0uMzIyLS4zNWEyLjAxNCwyLjAxNCwwLDAsMC0xLjY3LTFjLTEuMDc3LS4wNzQtMS43NzEuNDktMS43NjYsMS40MzNhMS4zLDEuMywwLDAsMCwuMTUzLjY2NmMuMjM3LjQ5LjY3Ny43ODQsMi4wNTksMS4zODMsMi41NDQsMS4wOTUsMy42MzYsMS44MTcsNC4zMSwyLjg0M2E1LjE1OCw1LjE1OCwwLDAsMSwuNDE2LDQuMzMzLDQuNzY0LDQuNzY0LDAsMCwxLTMuOTMyLDIuODE1LDEwLjksMTAuOSwwLDAsMS0yLjcwOC0uMDI4LDYuNTMxLDYuNTMxLDAsMCwxLTMuNjE2LTEuODg0LDYuMjc4LDYuMjc4LDAsMCwxLS45MjYtMS4zNzEsMi42NTUsMi42NTUsMCwwLDEsLjMyNy0uMjA4Yy4xNTgtLjA5Ljc1Ni0uNDM0LDEuMzItLjc2MUwxOS4xLDE5LjZsLjIxNC4zMTJhNC43NzEsNC43NzEsMCwwLDAsMS4zNSwxLjI5MiwzLjMsMy4zLDAsMCwwLDMuNDU4LS4xNzUsMS41NDUsMS41NDUsMCwwLDAsLjItMS45NzRjLS4yNzYtLjM5NS0uODQtLjcyNy0yLjQ0My0xLjQyMmE4LjgsOC44LDAsMCwxLTMuMzQ5LTIuMDU1LDQuNjg3LDQuNjg3LDAsMCwxLS45NzYtMS43NzcsNy4xMTYsNy4xMTYsMCwwLDEtLjA2Mi0yLjI2OCw0LjMzMiw0LjMzMiwwLDAsMSwzLjY0NC0zLjM3NEE5LDksMCwwLDEsMjMuODI3LDguMjQzWk0xNS40ODQsOS43MjZsLjAxMSwxLjQ1NGgtNC42M1YyNC4zMjhINy42VjExLjE4M0gyLjk3VjkuNzU1QTEzLjk4NiwxMy45ODYsMCwwLDEsMy4wMSw4LjI4OWMuMDE3LS4wMjMsMi44MzItLjAzNCw2LjI0NS0uMDI4bDYuMjExLjAxN1oiIHN0eWxlPSJmaWxsOiMwMDdhY2MiLz48L3N2Zz4=',
          },
          iconStatus: 'success'
        },
      });
    }
  });
  await waitForAnimationEnd(page);

  const answerCardSelector = getSelector(testIds.chatItem.type.answer);
  const locator1 = page.locator(answerCardSelector).nth(0);
  await locator1.scrollIntoViewIfNeeded();
  if (skipScreenshots !== true) {
    expect(await locator1.screenshot()).toMatchSnapshot();
  }

  const locator2 = page.locator(answerCardSelector).nth(2);
  await locator2.scrollIntoViewIfNeeded();
  if (skipScreenshots !== true) {
    expect(await locator2.screenshot()).toMatchSnapshot();
  }
};
