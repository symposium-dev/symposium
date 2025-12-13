/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { FeedbackFormComment } from '../../../components/feedback-form/feedback-form-comment';
import { Config } from '../../../helper/config';
import testIds from '../../../helper/test-ids';

// Mock Config
jest.mock('../../../helper/config');

describe('FeedbackFormComment Component', () => {
  let feedbackFormComment: FeedbackFormComment;
  let mockOnChange: jest.Mock;
  let mockConfig: jest.Mocked<Config>;

  beforeEach(() => {
    document.body.innerHTML = '';
    mockOnChange = jest.fn();

    // Setup Config mock to enable test mode
    mockConfig = {
      config: {
        test: true
      }
    } as any;
    (Config.getInstance as jest.Mock).mockReturnValue(mockConfig);
  });

  afterEach(() => {
    document.body.innerHTML = '';
    jest.clearAllMocks();
  });

  describe('Constructor', () => {
    it('should create feedback form comment with default props', () => {
      feedbackFormComment = new FeedbackFormComment({});

      expect(feedbackFormComment.render).toBeDefined();
      expect(feedbackFormComment.render.tagName).toBe('TEXTAREA');
      expect(feedbackFormComment.render.classList.contains('mynah-feedback-form-comment')).toBe(true);
      expect(feedbackFormComment.render.getAttribute('data-testid')).toBe(testIds.feedbackForm.comment);
    });

    it('should create feedback form comment with initial comment', () => {
      const initComment = 'Initial feedback comment';
      feedbackFormComment = new FeedbackFormComment({ initComment });

      expect(feedbackFormComment.render.value).toBe(initComment);
      expect(feedbackFormComment.getComment()).toBe(initComment);
    });

    it('should create feedback form comment with onChange callback', () => {
      feedbackFormComment = new FeedbackFormComment({ onChange: mockOnChange });

      expect(feedbackFormComment.render).toBeDefined();
      expect(mockOnChange).not.toHaveBeenCalled();
    });

    it('should create feedback form comment with both initial comment and onChange', () => {
      const initComment = 'Test comment';
      feedbackFormComment = new FeedbackFormComment({
        initComment,
        onChange: mockOnChange
      });

      expect(feedbackFormComment.render.value).toBe(initComment);
      expect(feedbackFormComment.getComment()).toBe(initComment);
      expect(mockOnChange).not.toHaveBeenCalled();
    });
  });

  describe('Event Handling', () => {
    it('should call onChange when keyup event is triggered', () => {
      feedbackFormComment = new FeedbackFormComment({ onChange: mockOnChange });
      document.body.appendChild(feedbackFormComment.render);

      const testValue = 'New comment text';
      feedbackFormComment.render.value = testValue;

      const keyupEvent = new KeyboardEvent('keyup', { key: 'a' });
      feedbackFormComment.render.dispatchEvent(keyupEvent);

      expect(mockOnChange).toHaveBeenCalledWith(testValue);
      expect(mockOnChange).toHaveBeenCalledTimes(1);
    });

    it('should call onChange multiple times for multiple keyup events', () => {
      feedbackFormComment = new FeedbackFormComment({ onChange: mockOnChange });
      document.body.appendChild(feedbackFormComment.render);

      // First keyup
      feedbackFormComment.render.value = 'First';
      feedbackFormComment.render.dispatchEvent(new KeyboardEvent('keyup', { key: 'a' }));

      // Second keyup
      feedbackFormComment.render.value = 'First Second';
      feedbackFormComment.render.dispatchEvent(new KeyboardEvent('keyup', { key: 'b' }));

      expect(mockOnChange).toHaveBeenCalledTimes(2);
      expect(mockOnChange).toHaveBeenNthCalledWith(1, 'First');
      expect(mockOnChange).toHaveBeenNthCalledWith(2, 'First Second');
    });

    it('should not throw error when onChange is not provided and keyup is triggered', () => {
      feedbackFormComment = new FeedbackFormComment({});
      document.body.appendChild(feedbackFormComment.render);

      feedbackFormComment.render.value = 'Test value';

      expect(() => {
        feedbackFormComment.render.dispatchEvent(new KeyboardEvent('keyup', { key: 'a' }));
      }).not.toThrow();
    });

    it('should handle empty string values in onChange', () => {
      feedbackFormComment = new FeedbackFormComment({ onChange: mockOnChange });
      document.body.appendChild(feedbackFormComment.render);

      feedbackFormComment.render.value = '';
      feedbackFormComment.render.dispatchEvent(new KeyboardEvent('keyup', { key: 'Backspace' }));

      expect(mockOnChange).toHaveBeenCalledWith('');
    });
  });

  describe('Methods', () => {
    beforeEach(() => {
      feedbackFormComment = new FeedbackFormComment({ initComment: 'Initial comment' });
    });

    it('should return current comment value with getComment', () => {
      expect(feedbackFormComment.getComment()).toBe('Initial comment');

      feedbackFormComment.render.value = 'Updated comment';
      expect(feedbackFormComment.getComment()).toBe('Updated comment');
    });

    it('should clear comment value with clear method', () => {
      expect(feedbackFormComment.getComment()).toBe('Initial comment');

      feedbackFormComment.clear();

      expect(feedbackFormComment.getComment()).toBe('');
      expect(feedbackFormComment.render.value).toBe('');
    });

    it('should clear empty comment without issues', () => {
      feedbackFormComment = new FeedbackFormComment({});

      expect(feedbackFormComment.getComment()).toBe('');

      feedbackFormComment.clear();

      expect(feedbackFormComment.getComment()).toBe('');
    });
  });

  describe('DOM Structure', () => {
    it('should have correct HTML attributes', () => {
      const initComment = 'Test comment';
      feedbackFormComment = new FeedbackFormComment({ initComment });

      expect(feedbackFormComment.render.tagName).toBe('TEXTAREA');
      expect(feedbackFormComment.render.getAttribute('data-testid')).toBe(testIds.feedbackForm.comment);
      expect(feedbackFormComment.render.classList.contains('mynah-feedback-form-comment')).toBe(true);
      expect(feedbackFormComment.render.value).toBe(initComment);
    });

    it('should be focusable', () => {
      feedbackFormComment = new FeedbackFormComment({});
      document.body.appendChild(feedbackFormComment.render);

      feedbackFormComment.render.focus();
      expect(document.activeElement).toBe(feedbackFormComment.render);
    });

    it('should handle special characters in initial comment', () => {
      const specialComment = 'Comment with "quotes" and <tags> & symbols';
      feedbackFormComment = new FeedbackFormComment({ initComment: specialComment });

      expect(feedbackFormComment.getComment()).toBe(specialComment);
      expect(feedbackFormComment.render.value).toBe(specialComment);
    });
  });

  describe('Integration with onChange', () => {
    it('should trigger onChange with correct value after programmatic value change', () => {
      feedbackFormComment = new FeedbackFormComment({ onChange: mockOnChange });
      document.body.appendChild(feedbackFormComment.render);

      // Simulate user typing
      const newValue = 'User typed this';
      feedbackFormComment.render.value = newValue;
      feedbackFormComment.render.dispatchEvent(new KeyboardEvent('keyup'));

      expect(mockOnChange).toHaveBeenCalledWith(newValue);
    });

    it('should work with onChange callback that modifies external state', () => {
      let externalState = '';
      const stateUpdater = (value: string): void => {
        externalState = value.toUpperCase();
      };

      feedbackFormComment = new FeedbackFormComment({ onChange: stateUpdater });
      document.body.appendChild(feedbackFormComment.render);

      feedbackFormComment.render.value = 'test input';
      feedbackFormComment.render.dispatchEvent(new KeyboardEvent('keyup'));

      expect(externalState).toBe('TEST INPUT');
    });
  });
});
