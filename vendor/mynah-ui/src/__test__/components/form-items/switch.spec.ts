/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { Switch, SwitchInternal, SwitchProps } from '../../../components/form-items/switch';
import { MynahIcons } from '../../../components/icon';
import { DomBuilder } from '../../../helper/dom';

describe('Switch Component', () => {
  let switchComponent: SwitchInternal;
  let mockOnChange: jest.Mock;

  beforeEach(() => {
    document.body.innerHTML = '';
    mockOnChange = jest.fn();
  });

  afterEach(() => {
    document.body.innerHTML = '';
  });

  describe('SwitchInternal', () => {
    it('should create switch with default props', () => {
      switchComponent = new SwitchInternal({});

      expect(switchComponent.render).toBeDefined();
      expect(switchComponent.render.classList.contains('mynah-form-input-wrapper')).toBe(true);
      expect(switchComponent.getValue()).toBe('false');
    });

    it('should create switch with initial value true', () => {
      switchComponent = new SwitchInternal({ value: 'true' });

      expect(switchComponent.getValue()).toBe('true');
    });

    it('should create switch with label', () => {
      const label = 'Test Switch';
      switchComponent = new SwitchInternal({ label });

      document.body.appendChild(switchComponent.render);
      const switchWrapper = document.body.querySelector('.mynah-form-input-switch-wrapper');
      expect(switchWrapper?.textContent).toContain(label);
    });

    it('should create switch with title', () => {
      const title = 'Switch Title';
      switchComponent = new SwitchInternal({ title });

      document.body.appendChild(switchComponent.render);
      const titleElement = document.body.querySelector('.mynah-form-input-label');
      expect(titleElement?.textContent).toBe(title);
    });

    it('should create switch with custom icon', () => {
      switchComponent = new SwitchInternal({ icon: MynahIcons.CANCEL });

      document.body.appendChild(switchComponent.render);
      const iconElement = document.body.querySelector('.mynah-icon');
      expect(iconElement).toBeDefined();
    });

    it('should create switch with default OK icon', () => {
      switchComponent = new SwitchInternal({});

      document.body.appendChild(switchComponent.render);
      const iconElement = document.body.querySelector('.mynah-icon');
      expect(iconElement).toBeDefined();
    });

    it('should create switch with custom class names', () => {
      const customClasses = [ 'custom-class-1', 'custom-class-2' ];
      switchComponent = new SwitchInternal({ classNames: customClasses });

      document.body.appendChild(switchComponent.render);
      const switchInput = document.body.querySelector('.mynah-form-input');
      expect(switchInput?.classList.contains('custom-class-1')).toBe(true);
      expect(switchInput?.classList.contains('custom-class-2')).toBe(true);
    });

    it('should create switch with custom attributes', () => {
      const attributes = { 'data-test': 'test-value', 'aria-label': 'test-switch' };
      switchComponent = new SwitchInternal({ attributes });

      document.body.appendChild(switchComponent.render);
      const container = document.body.querySelector('.mynah-form-input-container');
      expect(container?.getAttribute('data-test')).toBe('test-value');
      expect(container?.getAttribute('aria-label')).toBe('test-switch');
    });

    it('should create switch with test ID', () => {
      const testId = 'test-switch-id';
      switchComponent = new SwitchInternal({ testId });

      document.body.appendChild(switchComponent.render);
      const switchElement = document.body.querySelector(`[data-testid="${testId}"]`);
      expect(switchElement).toBeDefined();
    });

    it('should handle setValue method', () => {
      switchComponent = new SwitchInternal({});

      switchComponent.setValue('true');
      expect(switchComponent.getValue()).toBe('true');

      switchComponent.setValue('false');
      expect(switchComponent.getValue()).toBe('false');
    });

    it('should handle setEnabled method', () => {
      switchComponent = new SwitchInternal({});
      document.body.appendChild(switchComponent.render);

      const checkboxInput = document.body.querySelector('input[type="checkbox"]') as HTMLInputElement;
      const wrapper = document.body.querySelector('.mynah-form-input') as HTMLElement;

      // Test disabling
      switchComponent.setEnabled(false);
      expect(checkboxInput.hasAttribute('disabled')).toBe(true);
      expect(wrapper.hasAttribute('disabled')).toBe(true);

      // Test enabling
      switchComponent.setEnabled(true);
      expect(checkboxInput.hasAttribute('disabled')).toBe(false);
      expect(wrapper.hasAttribute('disabled')).toBe(false);
    });

    it('should trigger onChange when clicked', () => {
      switchComponent = new SwitchInternal({ onChange: mockOnChange });
      document.body.appendChild(switchComponent.render);

      const label = document.body.querySelector('.mynah-form-input-switch-label') as HTMLElement;

      // Click to turn on
      label.click();
      expect(mockOnChange).toHaveBeenCalledWith('true');
      expect(switchComponent.getValue()).toBe('true');

      // Click to turn off
      label.click();
      expect(mockOnChange).toHaveBeenCalledWith('false');
      expect(switchComponent.getValue()).toBe('false');
    });

    it('should prevent event propagation on click', () => {
      switchComponent = new SwitchInternal({ onChange: mockOnChange });
      document.body.appendChild(switchComponent.render);

      const label = document.body.querySelector('.mynah-form-input-switch-label') as HTMLElement;
      const clickEvent = new MouseEvent('click', { bubbles: true, cancelable: true });

      const preventDefaultSpy = jest.spyOn(clickEvent, 'preventDefault');
      const stopPropagationSpy = jest.spyOn(clickEvent, 'stopPropagation');

      label.dispatchEvent(clickEvent);

      expect(preventDefaultSpy).toHaveBeenCalled();
      expect(stopPropagationSpy).toHaveBeenCalled();
    });

    it('should have switch-specific elements', () => {
      switchComponent = new SwitchInternal({});
      document.body.appendChild(switchComponent.render);

      const switchWrapper = document.body.querySelector('.mynah-form-input-switch-wrapper');
      const switchLabel = document.body.querySelector('.mynah-form-input-switch-label');
      const switchCheck = document.body.querySelector('.mynah-form-input-switch-check');
      const switchBg = document.body.querySelector('.mynah-form-input-switch-check-bg');

      expect(switchWrapper).toBeDefined();
      expect(switchLabel).toBeDefined();
      expect(switchCheck).toBeDefined();
      expect(switchBg).toBeDefined();
    });

    it('should contain checkbox input', () => {
      switchComponent = new SwitchInternal({});
      document.body.appendChild(switchComponent.render);

      const checkboxInput = document.body.querySelector('input[type="checkbox"]');
      expect(checkboxInput).toBeDefined();
    });

    it('should handle description element', () => {
      const descriptionElement = DomBuilder.getInstance().build({
        type: 'div',
        children: [ 'Test description' ],
        classNames: [ 'test-description' ]
      });

      switchComponent = new SwitchInternal({ description: descriptionElement });
      document.body.appendChild(switchComponent.render);

      const description = document.body.querySelector('.test-description');
      expect(description?.textContent).toBe('Test description');
    });

    it('should have no-border class on container', () => {
      switchComponent = new SwitchInternal({});

      document.body.appendChild(switchComponent.render);
      const container = document.body.querySelector('.mynah-form-input-container');
      expect(container?.classList.contains('no-border')).toBe(true);
    });

    it('should handle optional property', () => {
      switchComponent = new SwitchInternal({ optional: true });

      // The optional property is passed but doesn't affect rendering in current implementation
      expect(switchComponent.render).toBeDefined();
    });

    it('should toggle state correctly on multiple clicks', () => {
      switchComponent = new SwitchInternal({ onChange: mockOnChange });
      document.body.appendChild(switchComponent.render);

      const label = document.body.querySelector('.mynah-form-input-switch-label') as HTMLElement;

      // Multiple clicks
      label.click(); // false -> true
      label.click(); // true -> false
      label.click(); // false -> true
      label.click(); // true -> false

      expect(mockOnChange).toHaveBeenCalledTimes(4);
      expect(mockOnChange).toHaveBeenNthCalledWith(1, 'true');
      expect(mockOnChange).toHaveBeenNthCalledWith(2, 'false');
      expect(mockOnChange).toHaveBeenNthCalledWith(3, 'true');
      expect(mockOnChange).toHaveBeenNthCalledWith(4, 'false');
      expect(switchComponent.getValue()).toBe('false');
    });

    it('should start with correct initial state', () => {
      switchComponent = new SwitchInternal({ value: 'true' });
      document.body.appendChild(switchComponent.render);

      const checkboxInput = document.body.querySelector('input[type="checkbox"]') as HTMLInputElement;
      expect(checkboxInput.checked).toBe(true);
      expect(switchComponent.getValue()).toBe('true');
    });
  });

  describe('Switch Factory', () => {
    it('should create SwitchInternal by default', () => {
      const switchFactory = new Switch({});
      expect(switchFactory).toBeInstanceOf(SwitchInternal);
    });

    it('should have abstract methods', () => {
      const switchFactory = new Switch({});
      expect(typeof switchFactory.setValue).toBe('function');
      expect(typeof switchFactory.getValue).toBe('function');
      expect(typeof switchFactory.setEnabled).toBe('function');
    });
  });

  describe('Edge Cases', () => {
    it('should handle empty props object', () => {
      switchComponent = new SwitchInternal({});
      expect(switchComponent.render).toBeDefined();
      expect(switchComponent.getValue()).toBe('false');
    });

    it('should handle null/undefined values gracefully', () => {
      const props: SwitchProps = {
        title: undefined,
        label: undefined,
        description: undefined,
        onChange: undefined
      };

      switchComponent = new SwitchInternal(props);
      expect(switchComponent.render).toBeDefined();
    });

    it('should handle onChange without callback', () => {
      switchComponent = new SwitchInternal({});

      document.body.appendChild(switchComponent.render);
      const label = document.body.querySelector('.mynah-form-input-switch-label') as HTMLElement;

      // Should not throw error
      label.click();

      expect(switchComponent.getValue()).toBe('true');
    });

    it('should handle setValue with string boolean values', () => {
      switchComponent = new SwitchInternal({});

      switchComponent.setValue('true');
      expect(switchComponent.getValue()).toBe('true');

      switchComponent.setValue('false');
      expect(switchComponent.getValue()).toBe('false');
    });

    it('should maintain state consistency', () => {
      switchComponent = new SwitchInternal({ value: 'false' });
      document.body.appendChild(switchComponent.render);

      const checkboxInput = document.body.querySelector('input[type="checkbox"]') as HTMLInputElement;

      // Initial state
      expect(checkboxInput.checked).toBe(false);
      expect(switchComponent.getValue()).toBe('false');

      // Set to true
      switchComponent.setValue('true');
      expect(checkboxInput.checked).toBe(true);
      expect(switchComponent.getValue()).toBe('true');

      // Set back to false
      switchComponent.setValue('false');
      expect(checkboxInput.checked).toBe(false);
      expect(switchComponent.getValue()).toBe('false');
    });
  });
});
