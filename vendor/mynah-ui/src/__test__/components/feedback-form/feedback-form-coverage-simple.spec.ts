/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { FeedbackForm } from '../../../components/feedback-form/feedback-form';
import { MynahEventNames, FeedbackPayload, ChatItemFormItem } from '../../../static';
import { MynahUIGlobalEvents } from '../../../helper/events';
import { Config } from '../../../helper/config';
import { MynahUITabsStore } from '../../../helper/tabs-store';
import testIds from '../../../helper/test-ids';

// Mock dependencies
jest.mock('../../../helper/config');
jest.mock('../../../helper/tabs-store');
jest.mock('../../../helper/events');

describe('FeedbackForm Simple Coverage Tests', () => {
  let feedbackForm: FeedbackForm;
  let mockConfig: jest.Mocked<Config>;
  let mockGlobalEvents: jest.Mocked<MynahUIGlobalEvents>;
  let mockTabsStore: jest.Mocked<MynahUITabsStore>;

  const mockFeedbackOptions = [
    { value: 'helpful', label: 'Helpful' },
    { value: 'not-helpful', label: 'Not Helpful' },
    { value: 'inaccurate', label: 'Inaccurate' }
  ];

  const mockTexts = {
    feedbackFormOptionsLabel: 'How was this response?',
    feedbackFormCommentLabel: 'Tell us more (optional)',
    feedbackFormTitle: 'Feedback',
    feedbackFormDescription: 'Help us improve',
    submit: 'Submit',
    cancel: 'Cancel'
  };

  beforeEach(() => {
    document.body.innerHTML = '';

    // Setup Config mock
    mockConfig = {
      config: {
        feedbackOptions: mockFeedbackOptions,
        texts: mockTexts,
        test: true,
        componentClasses: {}
      }
    } as any;
    (Config.getInstance as jest.Mock).mockReturnValue(mockConfig);

    // Setup GlobalEvents mock
    mockGlobalEvents = {
      addListener: jest.fn(),
      dispatch: jest.fn()
    } as any;
    (MynahUIGlobalEvents.getInstance as jest.Mock).mockReturnValue(mockGlobalEvents);

    // Setup TabsStore mock
    mockTabsStore = {
      getTabDataStore: jest.fn().mockReturnValue({ tabId: 'test-tab' })
    } as any;
    (MynahUITabsStore.getInstance as jest.Mock).mockReturnValue(mockTabsStore);
  });

  afterEach(() => {
    document.body.innerHTML = '';
    jest.clearAllMocks();
  });

  describe('InitPayload Comment Optional Chaining Coverage', () => {
    it('should handle undefined comment in initPayload', () => {
      const initPayload: FeedbackPayload = {
        messageId: 'msg-123',
        tabId: 'tab-456',
        selectedOption: 'helpful'
        // comment is undefined - this tests the optional chaining on line 51
      };

      expect(() => {
        feedbackForm = new FeedbackForm({ initPayload });
      }).not.toThrow();

      expect(feedbackForm).toBeDefined();
    });

    it('should handle null comment in initPayload', () => {
      const initPayload: any = {
        messageId: 'msg-123',
        tabId: 'tab-456',
        selectedOption: 'helpful',
        comment: null // This tests the optional chaining
      };

      expect(() => {
        feedbackForm = new FeedbackForm({ initPayload });
      }).not.toThrow();

      expect(feedbackForm).toBeDefined();
    });
  });

  describe('Custom Form Data Optional Chaining Coverage', () => {
    it('should handle custom form data with undefined title and description', () => {
      feedbackForm = new FeedbackForm();

      const customFormData = {
        formItems: [ { id: 'input1', type: 'textinput' } ] as ChatItemFormItem[]
        // title and description are undefined - tests optional chaining on lines 118-125
      };

      const eventData = {
        tabId: 'tab-456',
        customFormData
      };

      const callback = (mockGlobalEvents.addListener as jest.Mock).mock.calls[0][1];

      expect(() => {
        callback(eventData);
      }).not.toThrow();

      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.OPEN_SHEET,
        expect.objectContaining({
          tabId: 'tab-456',
          title: undefined,
          description: undefined
        })
      );
    });

    it('should handle custom form data with null values', () => {
      feedbackForm = new FeedbackForm();

      const customFormData: any = {
        title: null,
        description: null,
        formItems: [ { id: 'input1', type: 'textinput' } ] as ChatItemFormItem[]
      };

      const eventData = {
        tabId: 'tab-456',
        customFormData
      };

      const callback = (mockGlobalEvents.addListener as jest.Mock).mock.calls[0][1];

      expect(() => {
        callback(eventData);
      }).not.toThrow();
    });
  });

  describe('Comment Change Coverage', () => {
    it('should handle comment changes through the feedback comment component', () => {
      feedbackForm = new FeedbackForm();

      // Get the comment component from the form items
      const formItems = feedbackForm.defaultFeedbackFormItems;
      const commentTextarea = formItems.find(item =>
        item.getAttribute != null && item.getAttribute('data-testid') === testIds.feedbackForm.comment
      ) as unknown as HTMLTextAreaElement;

      expect(commentTextarea).toBeDefined();

      // Simulate typing in the comment field
      document.body.appendChild(commentTextarea);
      commentTextarea.value = 'Test comment';
      commentTextarea.dispatchEvent(new KeyboardEvent('keyup'));

      // Submit the form to verify the comment was captured
      const buttonsContainer = feedbackForm.defaultFeedbackFormItems.find(item =>
        item.classList?.contains('mynah-feedback-form-buttons-container')
      );

      document.body.appendChild(buttonsContainer as HTMLElement);
      const submitButton = buttonsContainer?.querySelector(`[data-testid="${testIds.feedbackForm.submitButton}"]`) as HTMLButtonElement;

      submitButton.click();

      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.FEEDBACK_SET,
        expect.objectContaining({
          comment: 'Test comment'
        })
      );
    });
  });

  describe('Edge Cases for Better Coverage', () => {
    it('should handle empty custom form data', () => {
      feedbackForm = new FeedbackForm();

      const eventData = {
        tabId: 'tab-456',
        customFormData: {}
      };

      const callback = (mockGlobalEvents.addListener as jest.Mock).mock.calls[0][1];

      expect(() => {
        callback(eventData);
      }).not.toThrow();

      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.OPEN_SHEET,
        expect.objectContaining({
          tabId: 'tab-456',
          children: []
        })
      );
    });

    it('should handle form with both messageId and customFormData (messageId takes precedence)', () => {
      feedbackForm = new FeedbackForm();

      const eventData = {
        messageId: 'msg-123',
        tabId: 'tab-456',
        customFormData: {
          title: 'Custom Title',
          description: 'Custom Description'
        }
      };

      const callback = (mockGlobalEvents.addListener as jest.Mock).mock.calls[0][1];
      callback(eventData);

      // When messageId is present, it should use the default feedback form
      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.OPEN_SHEET,
        expect.objectContaining({
          tabId: 'tab-456',
          title: mockTexts.feedbackFormTitle,
          description: mockTexts.feedbackFormDescription,
          children: feedbackForm.defaultFeedbackFormItems
        })
      );
    });

    it('should handle multiple form submissions', () => {
      feedbackForm = new FeedbackForm();

      const buttonsContainer = feedbackForm.defaultFeedbackFormItems.find(item =>
        item.classList?.contains('mynah-feedback-form-buttons-container')
      );

      document.body.appendChild(buttonsContainer as HTMLElement);
      const submitButton = buttonsContainer?.querySelector(`[data-testid="${testIds.feedbackForm.submitButton}"]`) as HTMLButtonElement;

      // Submit multiple times
      submitButton.click();
      submitButton.click();

      expect(mockGlobalEvents.dispatch).toHaveBeenCalledTimes(4); // 2 FEEDBACK_SET + 2 CLOSE_SHEET
      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.FEEDBACK_SET,
        expect.any(Object)
      );
      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.CLOSE_SHEET,
        {}
      );
    });

    it('should handle close method multiple times', () => {
      feedbackForm = new FeedbackForm();

      // Call close multiple times
      feedbackForm.close();
      feedbackForm.close();

      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.CLOSE_SHEET,
        {}
      );
    });
  });

  describe('Constructor Variations Coverage', () => {
    it('should create feedback form without props', () => {
      feedbackForm = new FeedbackForm();
      expect(feedbackForm).toBeDefined();
      expect(feedbackForm.defaultFeedbackFormItems).toHaveLength(4);
    });

    it('should create feedback form with empty props', () => {
      feedbackForm = new FeedbackForm({});
      expect(feedbackForm).toBeDefined();
      expect(feedbackForm.defaultFeedbackFormItems).toHaveLength(4);
    });

    it('should create feedback form with partial initPayload', () => {
      const initPayload: Partial<FeedbackPayload> = {
        messageId: 'msg-123'
        // Other fields are undefined
      };

      feedbackForm = new FeedbackForm({ initPayload: initPayload as FeedbackPayload });
      expect(feedbackForm).toBeDefined();
    });
  });
});
