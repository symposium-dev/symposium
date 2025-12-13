/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { TextArea, TextAreaInternal, TextAreaProps } from '../../../components/form-items/text-area';
import { DomBuilder } from '../../../helper/dom';

// Mock the validator helper
jest.mock('../../../helper/validator', () => ({
  checkTextElementValidation: jest.fn()
}));

describe('TextArea Component', () => {
  let textArea: TextAreaInternal;
  let mockOnChange: jest.Mock;
  let mockOnKeyPress: jest.Mock;
  let mockFireModifierAndEnterKeyPress: jest.Mock;

  beforeEach(() => {
    document.body.innerHTML = '';
    mockOnChange = jest.fn();
    mockOnKeyPress = jest.fn();
    mockFireModifierAndEnterKeyPress = jest.fn();
    jest.clearAllMocks();
  });

  afterEach(() => {
    document.body.innerHTML = '';
  });

  describe('TextAreaInternal', () => {
    it('should create text area with default props', () => {
      textArea = new TextAreaInternal({});

      expect(textArea.render).toBeDefined();
      expect(textArea.render.classList.contains('mynah-form-input-wrapper')).toBe(true);
      expect(textArea.getValue()).toBe('');
    });

    it('should create text area with initial value', () => {
      const initialValue = 'test value';
      textArea = new TextAreaInternal({ value: initialValue });

      expect(textArea.getValue()).toBe(initialValue);
    });

    it('should create text area with label', () => {
      const label = 'Test Label';
      textArea = new TextAreaInternal({ label });

      document.body.appendChild(textArea.render);
      const labelElement = document.body.querySelector('.mynah-form-input-label');
      expect(labelElement?.textContent).toBe(label);
    });

    it('should create text area with placeholder', () => {
      const placeholder = 'Enter text here';
      textArea = new TextAreaInternal({ placeholder });

      document.body.appendChild(textArea.render);
      const textAreaElement = document.body.querySelector('textarea') as HTMLTextAreaElement;
      expect(textAreaElement.placeholder).toBe(placeholder);
    });

    it('should create text area with custom class names', () => {
      const customClasses = [ 'custom-class-1', 'custom-class-2' ];
      textArea = new TextAreaInternal({ classNames: customClasses });

      document.body.appendChild(textArea.render);
      const textAreaElement = document.body.querySelector('textarea');
      expect(textAreaElement?.classList.contains('custom-class-1')).toBe(true);
      expect(textAreaElement?.classList.contains('custom-class-2')).toBe(true);
    });

    it('should create text area with custom attributes', () => {
      const attributes = { 'data-test': 'test-value', 'aria-label': 'test-textarea' };
      textArea = new TextAreaInternal({ attributes });

      document.body.appendChild(textArea.render);
      const container = document.body.querySelector('.mynah-form-input-container');
      expect(container?.getAttribute('data-test')).toBe('test-value');
      expect(container?.getAttribute('aria-label')).toBe('test-textarea');
    });

    it('should create text area with test ID', () => {
      const testId = 'test-textarea-id';
      textArea = new TextAreaInternal({ testId });

      document.body.appendChild(textArea.render);
      const textAreaElement = document.body.querySelector(`[data-testid="${testId}"]`);
      expect(textAreaElement).toBeDefined();
    });

    it('should handle autoFocus', (done) => {
      textArea = new TextAreaInternal({ autoFocus: true });
      document.body.appendChild(textArea.render);

      const textAreaElement = document.body.querySelector('textarea') as HTMLTextAreaElement;

      // Check that autofocus attribute is set
      expect(textAreaElement.hasAttribute('autofocus')).toBe(true);

      // Check that focus is called after timeout
      setTimeout(() => {
        expect(document.activeElement).toBe(textAreaElement);
        done();
      }, 300);
    });

    it('should handle setValue and getValue methods', () => {
      textArea = new TextAreaInternal({});

      const testValue = 'test value';
      textArea.setValue(testValue);
      expect(textArea.getValue()).toBe(testValue);
    });

    it('should handle setEnabled method', () => {
      textArea = new TextAreaInternal({});
      document.body.appendChild(textArea.render);

      const textAreaElement = document.body.querySelector('textarea') as HTMLTextAreaElement;

      // Test disabling
      textArea.setEnabled(false);
      expect(textAreaElement.hasAttribute('disabled')).toBe(true);

      // Test enabling
      textArea.setEnabled(true);
      expect(textAreaElement.hasAttribute('disabled')).toBe(false);
    });

    it('should trigger onChange on keyup event', () => {
      textArea = new TextAreaInternal({ onChange: mockOnChange });
      document.body.appendChild(textArea.render);

      const textAreaElement = document.body.querySelector('textarea') as HTMLTextAreaElement;
      const testValue = 'test input';

      // Set value and trigger keyup event with proper target
      textAreaElement.value = testValue;
      const keyupEvent = new KeyboardEvent('keyup', { bubbles: true });
      Object.defineProperty(keyupEvent, 'currentTarget', {
        value: textAreaElement,
        enumerable: true
      });
      textAreaElement.dispatchEvent(keyupEvent);

      expect(mockOnChange).toHaveBeenCalledWith(testValue);
    });

    it('should trigger onKeyPress on keypress event', () => {
      textArea = new TextAreaInternal({ onKeyPress: mockOnKeyPress });
      document.body.appendChild(textArea.render);

      const textAreaElement = document.body.querySelector('textarea') as HTMLTextAreaElement;
      const keyEvent = new KeyboardEvent('keypress', { key: 'a' });

      textAreaElement.dispatchEvent(keyEvent);

      expect(mockOnKeyPress).toHaveBeenCalledWith(keyEvent);
    });

    it('should trigger fireModifierAndEnterKeyPress on Ctrl+Enter', () => {
      textArea = new TextAreaInternal({ fireModifierAndEnterKeyPress: mockFireModifierAndEnterKeyPress });
      document.body.appendChild(textArea.render);

      const textAreaElement = document.body.querySelector('textarea') as HTMLTextAreaElement;
      const keyEvent = new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true });

      textAreaElement.dispatchEvent(keyEvent);

      expect(mockFireModifierAndEnterKeyPress).toHaveBeenCalled();
    });

    it('should trigger fireModifierAndEnterKeyPress on Cmd+Enter (Mac)', () => {
      textArea = new TextAreaInternal({ fireModifierAndEnterKeyPress: mockFireModifierAndEnterKeyPress });
      document.body.appendChild(textArea.render);

      const textAreaElement = document.body.querySelector('textarea') as HTMLTextAreaElement;
      const keyEvent = new KeyboardEvent('keydown', { key: 'Enter', metaKey: true });

      textAreaElement.dispatchEvent(keyEvent);

      expect(mockFireModifierAndEnterKeyPress).toHaveBeenCalled();
    });

    it('should not trigger fireModifierAndEnterKeyPress on Enter without modifier', () => {
      textArea = new TextAreaInternal({ fireModifierAndEnterKeyPress: mockFireModifierAndEnterKeyPress });
      document.body.appendChild(textArea.render);

      const textAreaElement = document.body.querySelector('textarea') as HTMLTextAreaElement;
      const keyEvent = new KeyboardEvent('keydown', { key: 'Enter' });

      textAreaElement.dispatchEvent(keyEvent);

      expect(mockFireModifierAndEnterKeyPress).not.toHaveBeenCalled();
    });

    it('should call checkValidation on blur event', () => {
      const checkTextElementValidation = jest.requireMock('../../../helper/validator').checkTextElementValidation;
      textArea = new TextAreaInternal({});
      document.body.appendChild(textArea.render);

      const textAreaElement = document.body.querySelector('textarea') as HTMLTextAreaElement;
      textAreaElement.dispatchEvent(new Event('blur'));

      expect(checkTextElementValidation).toHaveBeenCalled();
    });

    it('should call checkValidation on keyup event', () => {
      const checkTextElementValidation = jest.requireMock('../../../helper/validator').checkTextElementValidation;
      textArea = new TextAreaInternal({});
      document.body.appendChild(textArea.render);

      const textAreaElement = document.body.querySelector('textarea') as HTMLTextAreaElement;
      const keyupEvent = new KeyboardEvent('keyup', { bubbles: true });
      Object.defineProperty(keyupEvent, 'currentTarget', {
        value: textAreaElement,
        enumerable: true
      });
      textAreaElement.dispatchEvent(keyupEvent);

      expect(checkTextElementValidation).toHaveBeenCalled();
    });

    it('should handle validation patterns', () => {
      const validationPatterns = {
        operator: 'and' as const,
        patterns: [ { pattern: /^[^\s@]+@[^\s@]+\.[^\s@]+$/, errorMessage: 'Invalid email' } ]
      };

      textArea = new TextAreaInternal({ validationPatterns });
      expect(textArea.render).toBeDefined();
    });

    it('should handle mandatory field', () => {
      textArea = new TextAreaInternal({ mandatory: true });
      expect(textArea.render).toBeDefined();
    });

    it('should handle description element', () => {
      const descriptionElement = DomBuilder.getInstance().build({
        type: 'div',
        children: [ 'Test description' ],
        classNames: [ 'test-description' ]
      });

      textArea = new TextAreaInternal({ description: descriptionElement });
      document.body.appendChild(textArea.render);

      const description = document.body.querySelector('.test-description');
      expect(description?.textContent).toBe('Test description');
    });

    it('should have validation error block', () => {
      textArea = new TextAreaInternal({});
      document.body.appendChild(textArea.render);

      const errorBlock = document.body.querySelector('.mynah-form-input-validation-error-block');
      expect(errorBlock).toBeDefined();
    });

    it('should create textarea element with correct tag', () => {
      textArea = new TextAreaInternal({});
      document.body.appendChild(textArea.render);

      const textAreaElement = document.body.querySelector('textarea');
      expect(textAreaElement).toBeDefined();
      expect(textAreaElement?.tagName.toLowerCase()).toBe('textarea');
    });

    it('should have mynah-form-input class', () => {
      textArea = new TextAreaInternal({});
      document.body.appendChild(textArea.render);

      const textAreaElement = document.body.querySelector('textarea');
      expect(textAreaElement?.classList.contains('mynah-form-input')).toBe(true);
    });
  });

  describe('TextArea Factory', () => {
    it('should create TextAreaInternal by default', () => {
      const textAreaFactory = new TextArea({});
      expect(textAreaFactory).toBeInstanceOf(TextAreaInternal);
    });

    it('should have abstract methods', () => {
      const textAreaFactory = new TextArea({});
      expect(typeof textAreaFactory.setValue).toBe('function');
      expect(typeof textAreaFactory.getValue).toBe('function');
      expect(typeof textAreaFactory.setEnabled).toBe('function');
      expect(typeof textAreaFactory.checkValidation).toBe('function');
    });
  });

  describe('Edge Cases', () => {
    it('should handle empty props object', () => {
      textArea = new TextAreaInternal({});
      expect(textArea.render).toBeDefined();
      expect(textArea.getValue()).toBe('');
    });

    it('should handle null/undefined values gracefully', () => {
      const props: TextAreaProps = {
        label: undefined,
        description: undefined,
        onChange: undefined,
        onKeyPress: undefined,
        fireModifierAndEnterKeyPress: undefined
      };

      textArea = new TextAreaInternal(props);
      expect(textArea.render).toBeDefined();
    });

    it('should handle empty string value', () => {
      textArea = new TextAreaInternal({ value: '' });
      expect(textArea.getValue()).toBe('');
    });

    it('should handle multiline text value', () => {
      const multilineValue = 'Line 1\nLine 2\nLine 3';
      textArea = new TextAreaInternal({ value: multilineValue });
      expect(textArea.getValue()).toBe(multilineValue);
    });

    it('should handle onChange without callback', () => {
      textArea = new TextAreaInternal({});

      document.body.appendChild(textArea.render);
      const textAreaElement = document.body.querySelector('textarea') as HTMLTextAreaElement;

      // Should not throw error
      textAreaElement.value = 'test';
      textAreaElement.dispatchEvent(new Event('input'));

      expect(textArea.getValue()).toBe('test');
    });

    it('should handle onKeyPress without callback', () => {
      textArea = new TextAreaInternal({});

      document.body.appendChild(textArea.render);
      const textAreaElement = document.body.querySelector('textarea') as HTMLTextAreaElement;

      // Should not throw error
      const keyEvent = new KeyboardEvent('keypress', { key: 'a' });
      textAreaElement.dispatchEvent(keyEvent);

      expect(textArea.render).toBeDefined();
    });

    it('should handle fireModifierAndEnterKeyPress without callback', () => {
      textArea = new TextAreaInternal({});

      document.body.appendChild(textArea.render);
      const textAreaElement = document.body.querySelector('textarea') as HTMLTextAreaElement;

      // Should not throw error
      const keyEvent = new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true });
      textAreaElement.dispatchEvent(keyEvent);

      expect(textArea.render).toBeDefined();
    });
  });
});
