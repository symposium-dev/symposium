import { MynahUITabsStore } from '../../../helper/tabs-store';
import { ChatPromptInputStickyCard } from '../../chat-item/chat-prompt-input-sticky-card';

describe('chat-prompt-input-sticky-card', () => {
  it('renders with sticky card data', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {
        promptInputStickyCard: {
          body: 'Test sticky card content',
          messageId: 'test-message'
        }
      }
    }) as string;

    const stickyCard = new ChatPromptInputStickyCard({
      tabId: testTabId
    });

    expect(stickyCard.render.querySelector('.mynah-chat-prompt-input-sticky-card')).toBeDefined();
    expect(stickyCard.render.textContent).toContain('Test sticky card content');
  });

  it('handles null sticky card', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {
        promptInputStickyCard: null
      }
    }) as string;

    const stickyCard = new ChatPromptInputStickyCard({
      tabId: testTabId
    });

    expect(stickyCard.render.children.length).toBe(0);
  });

  it('updates sticky card content', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {
        promptInputStickyCard: {
          body: 'Initial content'
        }
      }
    }) as string;

    const stickyCard = new ChatPromptInputStickyCard({
      tabId: testTabId
    });

    MynahUITabsStore.getInstance().updateTab(testTabId, {
      store: {
        promptInputStickyCard: {
          body: 'Updated content'
        }
      }
    });

    expect(stickyCard.render.textContent).toContain('Updated content');
  });

  it('clears sticky card when set to null', () => {
    const testTabId = MynahUITabsStore.getInstance().addTab({
      isSelected: true,
      store: {
        promptInputStickyCard: {
          body: 'Initial content'
        }
      }
    }) as string;

    const stickyCard = new ChatPromptInputStickyCard({
      tabId: testTabId
    });

    MynahUITabsStore.getInstance().updateTab(testTabId, {
      store: {
        promptInputStickyCard: null
      }
    });

    expect(stickyCard.render.children.length).toBe(0);
  });
});
