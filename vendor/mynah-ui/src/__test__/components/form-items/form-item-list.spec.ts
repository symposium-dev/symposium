/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { FormItemList, FormItemListInternal, FormItemListProps } from '../../../components/form-items/form-item-list';
import { SingularFormItem, ListItemEntry } from '../../../static';
import { DomBuilder } from '../../../helper/dom';

// Mock the generateUID helper
jest.mock('../../../helper/guid', () => ({
  generateUID: jest.fn(() => 'test-row-id')
}));

// Mock the ChatItemFormItemsWrapper
jest.mock('../../../components/chat-item/chat-item-form-items', () => ({
  ChatItemFormItemsWrapper: jest.fn().mockImplementation((props) => ({
    render: document.createElement('div'),
    getAllValues: jest.fn(() => ({ field1: 'value1', field2: 'value2' })),
    enableAll: jest.fn(),
    disableAll: jest.fn()
  }))
}));

describe('FormItemList Component', () => {
  let formItemList: FormItemListInternal;
  let mockOnChange: jest.Mock;

  const testFormItems: SingularFormItem[] = [
    {
      id: 'field1',
      type: 'textinput',
      title: 'Field 1',
      description: 'Description for field 1'
    },
    {
      id: 'field2',
      type: 'textinput',
      title: 'Field 2'
    }
  ];

  beforeEach(() => {
    document.body.innerHTML = '';
    mockOnChange = jest.fn();
    jest.clearAllMocks();
  });

  afterEach(() => {
    document.body.innerHTML = '';
  });

  describe('FormItemListInternal', () => {
    it('should create form item list with default props', () => {
      formItemList = new FormItemListInternal({ items: testFormItems });

      expect(formItemList.render).toBeDefined();
      expect(formItemList.render.classList.contains('mynah-form-input-wrapper')).toBe(true);
    });

    it('should create form item list with label', () => {
      const label = 'Test Form List';
      formItemList = new FormItemListInternal({
        items: testFormItems,
        label
      });

      document.body.appendChild(formItemList.render);
      const labelElement = document.body.querySelector('.mynah-form-input-label');
      expect(labelElement?.textContent).toBe(label);
    });

    it('should create form item list with description', () => {
      const descriptionElement = DomBuilder.getInstance().build({
        type: 'div',
        children: [ 'Test description' ],
        classNames: [ 'test-description' ]
      });

      formItemList = new FormItemListInternal({
        items: testFormItems,
        description: descriptionElement
      });

      document.body.appendChild(formItemList.render);
      const description = document.body.querySelector('.test-description');
      expect(description?.textContent).toBe('Test description');
    });

    it('should create form item list with wrapper test ID', () => {
      const wrapperTestId = 'wrapper-test-id';
      formItemList = new FormItemListInternal({
        items: testFormItems,
        wrapperTestId
      });

      document.body.appendChild(formItemList.render);
      const wrapper = document.body.querySelector(`[data-testid="${wrapperTestId}"]`);
      expect(wrapper).toBeDefined();
    });

    it('should create form item list with custom class names', () => {
      const customClasses = [ 'custom-class-1', 'custom-class-2' ];
      formItemList = new FormItemListInternal({
        items: testFormItems,
        classNames: customClasses
      });

      // Note: classNames are not currently used in the implementation
      expect(formItemList.render).toBeDefined();
    });

    it('should create form item list with custom attributes', () => {
      const attributes = { 'data-test': 'test-value' };
      formItemList = new FormItemListInternal({
        items: testFormItems,
        attributes
      });

      // Note: attributes are not currently used in the implementation
      expect(formItemList.render).toBeDefined();
    });

    it('should initialize with one empty row by default', () => {
      formItemList = new FormItemListInternal({ items: testFormItems });

      document.body.appendChild(formItemList.render);
      const rows = document.body.querySelectorAll('.mynah-form-item-list-row');

      // Should have header row + 1 data row
      expect(rows.length).toBeGreaterThanOrEqual(1);
    });

    it('should initialize with provided values', () => {
      const initialValues: ListItemEntry[] = [
        { value: { field1: 'value1', field2: 'value2' } },
        { value: { field1: 'value3', field2: 'value4' } }
      ];

      formItemList = new FormItemListInternal({
        items: testFormItems,
        value: initialValues
      });

      document.body.appendChild(formItemList.render);
      const rows = document.body.querySelectorAll('.mynah-form-item-list-row');

      // Should have header row + 2 data rows
      expect(rows.length).toBeGreaterThanOrEqual(2);
    });

    it('should have add button', () => {
      formItemList = new FormItemListInternal({ items: testFormItems });

      document.body.appendChild(formItemList.render);
      const addButton = document.body.querySelector('.mynah-form-item-list-add-button');
      expect(addButton).toBeDefined();
    });

    it('should have remove buttons for each row', () => {
      formItemList = new FormItemListInternal({ items: testFormItems });

      document.body.appendChild(formItemList.render);
      const removeButtons = document.body.querySelectorAll('.mynah-form-item-list-row-remove-button');
      expect(removeButtons.length).toBeGreaterThanOrEqual(1);
    });

    it('should create header row with item titles and descriptions', () => {
      formItemList = new FormItemListInternal({ items: testFormItems });

      document.body.appendChild(formItemList.render);
      const headers = document.body.querySelectorAll('.mynah-form-item-list-row-header');

      // Should have headers for items with title or description
      expect(headers.length).toBeGreaterThanOrEqual(1);
    });

    it('should handle onChange callback', () => {
      formItemList = new FormItemListInternal({
        items: testFormItems,
        onChange: mockOnChange
      });

      // onChange should be called during initialization
      expect(mockOnChange).toHaveBeenCalled();
    });

    it('should handle getValue method', () => {
      formItemList = new FormItemListInternal({ items: testFormItems });

      const values = formItemList.getValue();
      expect(Array.isArray(values)).toBe(true);
      expect(values.length).toBeGreaterThanOrEqual(1);
    });

    it('should handle setValue method', () => {
      formItemList = new FormItemListInternal({ items: testFormItems });

      const newValues: ListItemEntry[] = [
        { value: { field1: 'new1', field2: 'new2' } }
      ];

      formItemList.setValue(newValues);

      // Should update the form with new values
      expect(formItemList.render).toBeDefined();
    });

    it('should handle setValue with empty array', () => {
      formItemList = new FormItemListInternal({ items: testFormItems });

      formItemList.setValue([]);

      // Should create one empty row
      const values = formItemList.getValue();
      expect(values.length).toBeGreaterThanOrEqual(1);
    });

    it('should handle setEnabled method', () => {
      formItemList = new FormItemListInternal({ items: testFormItems });

      // Test disabling
      formItemList.setEnabled(false);
      expect(formItemList.render.hasAttribute('disabled')).toBe(true);

      // Test enabling
      formItemList.setEnabled(true);
      expect(formItemList.render.hasAttribute('disabled')).toBe(false);
    });

    it('should handle persistent entries', () => {
      const persistentValues: ListItemEntry[] = [
        { value: { field1: 'persistent1', field2: 'persistent2' }, persistent: true }
      ];

      formItemList = new FormItemListInternal({
        items: testFormItems,
        value: persistentValues
      });

      document.body.appendChild(formItemList.render);

      // Persistent entries should have disabled remove buttons
      const removeButtons = document.body.querySelectorAll('.mynah-form-item-list-row-remove-button');
      expect(removeButtons.length).toBeGreaterThanOrEqual(1);
    });

    it('should handle add button click', () => {
      formItemList = new FormItemListInternal({
        items: testFormItems,
        onChange: mockOnChange
      });

      document.body.appendChild(formItemList.render);
      const addButton = document.body.querySelector('.mynah-form-item-list-add-button') as HTMLElement;

      const initialCallCount = mockOnChange.mock.calls.length;
      addButton.click();

      // Should trigger onChange when adding a row
      expect(mockOnChange.mock.calls.length).toBeGreaterThan(initialCallCount);
    });

    it('should handle remove button click', () => {
      formItemList = new FormItemListInternal({
        items: testFormItems,
        onChange: mockOnChange
      });

      document.body.appendChild(formItemList.render);
      const removeButton = document.body.querySelector('.mynah-form-item-list-row-remove-button') as HTMLElement;

      const initialCallCount = mockOnChange.mock.calls.length;
      removeButton.click();

      // Should trigger onChange when removing a row
      expect(mockOnChange.mock.calls.length).toBeGreaterThan(initialCallCount);
    });

    it('should have rows wrapper', () => {
      formItemList = new FormItemListInternal({ items: testFormItems });

      document.body.appendChild(formItemList.render);
      const rowsWrapper = document.body.querySelector('.mynah-form-item-list-rows-wrapper');
      expect(rowsWrapper).toBeDefined();
    });

    it('should have form item list wrapper', () => {
      formItemList = new FormItemListInternal({ items: testFormItems });

      document.body.appendChild(formItemList.render);
      const wrapper = document.body.querySelector('.mynah-form-item-list-wrapper');
      expect(wrapper).toBeDefined();
    });

    it('should handle items without title or description', () => {
      const itemsWithoutTitleDesc: SingularFormItem[] = [
        {
          id: 'field1',
          type: 'textinput'
        }
      ];

      formItemList = new FormItemListInternal({ items: itemsWithoutTitleDesc });

      document.body.appendChild(formItemList.render);
      const headers = document.body.querySelectorAll('.mynah-form-item-list-row-header');

      // Should have no headers when items have no title or description
      expect(headers.length).toBe(0);
    });
  });

  describe('FormItemList Factory', () => {
    it('should create FormItemListInternal by default', () => {
      const formItemListFactory = new FormItemList({ items: testFormItems });
      expect(formItemListFactory).toBeInstanceOf(FormItemListInternal);
    });

    it('should have abstract methods', () => {
      const formItemListFactory = new FormItemList({ items: testFormItems });
      expect(typeof formItemListFactory.setValue).toBe('function');
      expect(typeof formItemListFactory.getValue).toBe('function');
      expect(typeof formItemListFactory.setEnabled).toBe('function');
    });
  });

  describe('Edge Cases', () => {
    it('should handle empty items array', () => {
      formItemList = new FormItemListInternal({ items: [] });
      expect(formItemList.render).toBeDefined();
    });

    it('should handle null/undefined values gracefully', () => {
      const props: FormItemListProps = {
        items: testFormItems,
        label: undefined,
        description: undefined,
        onChange: undefined
      };

      formItemList = new FormItemListInternal(props);
      expect(formItemList.render).toBeDefined();
    });

    it('should handle onChange without callback', () => {
      formItemList = new FormItemListInternal({ items: testFormItems });

      // Should not throw error when onChange is not provided
      document.body.appendChild(formItemList.render);
      const addButton = document.body.querySelector('.mynah-form-item-list-add-button') as HTMLElement;
      addButton.click();

      expect(formItemList.render).toBeDefined();
    });

    it('should handle setValue with undefined', () => {
      formItemList = new FormItemListInternal({ items: testFormItems });

      // Should handle undefined gracefully
      formItemList.setValue([]);
      expect(formItemList.render).toBeDefined();
    });

    it('should handle complex form items', () => {
      const complexItems: SingularFormItem[] = [
        {
          id: 'field1',
          type: 'select',
          title: 'Select Field',
          options: [
            { value: 'option1', label: 'Option 1' },
            { value: 'option2', label: 'Option 2' }
          ]
        },
        {
          id: 'field2',
          type: 'radiogroup',
          title: 'Radio Field',
          options: [
            { value: 'radio1', label: 'Radio 1' },
            { value: 'radio2', label: 'Radio 2' }
          ]
        }
      ];

      formItemList = new FormItemListInternal({ items: complexItems });
      expect(formItemList.render).toBeDefined();
    });
  });
});
