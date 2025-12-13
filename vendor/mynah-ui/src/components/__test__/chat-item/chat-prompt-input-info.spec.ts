import { MynahUITabsStore } from '../../../helper/tabs-store';
import { MynahUIGlobalEvents } from '../../../helper/events';
import { MynahEventNames } from '../../../static';
import { ChatPromptInputInfo } from '../../chat-item/chat-prompt-input-info';

describe('chat-prompt-input-info', () => {
  it('renders with info text', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {
        promptInputInfo: 'Test info message'
      }
    }) as string;

    const info = new ChatPromptInputInfo({
      tabId: testTabId
    });

    expect(info.render.querySelector('.mynah-chat-prompt-input-info')).toBeDefined();
    expect(info.render.textContent).toContain('Test info message');
  });

  it('handles empty info', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {
        promptInputInfo: ''
      }
    }) as string;

    const info = new ChatPromptInputInfo({
      tabId: testTabId
    });

    expect(info.render.children.length).toBe(0);
  });

  it('updates info text', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {
        promptInputInfo: 'Initial info'
      }
    }) as string;

    const info = new ChatPromptInputInfo({
      tabId: testTabId
    });

    MynahUITabsStore.getInstance().updateTab(testTabId, {
      store: {
        promptInputInfo: 'Updated info'
      }
    });

    expect(info.render.textContent).toContain('Updated info');
  });

  it('clears info when set to null', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {
        promptInputInfo: 'Initial info'
      }
    }) as string;

    const info = new ChatPromptInputInfo({
      tabId: testTabId
    });

    MynahUITabsStore.getInstance().updateTab(testTabId, {
      store: {
        promptInputInfo: ''
      }
    });

    expect(info.render.children.length).toBe(0);
  });

  it('handles link clicks', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {
        promptInputInfo: '<a href="https://example.com">Test link</a>'
      }
    }) as string;

    let linkClicked = false;
    const originalDispatch = MynahUIGlobalEvents.getInstance().dispatch;
    MynahUIGlobalEvents.getInstance().dispatch = (eventName: string) => {
      if (eventName === MynahEventNames.INFO_LINK_CLICK) {
        linkClicked = true;
      }
    };

    const info = new ChatPromptInputInfo({
      tabId: testTabId
    });

    const link = info.render.querySelector('a') as HTMLElement;
    if (link != null) {
      link.click();
      expect(linkClicked).toBe(true);
    }

    MynahUIGlobalEvents.getInstance().dispatch = originalDispatch;
  });
});
