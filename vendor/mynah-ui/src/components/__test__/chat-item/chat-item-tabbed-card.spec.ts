import { ChatItemTabbedCard } from '../../chat-item/chat-item-tabbed-card';

// Mock the ChatItemCard component
jest.mock('../../chat-item/chat-item-card', () => ({
  ChatItemCard: jest.fn().mockImplementation((props) => ({
    render: document.createElement('div'),
    props,
    clearContent: jest.fn(),
    updateCardStack: jest.fn()
  }))
}));

// Mock the Tab component
jest.mock('../../tabs', () => ({
  Tab: jest.fn().mockImplementation(() => ({
    render: document.createElement('div')
  }))
}));

describe('ChatItemTabbedCard', () => {
  it('should render tabbed card with multiple tabs', () => {
    const tabbedCard = [
      {
        label: 'Tab 1',
        value: 'tab1',
        content: {
          body: 'Content 1'
        }
      },
      {
        label: 'Tab 2',
        value: 'tab2',
        content: {
          body: 'Content 2'
        }
      }
    ];

    const card = new ChatItemTabbedCard({
      tabId: 'test-tab',
      messageId: 'test-message',
      tabbedCard
    });

    expect(card.render).toBeDefined();

    // Check that the first tab's content is rendered by default
    expect(card.contentCard.props.chatItem.body).toBe('Content 1');
  });

  it('should render tabbed card with selected tab', () => {
    const tabbedCard = [
      {
        label: 'Tab 1',
        value: 'tab1',
        content: {
          body: 'Content 1'
        }
      },
      {
        label: 'Tab 2',
        value: 'tab2',
        selected: true,
        content: {
          body: 'Content 2'
        }
      }
    ];

    const card = new ChatItemTabbedCard({
      tabId: 'test-tab',
      messageId: 'test-message',
      tabbedCard
    });

    // Check that the selected tab's content is rendered
    expect(card.contentCard.props.chatItem.body).toBe('Content 2');
  });
});
