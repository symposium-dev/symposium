/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { FeedbackForm } from '../../../components/feedback-form/feedback-form';
import { FeedbackFormComment } from '../../../components/feedback-form/feedback-form-comment';
import { MynahEventNames, FeedbackPayload } from '../../../static';
import { MynahUIGlobalEvents } from '../../../helper/events';
import { Config } from '../../../helper/config';
import { MynahUITabsStore } from '../../../helper/tabs-store';
import testIds from '../../../helper/test-ids';

// Mock dependencies
jest.mock('../../../helper/config');
jest.mock('../../../helper/tabs-store');
jest.mock('../../../helper/events');

describe('FeedbackForm Integration Tests', () => {
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
        test: true, // Enable test mode for testId attributes
        componentClasses: {} // Add componentClasses to prevent Select component errors
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

  describe('Complete Feedback Flow', () => {
    it('should handle complete feedback submission flow', () => {
      // Create feedback form
      feedbackForm = new FeedbackForm();

      // Simulate showing feedback form
      const showFeedbackData = {
        messageId: 'msg-123',
        tabId: 'tab-456'
      };

      const showFeedbackCallback = (mockGlobalEvents.addListener as jest.Mock).mock.calls[0][1];
      showFeedbackCallback(showFeedbackData);

      // Verify form was opened
      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.OPEN_SHEET,
        expect.objectContaining({
          tabId: 'tab-456',
          title: mockTexts.feedbackFormTitle,
          description: mockTexts.feedbackFormDescription
        })
      );

      // Get the form items
      const formItems = feedbackForm.defaultFeedbackFormItems;

      // Find comment textarea
      const commentTextarea = formItems.find(item =>
        item.getAttribute != null && item.getAttribute('data-testid') === testIds.feedbackForm.comment
      ) as unknown as HTMLTextAreaElement;

      expect(commentTextarea).toBeDefined();

      // Simulate user typing in comment
      document.body.appendChild(commentTextarea);
      commentTextarea.value = 'This is my feedback comment';
      commentTextarea.dispatchEvent(new KeyboardEvent('keyup'));

      // Find and click submit button
      const buttonsContainer = formItems.find(item =>
        item.classList?.contains('mynah-feedback-form-buttons-container')
      );

      expect(buttonsContainer).toBeDefined();

      document.body.appendChild(buttonsContainer as HTMLElement);
      const submitButton = buttonsContainer?.querySelector(`[data-testid="${testIds.feedbackForm.submitButton}"]`) as HTMLButtonElement;

      expect(submitButton).toBeDefined();

      submitButton.click();

      // Verify feedback was submitted
      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.FEEDBACK_SET,
        expect.objectContaining({
          messageId: 'msg-123',
          tabId: 'tab-456',
          selectedOption: 'helpful', // Default first option
          comment: 'This is my feedback comment'
        })
      );

      // Verify form was closed
      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.CLOSE_SHEET,
        {}
      );
    });

    it('should handle feedback cancellation flow', () => {
      feedbackForm = new FeedbackForm();

      // Simulate showing feedback form
      const showFeedbackData = {
        messageId: 'msg-123',
        tabId: 'tab-456'
      };

      const showFeedbackCallback = (mockGlobalEvents.addListener as jest.Mock).mock.calls[0][1];
      showFeedbackCallback(showFeedbackData);

      // Get form items and add comment
      const formItems = feedbackForm.defaultFeedbackFormItems;
      const commentTextarea = formItems.find(item =>
        item.getAttribute != null && item.getAttribute('data-testid') === testIds.feedbackForm.comment
      ) as unknown as HTMLTextAreaElement;

      document.body.appendChild(commentTextarea);
      commentTextarea.value = 'This comment should be cleared';
      commentTextarea.dispatchEvent(new KeyboardEvent('keyup'));

      // Find and click cancel button
      const buttonsContainer = formItems.find(item =>
        item.classList?.contains('mynah-feedback-form-buttons-container')
      );

      expect(buttonsContainer).toBeDefined();

      document.body.appendChild(buttonsContainer as HTMLElement);
      const cancelButton = buttonsContainer?.querySelector(`[data-testid="${testIds.feedbackForm.cancelButton}"]`) as HTMLButtonElement;

      expect(cancelButton).toBeDefined();

      cancelButton.click();

      // Verify form was closed without submitting feedback
      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.CLOSE_SHEET,
        {}
      );

      // Verify FEEDBACK_SET was not called
      expect(mockGlobalEvents.dispatch).not.toHaveBeenCalledWith(
        MynahEventNames.FEEDBACK_SET,
        expect.any(Object)
      );
    });

    it('should handle feedback form with initial payload', () => {
      const initPayload: FeedbackPayload = {
        messageId: 'existing-msg',
        tabId: 'existing-tab',
        selectedOption: 'not-helpful',
        comment: 'Initial comment'
      };

      feedbackForm = new FeedbackForm({ initPayload });

      // Get comment component and verify initial value
      const formItems = feedbackForm.defaultFeedbackFormItems;
      const commentTextarea = formItems.find(item =>
        item.getAttribute != null && item.getAttribute('data-testid') === testIds.feedbackForm.comment
      ) as unknown as HTMLTextAreaElement;

      expect(commentTextarea.value).toBe('Initial comment');

      // Submit the form
      const buttonsContainer = formItems.find(item =>
        item.classList?.contains('mynah-feedback-form-buttons-container')
      );

      document.body.appendChild(buttonsContainer as HTMLElement);
      const submitButton = buttonsContainer?.querySelector(`[data-testid="${testIds.feedbackForm.submitButton}"]`) as HTMLButtonElement;

      submitButton.click();

      // Verify the initial payload values are used
      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.FEEDBACK_SET,
        expect.objectContaining({
          selectedOption: 'not-helpful',
          comment: 'Initial comment'
        })
      );
    });
  });

  describe('Comment Component Integration', () => {
    it('should properly integrate comment component with feedback form', () => {
      feedbackForm = new FeedbackForm();

      const formItems = feedbackForm.defaultFeedbackFormItems;
      const commentTextarea = formItems.find(item =>
        item.getAttribute != null && item.getAttribute('data-testid') === testIds.feedbackForm.comment
      ) as unknown as HTMLTextAreaElement;

      // Verify comment component is properly integrated
      expect(commentTextarea).toBeDefined();
      expect(commentTextarea.tagName).toBe('TEXTAREA');
      expect(commentTextarea.classList.contains('mynah-feedback-form-comment')).toBe(true);

      // Test comment functionality
      document.body.appendChild(commentTextarea);
      commentTextarea.value = 'Test comment';

      // Create a FeedbackFormComment instance to test methods
      const commentComponent = new FeedbackFormComment({
        initComment: 'Test comment'
      });

      expect(commentComponent.getComment()).toBe('Test comment');

      commentComponent.clear();
      expect(commentComponent.getComment()).toBe('');
    });

    it('should handle comment changes and form submission together', () => {
      let capturedComment = '';

      // Create comment component with onChange
      const commentComponent = new FeedbackFormComment({
        onChange: (comment) => {
          capturedComment = comment;
        }
      });

      document.body.appendChild(commentComponent.render);

      // Simulate user typing
      commentComponent.render.value = 'User feedback';
      commentComponent.render.dispatchEvent(new KeyboardEvent('keyup'));

      expect(capturedComment).toBe('User feedback');

      // Test clearing
      commentComponent.clear();
      expect(commentComponent.getComment()).toBe('');
    });
  });

  describe('Form State Management', () => {
    it('should maintain form state throughout interaction', () => {
      feedbackForm = new FeedbackForm();

      // Show feedback form
      const showFeedbackData = {
        messageId: 'msg-123',
        tabId: 'tab-456'
      };

      const showFeedbackCallback = (mockGlobalEvents.addListener as jest.Mock).mock.calls[0][1];
      showFeedbackCallback(showFeedbackData);

      // Modify form state
      const formItems = feedbackForm.defaultFeedbackFormItems;
      const commentTextarea = formItems.find(item =>
        item.getAttribute != null && item.getAttribute('data-testid') === testIds.feedbackForm.comment
      ) as unknown as HTMLTextAreaElement;

      document.body.appendChild(commentTextarea);
      commentTextarea.value = 'Modified comment';
      commentTextarea.dispatchEvent(new KeyboardEvent('keyup'));

      // Submit form
      const buttonsContainer = formItems.find(item =>
        item.classList?.contains('mynah-feedback-form-buttons-container')
      );

      document.body.appendChild(buttonsContainer as HTMLElement);
      const submitButton = buttonsContainer?.querySelector(`[data-testid="${testIds.feedbackForm.submitButton}"]`) as HTMLButtonElement;

      submitButton.click();

      // Verify state was maintained and submitted
      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.FEEDBACK_SET,
        expect.objectContaining({
          messageId: 'msg-123',
          tabId: 'tab-456',
          comment: 'Modified comment'
        })
      );
    });

    it('should reset form state after close', () => {
      const initPayload: FeedbackPayload = {
        messageId: 'msg-123',
        tabId: 'tab-456',
        selectedOption: 'not-helpful',
        comment: 'Some comment'
      };

      feedbackForm = new FeedbackForm({ initPayload });

      // Close the form
      feedbackForm.close();

      // Verify close event was dispatched
      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.CLOSE_SHEET,
        {}
      );

      // Verify form items are reset
      const formItems = feedbackForm.defaultFeedbackFormItems;
      const commentTextarea = formItems.find(item =>
        item.getAttribute != null && item.getAttribute('data-testid') === testIds.feedbackForm.comment
      ) as unknown as HTMLTextAreaElement;

      expect(commentTextarea.value).toBe('');
    });
  });

  describe('Error Handling', () => {
    it('should handle missing tab data gracefully', () => {
      // Override the mock for this specific test
      mockTabsStore.getTabDataStore.mockReturnValueOnce(undefined as any);

      feedbackForm = new FeedbackForm();

      const customFormData = {
        title: 'Custom Form',
        formItems: [ { id: 'input1', type: 'textinput' } ] as any[]
      };

      const eventData = {
        tabId: 'invalid-tab',
        customFormData
      };

      const callback = (mockGlobalEvents.addListener as jest.Mock).mock.calls[0][1];

      // Should not throw error
      expect(() => {
        callback(eventData);
      }).not.toThrow();

      // Should open sheet with empty children
      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.OPEN_SHEET,
        expect.objectContaining({
          children: []
        })
      );
    });

    it('should handle form submission without comment gracefully', () => {
      feedbackForm = new FeedbackForm();

      const formItems = feedbackForm.defaultFeedbackFormItems;
      const buttonsContainer = formItems.find(item =>
        item.classList?.contains('mynah-feedback-form-buttons-container')
      );

      document.body.appendChild(buttonsContainer as HTMLElement);
      const submitButton = buttonsContainer?.querySelector(`[data-testid="${testIds.feedbackForm.submitButton}"]`) as HTMLButtonElement;

      // Submit without adding comment
      expect(() => {
        submitButton.click();
      }).not.toThrow();

      // Should submit with empty comment
      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.FEEDBACK_SET,
        expect.objectContaining({
          comment: ''
        })
      );
    });
  });
});
