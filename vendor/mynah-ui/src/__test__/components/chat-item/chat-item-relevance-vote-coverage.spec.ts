import { ChatItemRelevanceVote } from '../../../components/chat-item/chat-item-relevance-vote';
import { MynahEventNames, RelevancyVoteType } from '../../../static';
import { MynahUIGlobalEvents } from '../../../helper/events';
import { Config } from '../../../helper/config';
import testIds from '../../../helper/test-ids';

jest.mock('../../../helper/config');
jest.mock('../../../helper/events');

describe('ChatItemRelevanceVote Coverage Tests', () => {
  let relevanceVote: ChatItemRelevanceVote;
  let mockGlobalEvents: jest.Mocked<MynahUIGlobalEvents>;

  beforeEach(() => {
    document.body.innerHTML = '';

    (Config.getInstance as jest.Mock).mockReturnValue({
      config: {
        texts: {
          feedbackThanks: 'Thank you!',
          feedbackReportButtonLabel: 'Report'
        },
        componentClasses: {},
        test: true
      }
    });

    mockGlobalEvents = {
      dispatch: jest.fn(),
      addListener: jest.fn().mockReturnValue('listener-id'),
      removeListener: jest.fn()
    } as any;
    (MynahUIGlobalEvents.getInstance as jest.Mock).mockReturnValue(mockGlobalEvents);
  });

  afterEach(() => {
    jest.clearAllMocks();
  });

  it('should handle downvote', () => {
    relevanceVote = new ChatItemRelevanceVote({
      tabId: 'tab-1',
      messageId: 'msg-1'
    });
    document.body.appendChild(relevanceVote.render);

    const downvoteInput = relevanceVote.render.querySelector(`[data-testid="${testIds.chatItem.vote.downvote}"]`) as HTMLInputElement;
    downvoteInput.checked = true;
    downvoteInput.dispatchEvent(new Event('change'));

    expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
      MynahEventNames.CARD_VOTE,
      {
        messageId: 'msg-1',
        tabId: 'tab-1',
        vote: RelevancyVoteType.DOWN
      }
    );
  });
});
