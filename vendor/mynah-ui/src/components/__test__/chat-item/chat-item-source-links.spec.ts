import { ChatItemSourceLinksContainer } from '../../chat-item/chat-item-source-links';

// Mock the Card component
jest.mock('../../card/card', () => ({
  Card: jest.fn().mockImplementation(() => ({
    render: document.createElement('div')
  }))
}));

// Mock the SourceLinkHeader component
jest.mock('../../source-link/source-link-header', () => ({
  SourceLinkHeader: jest.fn().mockImplementation(() => ({
    render: document.createElement('div')
  }))
}));

describe('ChatItemSourceLinksContainer', () => {
  it('should not render when relatedContent is undefined', () => {
    const container = new ChatItemSourceLinksContainer({
      tabId: 'test-tab',
      messageId: 'test-message',
      title: 'Related Links'
    });

    expect(container.render).toBeUndefined();
  });
});
