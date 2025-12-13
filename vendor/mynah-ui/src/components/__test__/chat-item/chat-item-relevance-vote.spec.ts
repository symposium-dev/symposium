import { ChatItemRelevanceVote } from '../../chat-item/chat-item-relevance-vote';

// Mock the global events
jest.mock('../../../helper/events', () => ({
  MynahUIGlobalEvents: {
    getInstance: jest.fn(() => ({
      dispatch: jest.fn(),
      addListener: jest.fn().mockReturnValue('mock-listener-id'),
      removeListener: jest.fn()
    }))
  }
}));

describe('ChatItemRelevanceVote', () => {
  beforeEach(() => {
    jest.clearAllMocks();
    jest.useFakeTimers();
  });

  afterEach(() => {
    jest.useRealTimers();
  });

  it('should render relevance vote component', () => {
    const vote = new ChatItemRelevanceVote({
      tabId: 'test-tab',
      messageId: 'test-message'
    });

    expect(vote.render).toBeDefined();
  });
});
