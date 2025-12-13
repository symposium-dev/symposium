import { ChatItemTreeFile } from '../../chat-item/chat-item-tree-file';

describe('ChatItemTreeFile', () => {
  it('should render tree file with basic properties', () => {
    const treeFile = new ChatItemTreeFile({
      tabId: 'test-tab',
      messageId: 'test-message',
      filePath: '/src/test.ts',
      originalFilePath: '/src/test.ts',
      fileName: 'test.ts'
    });

    expect(treeFile.render).toBeDefined();
  });
});
