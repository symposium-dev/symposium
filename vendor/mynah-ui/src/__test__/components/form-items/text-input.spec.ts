/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { TextInput, TextInputInternal, TextInputProps } from '../../../components/form-items/text-input';
import { MynahIcons } from '../../../components/icon';
import { DomBuilder } from '../../../helper/dom';

// Mock the validator helper
jest.mock('../../../helper/validator', () => ({
  checkTextElementValidation: jest.fn()
}));

describe('TextInput Component', () => {
  let textInput: TextInputInternal;
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

  describe('TextInputInternal', () => {
    it('should create text input with default props', () => {
      textInput = new TextInputInternal({});

      expect(textInput.render).toBeDefined();
      expect(textInput.render.classList.contains('mynah-form-input-wrapper')).toBe(true);
      expect(textInput.getValue()).toBe('');
    });

    it('should create text input with initial value', () => {
      const initialValue = 'test value';
      textInput = new TextInputInternal({ value: initialValue });

      expect(textInput.getValue()).toBe(initialValue);
    });

    it('should create text input with label', () => {
      const label = 'Test Label';
      textInput = new TextInputInternal({ label });

      document.body.appendChild(textInput.render);
      const labelElement = document.body.querySelector('.mynah-form-input-label');
      expect(labelElement?.textContent).toBe(label);
    });

    it('should create text input with placeholder', () => {
      const placeholder = 'Enter text here';
      textInput = new TextInputInternal({ placeholder });

      document.body.appendChild(textInput.render);
      const inputElement = document.body.querySelector('.mynah-form-input') as HTMLInputElement;
      expect(inputElement.placeholder).toBe(placeholder);
    });

    it('should create text input with different types', () => {
      const types: Array<'text' | 'number' | 'email'> = [ 'text', 'number', 'email' ];

      types.forEach(type => {
        textInput = new TextInputInternal({ type });
        document.body.appendChild(textInput.render);

        const inputElement = document.body.querySelector('.mynah-form-input') as HTMLInputElement;
        expect(inputElement.type).toBe(type);

        document.body.innerHTML = '';
      });
    });

    it('should create text input with icon', () => {
      textInput = new TextInputInternal({ icon: MynahIcons.SEARCH });

      document.body.appendChild(textInput.render);
      const iconElement = document.body.querySelector('.mynah-form-input-icon');
      expect(iconElement).toBeDefined();
    });

    it('should create text input with custom class names', () => {
      const customClasses = [ 'custom-class-1', 'custom-class-2' ];
      textInput = new TextInputInternal({ classNames: customClasses });

      document.body.appendChild(textInput.render);
      const inputElement = document.body.querySelector('.mynah-form-input');
      expect(inputElement?.classList.contains('custom-class-1')).toBe(true);
      expect(inputElement?.classList.contains('custom-class-2')).toBe(true);
    });

    it('should create text input with custom attributes', () => {
      const attributes = { 'data-test': 'test-value', 'aria-label': 'test-input' };
      textInput = new TextInputInternal({ attributes });

      document.body.appendChild(textInput.render);
      const container = document.body.querySelector('.mynah-form-input-container');
      expect(container?.getAttribute('data-test')).toBe('test-value');
      expect(container?.getAttribute('aria-label')).toBe('test-input');
    });

    it('should create text input with test ID', () => {
      const testId = 'test-input-id';
      textInput = new TextInputInternal({ testId });

      document.body.appendChild(textInput.render);
      const inputElement = document.body.querySelector(`[data-testid="${testId}"]`);
      expect(inputElement).toBeDefined();
    });

    it('should handle autoFocus', (done) => {
      textInput = new TextInputInternal({ autoFocus: true });
      document.body.appendChild(textInput.render);

      const inputElement = document.body.querySelector('.mynah-form-input') as HTMLInputElement;

      // Check that autofocus attribute is set
      expect(inputElement.hasAttribute('autofocus')).toBe(true);

      // Check that focus is called after timeout
      setTimeout(() => {
        expect(document.activeElement).toBe(inputElement);
        done();
      }, 300);
    });

    it('should handle setValue and getValue methods', () => {
      textInput = new TextInputInternal({});

      const testValue = 'test value';
      textInput.setValue(testValue);
      expect(textInput.getValue()).toBe(testValue);
    });

    it('should handle setEnabled method', () => {
      textInput = new TextInputInternal({});
      document.body.appendChild(textInput.render);

      const inputElement = document.body.querySelector('.mynah-form-input') as HTMLInputElement;

      // Test disabling
      textInput.setEnabled(false);
      expect(inputElement.hasAttribute('disabled')).toBe(true);

      // Test enabling
      textInput.setEnabled(true);
      expect(inputElement.hasAttribute('disabled')).toBe(false);
    });

    it('should trigger onChange on input event', () => {
      textInput = new TextInputInternal({ onChange: mockOnChange });
      document.body.appendChild(textInput.render);

      const inputElement = document.body.querySelector('.mynah-form-input') as HTMLInputElement;
      const testValue = 'test input';

      inputElement.value = testValue;
      inputElement.dispatchEvent(new Event('input'));

      expect(mockOnChange).toHaveBeenCalledWith(testValue);
    });

    it('should trigger onKeyPress on keypress event', () => {
      textInput = new TextInputInternal({ onKeyPress: mockOnKeyPress });
      document.body.appendChild(textInput.render);

      const inputElement = document.body.querySelector('.mynah-form-input') as HTMLInputElement;
      const keyEvent = new KeyboardEvent('keypress', { key: 'a' });

      inputElement.dispatchEvent(keyEvent);

      expect(mockOnKeyPress).toHaveBeenCalledWith(keyEvent);
    });

    it('should trigger fireModifierAndEnterKeyPress on Ctrl+Enter', () => {
      textInput = new TextInputInternal({ fireModifierAndEnterKeyPress: mockFireModifierAndEnterKeyPress });
      document.body.appendChild(textInput.render);

      const inputElement = document.body.querySelector('.mynah-form-input') as HTMLInputElement;
      const keyEvent = new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true });

      inputElement.dispatchEvent(keyEvent);

      expect(mockFireModifierAndEnterKeyPress).toHaveBeenCalled();
    });

    it('should trigger fireModifierAndEnterKeyPress on Cmd+Enter (Mac)', () => {
      textInput = new TextInputInternal({ fireModifierAndEnterKeyPress: mockFireModifierAndEnterKeyPress });
      document.body.appendChild(textInput.render);

      const inputElement = document.body.querySelector('.mynah-form-input') as HTMLInputElement;
      const keyEvent = new KeyboardEvent('keydown', { key: 'Enter', metaKey: true });

      inputElement.dispatchEvent(keyEvent);

      expect(mockFireModifierAndEnterKeyPress).toHaveBeenCalled();
    });

    it('should not trigger fireModifierAndEnterKeyPress on Enter without modifier', () => {
      textInput = new TextInputInternal({ fireModifierAndEnterKeyPress: mockFireModifierAndEnterKeyPress });
      document.body.appendChild(textInput.render);

      const inputElement = document.body.querySelector('.mynah-form-input') as HTMLInputElement;
      const keyEvent = new KeyboardEvent('keydown', { key: 'Enter' });

      inputElement.dispatchEvent(keyEvent);

      expect(mockFireModifierAndEnterKeyPress).not.toHaveBeenCalled();
    });

    it('should call checkValidation on blur event', () => {
      const checkTextElementValidation = jest.requireMock('../../../helper/validator').checkTextElementValidation;
      textInput = new TextInputInternal({});
      document.body.appendChild(textInput.render);

      const inputElement = document.body.querySelector('.mynah-form-input') as HTMLInputElement;
      inputElement.dispatchEvent(new Event('blur'));

      expect(checkTextElementValidation).toHaveBeenCalled();
    });

    it('should call checkValidation on input event', () => {
      const checkTextElementValidation = jest.requireMock('../../../helper/validator').checkTextElementValidation;
      textInput = new TextInputInternal({});
      document.body.appendChild(textInput.render);

      const inputElement = document.body.querySelector('.mynah-form-input') as HTMLInputElement;
      inputElement.dispatchEvent(new Event('input'));

      expect(checkTextElementValidation).toHaveBeenCalled();
    });

    it('should handle validation patterns', () => {
      const validationPatterns = {
        operator: 'and' as const,
        patterns: [ { pattern: /^[^\s@]+@[^\s@]+\.[^\s@]+$/, errorMessage: 'Invalid email' } ]
      };

      textInput = new TextInputInternal({ validationPatterns });
      expect(textInput.render).toBeDefined();
    });

    it('should handle mandatory field', () => {
      textInput = new TextInputInternal({ mandatory: true });
      expect(textInput.render).toBeDefined();
    });

    it('should handle description element', () => {
      const descriptionElement = DomBuilder.getInstance().build({
        type: 'div',
        children: [ 'Test description' ],
        classNames: [ 'test-description' ]
      });

      textInput = new TextInputInternal({ description: descriptionElement });
      document.body.appendChild(textInput.render);

      const description = document.body.querySelector('.test-description');
      expect(description?.textContent).toBe('Test description');
    });

    it('should have validation error block', () => {
      textInput = new TextInputInternal({});
      document.body.appendChild(textInput.render);

      const errorBlock = document.body.querySelector('.mynah-form-input-validation-error-block');
      expect(errorBlock).toBeDefined();
    });
  });

  describe('TextInput Factory', () => {
    it('should create TextInputInternal by default', () => {
      const textInputFactory = new TextInput({});
      expect(textInputFactory).toBeInstanceOf(TextInputInternal);
    });

    it('should have abstract methods', () => {
      const textInputFactory = new TextInput({});
      expect(typeof textInputFactory.setValue).toBe('function');
      expect(typeof textInputFactory.getValue).toBe('function');
      expect(typeof textInputFactory.setEnabled).toBe('function');
      expect(typeof textInputFactory.checkValidation).toBe('function');
    });
  });

  describe('Edge Cases', () => {
    it('should handle empty props object', () => {
      textInput = new TextInputInternal({});
      expect(textInput.render).toBeDefined();
      expect(textInput.getValue()).toBe('');
    });

    it('should handle null/undefined values gracefully', () => {
      const props: TextInputProps = {
        label: undefined,
        description: undefined,
        onChange: undefined,
        onKeyPress: undefined,
        fireModifierAndEnterKeyPress: undefined
      };

      textInput = new TextInputInternal(props);
      expect(textInput.render).toBeDefined();
    });

    it('should handle numeric value as string', () => {
      textInput = new TextInputInternal({ value: '123' });
      expect(textInput.getValue()).toBe('123');
    });

    it('should handle empty string value', () => {
      textInput = new TextInputInternal({ value: '' });
      expect(textInput.getValue()).toBe('');
    });
  });
});
