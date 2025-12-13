import { ChatItemFollowUpContainer } from '../../chat-item/chat-item-followup';
import { ChatItemType } from '../../../static';

describe('ChatItemFollowUpContainer', () => {
  it('should render followup container', () => {
    const container = new ChatItemFollowUpContainer({
      tabId: 'test-tab',
      chatItem: {
        type: ChatItemType.ANSWER,
        followUp: {
          options: []
        }
      }
    });

    expect(container.render).toBeDefined();
  });
});
