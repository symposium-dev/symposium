import { ChatItemInformationCard } from '../../chat-item/chat-item-information-card';

// Mock the ChatItemCard component
jest.mock('../../chat-item/chat-item-card', () => ({
  ChatItemCard: jest.fn().mockImplementation(() => ({
    render: document.createElement('div')
  }))
}));

// Mock the TitleDescriptionWithIcon component
jest.mock('../../title-description-with-icon', () => ({
  TitleDescriptionWithIcon: jest.fn().mockImplementation(() => ({
    render: document.createElement('div')
  }))
}));

describe('ChatItemInformationCard', () => {
  it('should render information card with basic properties', () => {
    const card = new ChatItemInformationCard({
      tabId: 'test-tab',
      messageId: 'test-message',
      informationCard: {
        title: 'Test Information',
        content: {
          body: 'Test content'
        }
      }
    });

    expect(card.render).toBeDefined();
    expect(card.render.classList.contains('mynah-chat-item-information-card')).toBe(true);
  });
});
