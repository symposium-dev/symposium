import { ChatItemTreeView } from '../../chat-item/chat-item-tree-view';

describe('ChatItemTreeView', () => {
  it('should render tree view with file node', () => {
    const fileNode = {
      type: 'file' as const,
      name: 'test.ts',
      filePath: '/src/test.ts',
      originalFilePath: '/src/test.ts',
      deleted: false
    };

    const treeView = new ChatItemTreeView({
      tabId: 'test-tab',
      messageId: 'test-message',
      node: fileNode
    });

    expect(treeView.render).toBeDefined();
  });
});
