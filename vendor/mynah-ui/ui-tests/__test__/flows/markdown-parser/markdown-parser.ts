import { expect, Page } from 'playwright/test';
import { DEFAULT_VIEWPORT, getOffsetHeight, getSelector, waitForAnimationEnd } from '../../helpers';
import testIds from '../../../../src/helper/test-ids';
import allMarkdown from './all-markdown-tags';

export const parseMarkdown = async (page: Page, skipScreenshots?: boolean): Promise<void> => {
  await page.evaluate((body) => {
    const selectedTabId = window.mynahUI.getSelectedTabId();
    if (selectedTabId != null) {
      window.mynahUI.updateStore(selectedTabId, {
        chatItems: [],
      });

      window.mynahUI.addChatItem(selectedTabId, {
        type: 'answer' as any,
        snapToTop: true,
        codeReference: [ {
          information: 'Hello Reference Tracker',
          recommendationContentSpan: {
            start: 4828, // because of base64 image length calculation
            end: 4837
          }
        } ],
        body
      });
    }
  }, allMarkdown);
  await waitForAnimationEnd(page);

  const answerCardSelector = getSelector(testIds.chatItem.type.answer);
  const answerCard = await page.waitForSelector(answerCardSelector);
  const newViewportHeight = getOffsetHeight(await answerCard.boundingBox()) ?? 1000;

  // Update viewport size
  await page.setViewportSize({
    width: DEFAULT_VIEWPORT.width,
    // Chromium doesn't accept float numbers for the viewport, has to be converted to int
    height: Math.ceil(newViewportHeight) + 300
  });
  await answerCard.scrollIntoViewIfNeeded();

  // Headings
  expect(await answerCard.evaluate(node => node.querySelector('h1')?.innerText)).toBe('Heading 1');
  expect(await answerCard.evaluate(node => node.querySelector('h2')?.innerText)).toBe('Heading 2');
  expect(await answerCard.evaluate(node => node.querySelector('h3')?.innerText)).toBe('Heading 3');
  expect(await answerCard.evaluate(node => node.querySelector('h4')?.innerText)).toBe('Heading 4');
  expect(await answerCard.evaluate(node => node.querySelector('h5')?.innerText)).toBe('Heading 5');
  expect(await answerCard.evaluate(node => node.querySelector('h6')?.innerText)).toBe('Heading 6');

  // Italic, bold and strikethrough
  expect(await answerCard.evaluate(node => node.querySelector('del')?.innerText)).toBe('Strikethrough text');
  expect(await answerCard.evaluate(node => Array.from(node.querySelectorAll('em')).filter(elm => elm.innerText === 'Italic text').length)).toBe(2);
  expect(await answerCard.evaluate(node => Array.from(node.querySelectorAll('strong')).filter(elm => elm.innerText === 'Bold text').length)).toBe(2);
  expect(await answerCard.evaluate(node => Array.from(node.querySelectorAll('em > strong')).filter((elm) => (elm as HTMLElement).innerText === 'Bold and italic text').length)).toBe(2);

  // List items
  expect(await answerCard.evaluate(node => {
    const firstUl = node.querySelector('ul');
    const firstLiP = firstUl?.querySelector('li:first-child > p');
    return firstLiP != null ? (firstLiP as HTMLElement).innerText : '';
  })).toBe('Item 1');
  expect(await answerCard.evaluate(node => {
    const firstUl = node.querySelector('ul');
    const secondLiUlLi = firstUl?.querySelector('li:nth-child(2) > ul > li:first-child > p');
    return secondLiUlLi != null ? (secondLiUlLi as HTMLElement).innerText : '';
  })).toBe('Subitem 1');
  expect(await answerCard.evaluate(node => node.querySelector('ol > li:first-child > input[checked][disabled][type="checkbox"]'))).toBeDefined();

  // Anchors and IMG
  // We're not expecting any link except [TEXT](URL) format, we should have only 1 link.
  expect(await answerCard.evaluate(node => node.querySelectorAll('a[href="https://github.com/aws/mynah-ui"]').length)).toBe(1);
  expect(await answerCard.evaluate(node => node.querySelectorAll('a[href="https://github.com/aws/mynah-ui"]')[0]?.innerHTML)).toBe('mynah-ui');
  expect(await answerCard.evaluate(node => node.querySelector('img[src="https://placehold.co/600x400"]'))).toBeDefined();

  // Table
  expect(await answerCard.evaluate(node => node.querySelector('table > tbody > tr:nth-child(2) > td:nth-child(2)')?.innerHTML)).toBe('2');

  // Code blocks
  expect(await answerCard.evaluate(node => node.querySelectorAll('pre > code')[0]?.innerHTML)).toBe('inline code');
  expect(await answerCard.evaluate(node => node.querySelectorAll('pre')[2]?.nextElementSibling?.nextElementSibling?.querySelector(':scope > span')?.innerHTML)).toBe('javascript');
  expect(await answerCard.evaluate(node => node.querySelectorAll('pre')[2]?.nextElementSibling?.nextElementSibling?.querySelector(':scope > button')?.querySelector(':scope > span')?.innerHTML)).toBe('Copy');

  // Reference highlight
  expect(await answerCard.evaluate(node => node.querySelectorAll('pre')[1]?.querySelector(':scope > code > mark')?.innerHTML)).toBe('no syntax');

  // Reference hover
  const markPosition = await answerCard.evaluate(node => node.querySelectorAll('pre')[2]?.querySelector(':scope > code > mark')?.getBoundingClientRect());
  if (markPosition != null) {
    await page.mouse.move(Number(markPosition.top) + 2, Number(markPosition.left) + 2);
    await waitForAnimationEnd(page);
  }
  expect(page.getByText('Hello Reference Tracker')).toBeDefined();
  page.mouse.move(0, 0);
  await waitForAnimationEnd(page);
  expect(await page.getByText('Hello Reference Tracker').isHidden()).toBeTruthy();

  if (skipScreenshots !== true) {
    expect(await answerCard.screenshot()).toMatchSnapshot();
  }
};
