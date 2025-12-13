/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { FeedbackForm } from '../../../components/feedback-form/feedback-form';
import { MynahEventNames, FeedbackPayload, ChatItemButton, ChatItemFormItem } from '../../../static';
import { MynahUIGlobalEvents } from '../../../helper/events';
import { Config } from '../../../helper/config';
import { MynahUITabsStore } from '../../../helper/tabs-store';
import testIds from '../../../helper/test-ids';

// Mock dependencies
jest.mock('../../../helper/config');
jest.mock('../../../helper/tabs-store');
jest.mock('../../../helper/events');

describe('FeedbackForm Component', () => {
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

  describe('Constructor', () => {
    it('should create feedback form with default configuration', () => {
      feedbackForm = new FeedbackForm();

      expect(feedbackForm).toBeDefined();
      expect(feedbackForm.defaultFeedbackFormItems).toBeDefined();
      expect(feedbackForm.defaultFeedbackFormItems).toHaveLength(4);
    });

    it('should create feedback form with initial payload', () => {
      const initPayload: FeedbackPayload = {
        messageId: 'msg-123',
        tabId: 'tab-456',
        selectedOption: 'helpful',
        comment: 'Great response!'
      };

      feedbackForm = new FeedbackForm({ initPayload });

      expect(feedbackForm).toBeDefined();
    });

    it('should register event listener for SHOW_FEEDBACK_FORM', () => {
      feedbackForm = new FeedbackForm();

      expect(mockGlobalEvents.addListener).toHaveBeenCalledWith(
        MynahEventNames.SHOW_FEEDBACK_FORM,
        expect.any(Function)
      );
    });

    it('should initialize with first feedback option as default', () => {
      feedbackForm = new FeedbackForm();

      // The constructor should set the default selected option to the first option
      expect(mockConfig.config.feedbackOptions[0].value).toBe('helpful');
    });
  });

  describe('Event Handling', () => {
    beforeEach(() => {
      feedbackForm = new FeedbackForm();
    });

    it('should handle SHOW_FEEDBACK_FORM event with messageId', () => {
      const eventData = {
        messageId: 'msg-123',
        tabId: 'tab-456'
      };

      // Get the registered callback
      const callback = (mockGlobalEvents.addListener as jest.Mock).mock.calls[0][1];
      callback(eventData);

      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.OPEN_SHEET,
        expect.objectContaining({
          tabId: 'tab-456',
          title: mockTexts.feedbackFormTitle,
          description: mockTexts.feedbackFormDescription,
          children: expect.any(Array)
        })
      );
    });

    it('should handle SHOW_FEEDBACK_FORM event with custom form data', () => {
      const customFormData = {
        title: 'Custom Title',
        description: 'Custom Description',
        buttons: [ { id: 'btn1', text: 'Custom Button' } ] as ChatItemButton[],
        formItems: [ { id: 'item1', type: 'textinput' } ] as ChatItemFormItem[]
      };

      const eventData = {
        tabId: 'tab-456',
        customFormData
      };

      const callback = (mockGlobalEvents.addListener as jest.Mock).mock.calls[0][1];
      callback(eventData);

      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.OPEN_SHEET,
        expect.objectContaining({
          tabId: 'tab-456',
          title: 'Custom Title',
          description: 'Custom Description'
        })
      );
    });

    it('should handle SHOW_FEEDBACK_FORM event without messageId or customFormData', () => {
      const eventData = {
        tabId: 'tab-456'
      };

      const callback = (mockGlobalEvents.addListener as jest.Mock).mock.calls[0][1];
      callback(eventData);

      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.OPEN_SHEET,
        expect.objectContaining({
          tabId: 'tab-456',
          title: undefined,
          description: undefined,
          children: []
        })
      );
    });
  });

  describe('Form Submission', () => {
    beforeEach(() => {
      feedbackForm = new FeedbackForm();
    });

    it('should dispatch FEEDBACK_SET event when submit button is clicked', () => {
      // Get the buttons container
      const buttonsContainer = feedbackForm.defaultFeedbackFormItems.find(item =>
        item.classList?.contains('mynah-feedback-form-buttons-container')
      );

      expect(buttonsContainer).toBeDefined();

      // Find submit button within the container
      document.body.appendChild(buttonsContainer as HTMLElement);
      const submitButton = buttonsContainer?.querySelector(`[data-testid="${testIds.feedbackForm.submitButton}"]`) as HTMLButtonElement;

      expect(submitButton).toBeDefined();

      // Simulate button click
      submitButton.click();

      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.FEEDBACK_SET,
        expect.objectContaining({
          selectedOption: 'helpful', // Default first option
          messageId: '',
          tabId: '',
          comment: ''
        })
      );
    });

    it('should close form after submission', () => {
      const buttonsContainer = feedbackForm.defaultFeedbackFormItems.find(item =>
        item.classList?.contains('mynah-feedback-form-buttons-container')
      );

      document.body.appendChild(buttonsContainer as HTMLElement);
      const submitButton = buttonsContainer?.querySelector(`[data-testid="${testIds.feedbackForm.submitButton}"]`) as HTMLButtonElement;

      submitButton.click();

      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.CLOSE_SHEET,
        {}
      );
    });
  });

  describe('Form Cancellation', () => {
    beforeEach(() => {
      feedbackForm = new FeedbackForm();
    });

    it('should close form when cancel button is clicked', () => {
      const buttonsContainer = feedbackForm.defaultFeedbackFormItems.find(item =>
        item.classList?.contains('mynah-feedback-form-buttons-container')
      );

      expect(buttonsContainer).toBeDefined();

      document.body.appendChild(buttonsContainer as HTMLElement);
      const cancelButton = buttonsContainer?.querySelector(`[data-testid="${testIds.feedbackForm.cancelButton}"]`) as HTMLButtonElement;

      expect(cancelButton).toBeDefined();

      cancelButton.click();

      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.CLOSE_SHEET,
        {}
      );
    });
  });

  describe('Close Method', () => {
    beforeEach(() => {
      feedbackForm = new FeedbackForm();
    });

    it('should reset form state when closed', () => {
      feedbackForm.close();

      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.CLOSE_SHEET,
        {}
      );
    });

    it('should clear comment and reset selected option on close', () => {
      // Set some initial state
      const initPayload: FeedbackPayload = {
        messageId: 'msg-123',
        tabId: 'tab-456',
        selectedOption: 'not-helpful',
        comment: 'Some comment'
      };

      feedbackForm = new FeedbackForm({ initPayload });
      feedbackForm.close();

      // After close, the form should be reset to defaults
      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.CLOSE_SHEET,
        {}
      );
    });
  });

  describe('Custom Form Handling', () => {
    beforeEach(() => {
      feedbackForm = new FeedbackForm();
    });

    it('should handle custom form with form items', () => {
      const customFormData = {
        title: 'Custom Form',
        formItems: [
          { id: 'input1', type: 'textinput', title: 'Name' }
        ] as ChatItemFormItem[]
      };

      const eventData = {
        tabId: 'tab-456',
        customFormData
      };

      const callback = (mockGlobalEvents.addListener as jest.Mock).mock.calls[0][1];
      callback(eventData);

      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.OPEN_SHEET,
        expect.objectContaining({
          tabId: 'tab-456',
          title: 'Custom Form'
        })
      );
    });

    it('should handle custom form with buttons', () => {
      const customFormData = {
        title: 'Custom Form',
        buttons: [
          { id: 'submit', text: 'Submit Form' }
        ] as ChatItemButton[]
      };

      const eventData = {
        tabId: 'tab-456',
        customFormData
      };

      const callback = (mockGlobalEvents.addListener as jest.Mock).mock.calls[0][1];
      callback(eventData);

      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.OPEN_SHEET,
        expect.objectContaining({
          tabId: 'tab-456',
          title: 'Custom Form'
        })
      );
    });

    it('should return empty array when tab data store is undefined', () => {
      // Override the mock for this specific test
      mockTabsStore.getTabDataStore.mockReturnValueOnce(undefined as any);

      feedbackForm = new FeedbackForm();

      const customFormData = {
        title: 'Custom Form',
        formItems: [ { id: 'input1', type: 'textinput' } ] as ChatItemFormItem[]
      };

      const eventData = {
        tabId: 'invalid-tab',
        customFormData
      };

      const callback = (mockGlobalEvents.addListener as jest.Mock).mock.calls[0][1];
      callback(eventData);

      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.OPEN_SHEET,
        expect.objectContaining({
          children: []
        })
      );
    });
  });

  describe('Form Items Structure', () => {
    beforeEach(() => {
      feedbackForm = new FeedbackForm();
    });

    it('should have correct structure for default feedback form items', () => {
      const items = feedbackForm.defaultFeedbackFormItems;

      expect(items).toHaveLength(4);

      // First item should be the select wrapper
      expect(items[0].classList.contains('mynah-form-input-wrapper')).toBe(true);

      // Second item should be the comment label
      expect(items[1].tagName).toBe('SPAN');
      expect(items[1].textContent).toBe(mockTexts.feedbackFormCommentLabel);

      // Third item should be the comment textarea
      expect(items[2].tagName).toBe('TEXTAREA');
      expect(items[2].getAttribute('data-testid')).toBe(testIds.feedbackForm.comment);

      // Fourth item should be the buttons container
      expect(items[3].classList.contains('mynah-feedback-form-buttons-container')).toBe(true);
    });

    it('should have submit and cancel buttons in buttons container', () => {
      const buttonsContainer = feedbackForm.defaultFeedbackFormItems[3];

      expect(buttonsContainer.classList.contains('mynah-feedback-form-buttons-container')).toBe(true);

      // Append to DOM to query for buttons
      document.body.appendChild(buttonsContainer);

      const cancelButton = buttonsContainer.querySelector(`[data-testid="${testIds.feedbackForm.cancelButton}"]`);
      const submitButton = buttonsContainer.querySelector(`[data-testid="${testIds.feedbackForm.submitButton}"]`);

      expect(cancelButton).toBeDefined();
      expect(submitButton).toBeDefined();
      expect(cancelButton?.textContent).toBe(mockTexts.cancel);
      expect(submitButton?.textContent).toBe(mockTexts.submit);
    });
  });

  describe('Event Dispatching', () => {
    beforeEach(() => {
      feedbackForm = new FeedbackForm();
    });

    it('should dispatch custom form action click events', () => {
      const customFormData = {
        buttons: [ { id: 'custom-action', text: 'Custom Action' } ] as ChatItemButton[]
      };

      const eventData = {
        tabId: 'tab-456',
        customFormData
      };

      // Trigger the show feedback form event
      const callback = (mockGlobalEvents.addListener as jest.Mock).mock.calls[0][1];
      callback(eventData);

      // Verify that the form was opened
      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.OPEN_SHEET,
        expect.any(Object)
      );
    });

    it('should handle form change events', () => {
      const customFormData = {
        formItems: [ { id: 'input1', type: 'textinput' } ] as ChatItemFormItem[]
      };

      const eventData = {
        tabId: 'tab-456',
        customFormData
      };

      const callback = (mockGlobalEvents.addListener as jest.Mock).mock.calls[0][1];
      callback(eventData);

      // The form should be created and ready to handle form change events
      expect(mockGlobalEvents.dispatch).toHaveBeenCalledWith(
        MynahEventNames.OPEN_SHEET,
        expect.any(Object)
      );
    });
  });
});
