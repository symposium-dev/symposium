/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { DomBuilder, ExtendedHTMLElement } from '../../helper/dom';
import { DropdownFactoryProps, DropdownListOption } from '../../static';
import { DropdownList } from './dropdown-list';
import testIds from '../../helper/test-ids';

export interface DropdownWrapperProps {
  dropdownProps: DropdownFactoryProps;
  classNames?: string[];
}

export class DropdownWrapper {
  private readonly props: DropdownWrapperProps;
  private dropdown: DropdownList | null = null;

  render: ExtendedHTMLElement;

  constructor (props: DropdownWrapperProps) {
    this.props = props;

    this.render = DomBuilder.getInstance().build({
      type: 'div',
      testId: testIds.dropdownList.wrapper,
      classNames: [
        'mynah-dropdown-wrapper',
        ...(this.props.classNames ?? [])
      ],
      children: [ this.createDropdownComponent() ]
    });
  }

  private readonly createDropdownComponent = (): ExtendedHTMLElement => {
    const { dropdownProps } = this.props;

    // For now, all types use DropdownList
    // Future implementations can add specific components for radio, checkbox, etc.
    switch (dropdownProps.type) {
      case 'select':
      case 'radio':
      case 'checkbox':
      default:
        this.dropdown = new DropdownList({
          description: dropdownProps.description,
          descriptionLink: dropdownProps.descriptionLink,
          options: dropdownProps.options,
          onChange: dropdownProps.onChange,
          tabId: dropdownProps.tabId,
          messageId: dropdownProps.messageId,
          classNames: dropdownProps.classNames
        });
        return this.dropdown.render;
    }
  };

  public readonly getSelectedItems = (): DropdownListOption[] => {
    return this.dropdown?.getSelectedItems() ?? [];
  };

  public readonly setSelectedItems = (itemIds: string[]): void => {
    this.dropdown?.setSelectedItems(itemIds);
  };

  public readonly destroy = (): void => {
    this.dropdown?.destroy();
    this.dropdown = null;
  };
}
