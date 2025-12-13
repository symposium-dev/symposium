import { MynahUITabsStore } from '../../../helper/tabs-store';
import { PromptAttachment } from '../../chat-item/prompt-input/prompt-attachment';

describe('prompt-attachment', () => {
  it('renders code attachment', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {}
    }) as string;

    const attachment = new PromptAttachment({
      tabId: testTabId
    });

    attachment.updateAttachment('console.log("test");', 'code');
    expect(attachment.render.querySelector('.outer-container')).toBeDefined();
    expect(attachment.lastAttachmentContent).toContain('console.log("test");');
  });

  it('handles markdown attachment', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {}
    }) as string;

    const attachment = new PromptAttachment({
      tabId: testTabId
    });

    attachment.updateAttachment('# Test Markdown', 'markdown');
    expect(attachment.lastAttachmentContent).toContain('# Test Markdown');
  });

  it('clears attachment', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {}
    }) as string;

    const attachment = new PromptAttachment({
      tabId: testTabId
    });

    attachment.updateAttachment('test content', 'code');
    expect(attachment.lastAttachmentContent).not.toBe('');

    attachment.clear();
    expect(attachment.lastAttachmentContent).toBe('');
  });

  it('handles empty attachment', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {}
    }) as string;

    const attachment = new PromptAttachment({
      tabId: testTabId
    });

    attachment.updateAttachment(undefined, 'code');
    expect(attachment.lastAttachmentContent).toBe('');
  });
});
