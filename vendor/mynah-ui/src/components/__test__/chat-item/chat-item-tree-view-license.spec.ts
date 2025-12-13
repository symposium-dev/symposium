import { ChatItemTreeViewLicense } from '../../chat-item/chat-item-tree-view-license';

describe('ChatItemTreeViewLicense', () => {
  it('should render tree view license', () => {
    const license = new ChatItemTreeViewLicense({
      referenceSuggestionLabel: 'Test Reference',
      references: []
    });

    expect(license.render).toBeDefined();
  });
});
