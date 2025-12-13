/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { DomBuilder, ExtendedHTMLElement } from '../../helper/dom';
import { StyleLoader } from '../../helper/style-loader';
import { Icon, MynahIcons } from '../icon';
import { DropdownListOption, DropdownListProps } from '../../static';
import testIds from '../../helper/test-ids';
import { BaseDropdown, BaseDropdownProps } from './base-dropdown';

export class DropdownList extends BaseDropdown<DropdownListOption> {
  constructor (props: DropdownListProps) {
    // Handle backward compatibility - support both 'options' and 'items'
    const normalizedProps: BaseDropdownProps<DropdownListOption> = {
      ...props,
      items: props.options ?? [] // Map 'options' to 'items' for base class
    };

    super(normalizedProps);

    // Load the specific CSS for dropdown list
    StyleLoader.getInstance().load('components/_dropdown-list.scss');
  }

  protected getInitialSelection (): DropdownListOption[] {
    // Initialize selected items from options that have selected: true
    return this.props.items.filter(option => option.selected ?? false);
  }

  protected createItemElement (option: DropdownListOption): ExtendedHTMLElement {
    const isSelected = this.getItemSelectionState(option);

    return DomBuilder.getInstance().build({
      type: 'div',
      testId: testIds.dropdownList.option,
      classNames: [
        'mynah-dropdown-list-option',
        ...(isSelected ? [ 'selected' ] : [])
      ],
      attributes: {
        'data-item-id': option.id
      },
      events: {
        click: (e) => {
          e.stopPropagation();
          this.handleItemSelection(option);
        }
      },
      children: [
        {
          type: 'div',
          classNames: [ 'mynah-dropdown-list-checkbox' ],
          children: [
            ...(isSelected ? [ new Icon({ icon: MynahIcons.OK, classNames: [ 'mynah-dropdown-list-check-icon' ] }).render ] : [])
          ]
        },
        {
          type: 'span',
          testId: testIds.dropdownList.optionLabel,
          classNames: [ 'mynah-dropdown-list-option-label' ],
          children: [
            option.label
          ]
        }
      ]
    });
  }

  protected handleItemSelection (option: DropdownListOption): void {
    // Select only this option (single selection behavior)
    this.selectedItems = [ option ];

    // Update UI, close dropdown and dispatch event
    this.updateUI();
    this.isOpen = false;
    this.closeDropdown();
    this.dispatchChangeEvent();
  }

  protected getItemSelectionState (option: DropdownListOption): boolean {
    return this.selectedItems.some(selectedOption => selectedOption.id === option.id);
  }

  protected getDisplayLabel (): string {
    return this.selectedItems.length > 0 ? this.selectedItems[0].label : '';
  }

  protected getItemId (option: DropdownListOption): string {
    return option.id;
  }

  public readonly getSelectedOptions = (): DropdownListOption[] => {
    return this.getSelectedItems();
  };

  public readonly setSelectedOptions = (optionIds: string[]): void => {
    this.setSelectedItems(optionIds);
  };
}
