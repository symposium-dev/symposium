/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { ChatItemRelevanceVote, ChatItemRelevanceVoteProps } from '../../../components/chat-item/chat-item-relevance-vote';
import { MynahEventNames, RelevancyVoteType } from '../../../static';
import { MynahUIGlobalEvents } from '../../../helper/events';
import { Config } from '../../../helper/config';
import testIds from '../../../helper/test-ids';

// Mock dependencies
jest.mock('../../../helper/config');
jest.mock('../../../helper/events');

describe('ChatItemRelevanceVote Component', () => {
  let relevanceVote: ChatItemRelevanceVote;
  let mockConfig: jest.Mocked<Config>;
  let mockGlobalEvents: jest.Mocked<MynahUIGlobalEvents>;

  const mockTexts = {
    feedbackThanks: 'Thank you for your feedback!',
    feedbackReportButtonLabel: 'Report an issue'
  };

  const defaultProps: ChatItemRelevanceVoteProps = {
    tabId: 'test-tab-123',
    messageId: 'test-message-456',
    classNames: [ 'custom-class' ]
  };

  beforeEach(() => {
    document.body.innerHTML = '';

    // Setup Config mock
    mockConfig = {
      config: {
        texts: mockTexts,
        componentClasses: {},
        test: true
      }
    } as any;
    (Config.getInstance as jest.Mock).mockReturnValue(mockConfig);

    // Setup GlobalEvents mock
    mockGlobalEvents = {
      dispatch: jest.fn(),
      addListener: jest.fn().mockReturnValue('listener-id-123'),
      removeListener: jest.fn()
    } as any;
    (MynahUIGlobalEvents.getInstance as jest.Mock).mockReturnValue(mockGlobalEvents);
  });

  afterEach(() => {
    document.body.innerHTML = '';
    jest.clearAllMocks();
    jest.clearAllTimers();
  });

  describe('Constructor', () => {
    it('should create relevance vote component with required props', () => {
      const minimalProps: ChatItemRelevanceVoteProps = {
        tabId: 'tab-1',
        messageId: 'msg-1'
      };

      relevanceVote = new ChatItemRelevanceVote(minimalProps);

      expect(relevanceVote).toBeDefined();
      expect(relevanceVote.props).toEqual(minimalProps);
      expect(relevanceVote.render).toBeDefined();
    });

    it('should handle undefined classNames', () => {
      const propsWithoutClassNames: ChatItemRelevanceVoteProps = {
        tabId: 'tab-1',
        messageId: 'msg-1'
      };

      relevanceVote = new ChatItemRelevanceVote(propsWithoutClassNames);

      expect(relevanceVote).toBeDefined();
      expect(relevanceVote.render.classList.contains('mynah-card-votes-wrapper')).toBe(true);
    });
  });

  describe('Upvote Functionality', () => {
    beforeEach(() => {
      relevanceVote = new ChatItemRelevanceVote(defaultProps);
      document.body.appendChild(relevanceVote.render);
      jest.useFakeTimers();
    });

    afterEach(() => {
      jest.useRealTimers();
    });

    it('should handle upvote change event', () => {
      const upvoteInput = relevanceVote.render.querySelector(`[data-testid="${testIds.chatItem.vote.upvote}"]`) as HTMLInputElement;

      upvoteInput.checked = true;
      upvoteInput.dispatchEvent(new Event('change'));

      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.CARD_VOTE,
        {
          messageId: 'test-message-456',
          tabId: 'test-tab-123',
          vote: RelevancyVoteType.UP
        }
      );
    });

    it('should replace content with thanks message after upvote', () => {
      const upvoteInput = relevanceVote.render.querySelector(`[data-testid="${testIds.chatItem.vote.upvote}"]`) as HTMLInputElement;

      upvoteInput.checked = true;
      upvoteInput.dispatchEvent(new Event('change'));

      const thanksElement = relevanceVote.render.querySelector(`[data-testid="${testIds.chatItem.vote.thanks}"]`);
      expect(thanksElement).toBeDefined();
      expect(thanksElement?.innerHTML).toBe('Thank you for your feedback!');
    });

    it('should remove component after timeout for upvote', () => {
      const removeSpy = jest.spyOn(relevanceVote.render, 'remove');
      const upvoteInput = relevanceVote.render.querySelector(`[data-testid="${testIds.chatItem.vote.upvote}"]`) as HTMLInputElement;

      upvoteInput.checked = true;
      upvoteInput.dispatchEvent(new Event('change'));

      expect(removeSpy).not.toHaveBeenCalled();

      // Fast-forward time by 3500ms
      jest.advanceTimersByTime(3500);

      expect(removeSpy).toHaveBeenCalled();
    });
  });

  describe('Downvote Functionality', () => {
    beforeEach(() => {
      relevanceVote = new ChatItemRelevanceVote(defaultProps);
      document.body.appendChild(relevanceVote.render);
    });

    it('should handle downvote change event', () => {
      const downvoteInput = relevanceVote.render.querySelector(`[data-testid="${testIds.chatItem.vote.downvote}"]`) as HTMLInputElement;

      downvoteInput.checked = true;
      downvoteInput.dispatchEvent(new Event('change'));

      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.CARD_VOTE,
        {
          messageId: 'test-message-456',
          tabId: 'test-tab-123',
          vote: RelevancyVoteType.DOWN
        }
      );
    });

    it('should replace content with thanks message and report button after downvote', () => {
      const downvoteInput = relevanceVote.render.querySelector(`[data-testid="${testIds.chatItem.vote.downvote}"]`) as HTMLInputElement;

      downvoteInput.checked = true;
      downvoteInput.dispatchEvent(new Event('change'));

      const thanksElement = relevanceVote.render.querySelector(`[data-testid="${testIds.chatItem.vote.thanks}"]`);
      const reportButton = relevanceVote.render.querySelector(`[data-testid="${testIds.chatItem.vote.reportButton}"]`);

      expect(thanksElement).toBeDefined();
      expect(thanksElement?.innerHTML).toBe('Thank you for your feedback!');
      expect(reportButton).toBeDefined();
      expect(reportButton?.textContent).toBe('Report an issue');
    });
  });
});
