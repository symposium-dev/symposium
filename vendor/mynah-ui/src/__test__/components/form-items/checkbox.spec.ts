/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { Checkbox, CheckboxInternal, CheckboxProps } from '../../../components/form-items/checkbox';
import { MynahIcons } from '../../../components/icon';
import { DomBuilder } from '../../../helper/dom';

describe('Checkbox Component', () => {
  let checkbox: CheckboxInternal;
  let mockOnChange: jest.Mock;

  beforeEach(() => {
    document.body.innerHTML = '';
    mockOnChange = jest.fn();
  });

  afterEach(() => {
    document.body.innerHTML = '';
  });

  describe('CheckboxInternal', () => {
    it('should create checkbox with default props', () => {
      checkbox = new CheckboxInternal({});

      expect(checkbox.render).toBeDefined();
      expect(checkbox.render.classList.contains('mynah-form-input-wrapper')).toBe(true);
      expect(checkbox.getValue()).toBe('false');
    });

    it('should create checkbox with initial value true', () => {
      checkbox = new CheckboxInternal({ value: 'true' });

      expect(checkbox.getValue()).toBe('true');
    });

    it('should create checkbox with label', () => {
      const label = 'Test Checkbox';
      checkbox = new CheckboxInternal({ label });

      document.body.appendChild(checkbox.render);
      const labelElement = document.body.querySelector('.mynah-form-input-radio-label');
      expect(labelElement?.textContent).toContain(label);
    });

    it('should create checkbox with title', () => {
      const title = 'Checkbox Title';
      checkbox = new CheckboxInternal({ title });

      document.body.appendChild(checkbox.render);
      const titleElement = document.body.querySelector('.mynah-form-input-label');
      expect(titleElement?.textContent).toBe(title);
    });

    it('should create checkbox with custom icon', () => {
      checkbox = new CheckboxInternal({ icon: MynahIcons.CANCEL });

      document.body.appendChild(checkbox.render);
      const iconElement = document.body.querySelector('.mynah-icon');
      expect(iconElement).toBeDefined();
    });

    it('should create checkbox with custom class names', () => {
      const customClasses = [ 'custom-class-1', 'custom-class-2' ];
      checkbox = new CheckboxInternal({ classNames: customClasses });

      expect(checkbox.render.querySelector('.custom-class-1')).toBeDefined();
      expect(checkbox.render.querySelector('.custom-class-2')).toBeDefined();
    });

    it('should create checkbox with custom attributes', () => {
      const attributes = { 'data-test': 'test-value', 'aria-label': 'test-checkbox' };
      checkbox = new CheckboxInternal({ attributes });

      document.body.appendChild(checkbox.render);
      const container = document.body.querySelector('.mynah-form-input-container');
      expect(container?.getAttribute('data-test')).toBe('test-value');
      expect(container?.getAttribute('aria-label')).toBe('test-checkbox');
    });

    it('should create checkbox with test IDs', () => {
      const wrapperTestId = 'wrapper-test-id';
      const optionTestId = 'option-test-id';
      checkbox = new CheckboxInternal({ wrapperTestId, optionTestId });

      document.body.appendChild(checkbox.render);
      const wrapper = document.body.querySelector(`[data-testid="${wrapperTestId}"]`);
      const option = document.body.querySelector(`[data-testid="${optionTestId}"]`);
      expect(wrapper).toBeDefined();
      expect(option).toBeDefined();
    });

    it('should handle setValue method', () => {
      checkbox = new CheckboxInternal({});

      checkbox.setValue('true');
      expect(checkbox.getValue()).toBe('true');

      checkbox.setValue('false');
      expect(checkbox.getValue()).toBe('false');
    });

    it('should handle setEnabled method', () => {
      checkbox = new CheckboxInternal({});
      document.body.appendChild(checkbox.render);

      const checkboxInput = document.body.querySelector('.as-checkbox') as HTMLInputElement;
      const wrapper = document.body.querySelector('.mynah-form-input') as HTMLElement;

      // Test disabling
      checkbox.setEnabled(false);
      expect(checkboxInput.hasAttribute('disabled')).toBe(true);
      expect(wrapper.hasAttribute('disabled')).toBe(true);

      // Test enabling
      checkbox.setEnabled(true);
      expect(checkboxInput.hasAttribute('disabled')).toBe(false);
      expect(wrapper.hasAttribute('disabled')).toBe(false);
    });

    it('should trigger onChange when clicked', () => {
      checkbox = new CheckboxInternal({ onChange: mockOnChange });
      document.body.appendChild(checkbox.render);

      const label = document.body.querySelector('.mynah-form-input-radio-label') as HTMLElement;

      // Click to check
      label.click();
      expect(mockOnChange).toHaveBeenCalledWith('true');
      expect(checkbox.getValue()).toBe('true');

      // Click to uncheck
      label.click();
      expect(mockOnChange).toHaveBeenCalledWith('false');
      expect(checkbox.getValue()).toBe('false');
    });

    it('should prevent event propagation on click', () => {
      checkbox = new CheckboxInternal({ onChange: mockOnChange });
      document.body.appendChild(checkbox.render);

      const label = document.body.querySelector('.mynah-form-input-radio-label') as HTMLElement;
      const clickEvent = new MouseEvent('click', { bubbles: true, cancelable: true });

      const preventDefaultSpy = jest.spyOn(clickEvent, 'preventDefault');
      const stopPropagationSpy = jest.spyOn(clickEvent, 'stopPropagation');

      label.dispatchEvent(clickEvent);

      expect(preventDefaultSpy).toHaveBeenCalled();
      expect(stopPropagationSpy).toHaveBeenCalled();
    });

    it('should handle description element', () => {
      const descriptionElement = DomBuilder.getInstance().build({
        type: 'div',
        children: [ 'Test description' ],
        classNames: [ 'test-description' ]
      });

      checkbox = new CheckboxInternal({ description: descriptionElement });
      document.body.appendChild(checkbox.render);

      const description = document.body.querySelector('.test-description');
      expect(description?.textContent).toBe('Test description');
    });

    it('should handle optional property', () => {
      checkbox = new CheckboxInternal({ optional: true });

      // The optional property is passed but doesn't affect rendering in current implementation
      expect(checkbox.render).toBeDefined();
    });
  });

  describe('Checkbox Factory', () => {
    it('should create CheckboxInternal by default', () => {
      const checkboxFactory = new Checkbox({});
      expect(checkboxFactory).toBeInstanceOf(CheckboxInternal);
    });

    it('should have abstract methods', () => {
      const checkboxFactory = new Checkbox({});
      expect(typeof checkboxFactory.setValue).toBe('function');
      expect(typeof checkboxFactory.getValue).toBe('function');
      expect(typeof checkboxFactory.setEnabled).toBe('function');
    });
  });

  describe('Edge Cases', () => {
    it('should handle empty props object', () => {
      checkbox = new CheckboxInternal({});
      expect(checkbox.render).toBeDefined();
      expect(checkbox.getValue()).toBe('false');
    });

    it('should handle null/undefined values gracefully', () => {
      const props: CheckboxProps = {
        title: undefined,
        label: undefined,
        description: undefined,
        onChange: undefined
      };

      checkbox = new CheckboxInternal(props);
      expect(checkbox.render).toBeDefined();
    });

    it('should handle multiple rapid clicks', () => {
      checkbox = new CheckboxInternal({ onChange: mockOnChange });
      document.body.appendChild(checkbox.render);

      const label = document.body.querySelector('.mynah-form-input-radio-label') as HTMLElement;

      // Rapid clicks
      label.click();
      label.click();
      label.click();

      expect(mockOnChange).toHaveBeenCalledTimes(3);
      expect(mockOnChange).toHaveBeenNthCalledWith(1, 'true');
      expect(mockOnChange).toHaveBeenNthCalledWith(2, 'false');
      expect(mockOnChange).toHaveBeenNthCalledWith(3, 'true');
    });
  });
});
