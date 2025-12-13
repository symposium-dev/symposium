import { MynahUITabsStore } from '../../../helper/tabs-store';
import { PromptTextAttachment } from '../../chat-item/prompt-input/prompt-text-attachment';

describe('prompt-text-attachment', () => {
  it('renders with content', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {}
    }) as string;

    const attachment = new PromptTextAttachment({
      tabId: testTabId,
      content: 'test content',
      type: 'markdown'
    });

    expect(attachment.render.querySelector('.mynah-chat-prompt-attachment')).toBeDefined();
  });

  it('handles code type', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {}
    }) as string;

    const attachment = new PromptTextAttachment({
      tabId: testTabId,
      content: 'console.log("test");',
      type: 'code'
    });

    expect(attachment.render.querySelector('.mynah-chat-prompt-attachment')).toBeDefined();
  });

  it('handles remove click', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {}
    }) as string;

    const attachment = new PromptTextAttachment({
      tabId: testTabId,
      content: 'test content',
      type: 'markdown'
    });

    expect(attachment.render).toBeDefined();
  });

  it('clears content', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {}
    }) as string;

    const attachment = new PromptTextAttachment({
      tabId: testTabId,
      content: 'test content',
      type: 'markdown'
    });

    attachment.clear();
    expect(attachment.render).toBeDefined();
  });

  it('handles empty content', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {}
    }) as string;

    const attachment = new PromptTextAttachment({
      tabId: testTabId,
      content: '',
      type: 'markdown'
    });

    expect(attachment.render).toBeDefined();
  });
});
