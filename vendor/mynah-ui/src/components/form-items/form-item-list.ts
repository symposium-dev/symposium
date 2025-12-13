/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { Config } from '../../helper/config';
import { DomBuilder, ExtendedHTMLElement } from '../../helper/dom';
import { cancelEvent } from '../../helper/events';
import { generateUID } from '../../helper/guid';
import { StyleLoader } from '../../helper/style-loader';
import { ListItemEntry, SingularFormItem } from '../../static';
import { Button } from '../button';
import { ChatItemFormItemsWrapper } from '../chat-item/chat-item-form-items';
import { Icon, MynahIcons } from '../icon';

export interface FormItemListProps {
  items: SingularFormItem[];
  value?: ListItemEntry[];
  classNames?: string[];
  attributes?: Record<string, string>;
  label?: HTMLElement | ExtendedHTMLElement | string;
  description?: ExtendedHTMLElement;
  wrapperTestId?: string;
  onChange?: (values: Array< Record<string, string | Array<Record<string, string>>>>) => void;
}

export abstract class FormItemListAbstract {
  render: ExtendedHTMLElement;
  setValue = (value: ListItemEntry[]): void => { };
  getValue = (): Array< Record<string, string>> => [];
  setEnabled = (enabled: boolean): void => { };
}

export class FormItemListInternal extends FormItemListAbstract {
  private readonly rowWrapper: ExtendedHTMLElement;
  private readonly addButton: ExtendedHTMLElement;
  private readonly props: FormItemListProps;
  private readonly rows: Map<string, {rowElm: ExtendedHTMLElement; rowForm: ChatItemFormItemsWrapper}> = new Map();
  render: ExtendedHTMLElement;

  constructor (props: FormItemListProps) {
    StyleLoader.getInstance().load('components/form-items/_form-item-list.scss');
    super();
    this.props = props;

    this.rowWrapper = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-form-item-list-rows-wrapper' ],
      children: [
        DomBuilder.getInstance().build({
          type: 'div',
          classNames: [ 'mynah-form-item-list-row' ],
          children: [
            ...this.props.items.filter(item => item.description != null || item.title != null).map(item => DomBuilder.getInstance().build({
              type: 'div',
              classNames: [ 'mynah-form-item-list-row-header' ],
              children: [
                ...(item.title !== undefined
                  ? [ {
                      type: 'span',
                      classNames: [ 'mynah-form-input-label' ],
                      children: [
                        ...(item.title !== undefined ? [ item.title ] : []),
                      ]
                    } ]
                  : []),
                ...(item.description !== undefined
                  ? [ {
                      type: 'span',
                      classNames: [ 'mynah-ui-form-item-description' ],
                      children: [
                        ...(item.description !== undefined ? [ item.description ] : []),
                      ]
                    } ]
                  : []),
              ]
            })),
            new Button({
              classNames: [ 'mynah-form-item-list-row-remove-all-button' ],
              primary: false,
              disabled: true,
              onClick: (e) => {
                // Maybe remove all?
              },
              icon: new Icon({ icon: MynahIcons.CANCEL }).render
            }).render
          ]
        })
      ]
    });

    this.addButton = new Button({
      classNames: [ 'mynah-form-item-list-add-button' ],
      primary: false,
      label: Config.getInstance().config.texts.add,
      onClick: (e) => {
        cancelEvent(e);
        this.addRow();
      },
      icon: new Icon({ icon: MynahIcons.PLUS }).render
    }).render;

    this.render = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-form-input-wrapper' ],
      children: [
        {
          type: 'span',
          classNames: [ 'mynah-form-input-label' ],
          children: [
            ...(props.label !== undefined ? [ props.label ] : []),
          ]
        },
        ...[ props.description !== undefined ? props.description : '' ],
        {
          type: 'div',
          classNames: [ 'mynah-form-item-list-wrapper' ],
          testId: props.wrapperTestId,
          children: [
            this.rowWrapper,
            this.addButton
          ]
        },
      ]
    });

    // Initialize with existing values or add an empty row
    if (props.value != null && props.value.length > 0) {
      props.value?.forEach(entry => this.addRow(entry));
    } else {
      this.addRow();
    }
  }

  private addRow (entry?: ListItemEntry): void {
    const rowId = generateUID();
    const formItems: SingularFormItem[] = [];

    // Create form items container
    const formItemsContainer = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-form-item-list-row-items-container' ]
    });

    // Create remove button
    const removeButton = new Button({
      classNames: [ 'mynah-form-item-list-row-remove-button' ],
      primary: false,
      disabled: entry?.persistent,
      onClick: (e) => {
        cancelEvent(e);
        this.removeRow(rowId);
      },
      icon: new Icon({ icon: MynahIcons.CANCEL }).render
    }).render;

    // Create form items
    this.props.items.forEach(item => {
      item = { ...item, title: undefined, description: undefined };
      formItems.push({
        ...item,
        value: entry?.value[item.id] as any
      });

      const value = entry?.value[item.id];
      if (value != null) {
        item.value = value;
      }
    });

    // Create form render
    const newForm = new ChatItemFormItemsWrapper({
      tabId: '',
      chatItem: {
        formItems
      },
      onFormChange: (formData: Record<string, string>) => {
        // this.formData = formData;
        this.props.onChange?.(this.getValue());
      },
    });
    formItemsContainer.appendChild(newForm.render);

    // Create row container and add it to the wrapper
    const rowContainer = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-form-item-list-row' ],
      attributes: {
        'data-row-id': rowId
      },
      children: [
        formItemsContainer,
        removeButton
      ]
    });
    this.rowWrapper.appendChild(rowContainer);

    // Store the row reference
    this.rows.set(rowId, { rowElm: rowContainer, rowForm: newForm });
    this.props.onChange?.(this.getValue());
  }

  private removeRow (rowId: string): void {
    const row = this.rows.get(rowId);
    if (row != null) {
      row.rowElm.remove();
      this.rows.delete(rowId);
      this.props.onChange?.(this.getValue());
    }
  }

  setValue = (value: ListItemEntry[]): void => {
    // Clear existing rows
    this.rows.forEach(row => row.rowElm.remove());
    this.rows.clear();

    // Add new rows
    if (value.length > 0) {
      value.forEach(entry => this.addRow(entry));
    } else {
      this.addRow();
    }
  };

  getValue = (): Array< Record<string, string>> => {
    const values: Array< Record<string, string>> = [];
    this.rows.forEach(row => values.push(row.rowForm.getAllValues() as Record<string, string>));
    return values;
  };

  setEnabled = (enabled: boolean): void => {
    if (enabled) {
      this.render.removeAttribute('disabled');
      this.rows.forEach(row => {
        row.rowForm.enableAll();
      });
    } else {
      this.render.setAttribute('disabled', 'disabled');
      this.rows.forEach(row => {
        row.rowForm.disableAll();
      });
    }
  };

  isFormValid = (): boolean => {
    let isValid = true;
    this.rows.forEach(row => {
      isValid = isValid && row.rowForm.isFormValid();
    });
    return isValid;
  };
}

export class FormItemList extends FormItemListAbstract {
  render: ExtendedHTMLElement;

  constructor (props: FormItemListProps) {
    super();
    return new (Config.getInstance().config.componentClasses.FormItemList ?? FormItemListInternal)(props);
  }

  setValue = (value: ListItemEntry[]): void => { };
  getValue = (): Array< Record<string, string>> => [];
  setEnabled = (enabled: boolean): void => { };
}
