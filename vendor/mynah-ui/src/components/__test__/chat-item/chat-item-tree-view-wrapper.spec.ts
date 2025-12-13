import { ChatItemTreeViewWrapper } from '../../chat-item/chat-item-tree-view-wrapper';

describe('ChatItemTreeViewWrapper', () => {
  it('should render tree view wrapper', () => {
    const wrapper = new ChatItemTreeViewWrapper({
      tabId: 'test-tab',
      messageId: 'test-message',
      files: [],
      deletedFiles: [],
      referenceSuggestionLabel: 'Test Reference',
      references: [],
      onRootCollapsedStateChange: jest.fn()
    });

    expect(wrapper.render).toBeDefined();
  });
});
