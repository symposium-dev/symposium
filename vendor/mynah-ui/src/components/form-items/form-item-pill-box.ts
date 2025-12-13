/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { Config } from '../../helper/config';
import { DomBuilder, ExtendedHTMLElement } from '../../helper/dom';
import { cancelEvent } from '../../helper/events';
import { StyleLoader } from '../../helper/style-loader';

export interface FormItemPillBoxProps {
  id: string;
  value?: string;
  classNames?: string[];
  attributes?: Record<string, string>;
  label?: HTMLElement | ExtendedHTMLElement | string;
  description?: ExtendedHTMLElement;
  placeholder?: string;
  wrapperTestId?: string;
  onChange?: (value: string) => void;
  disabled?: boolean;
}

export abstract class FormItemPillBoxAbstract {
  render: ExtendedHTMLElement;
  setValue = (value: string): void => { };
  getValue = (): string => '';
  setEnabled = (enabled: boolean): void => { };
}

export class FormItemPillBoxInternal extends FormItemPillBoxAbstract {
  private readonly props: FormItemPillBoxProps;
  private readonly pillsContainer: ExtendedHTMLElement;
  private readonly input: ExtendedHTMLElement;
  private readonly wrapper: ExtendedHTMLElement;
  private pills: string[] = [];
  render: ExtendedHTMLElement;

  constructor (props: FormItemPillBoxProps) {
    StyleLoader.getInstance().load('components/form-items/_form-item-pill-box.scss');
    super();
    this.props = props;

    // Create pills container
    this.pillsContainer = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-form-item-pill-box-pills-container' ],
    });

    // Create input field
    this.input = DomBuilder.getInstance().build({
      type: 'textarea',
      classNames: [ 'mynah-form-item-pill-box-input' ],
      attributes: {
        placeholder: props.placeholder ?? 'Type and press Enter to add a tag',
        rows: '1',
      },
      events: {
        keydown: (e: KeyboardEvent) => {
          if (e.key === 'Enter' && !e.shiftKey) {
            cancelEvent(e);
            const value = (this.input as unknown as HTMLTextAreaElement).value.trim();
            if (value !== '') {
              this.addPill(value);
              (this.input as unknown as HTMLTextAreaElement).value = '';
              this.notifyChange();
            }
          }
        }
      }
    });

    // Create wrapper
    this.wrapper = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-form-item-pill-box-wrapper' ],
      children: [
        this.pillsContainer,
        this.input
      ]
    });

    // Create main container
    this.render = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-form-input-wrapper', ...(props.classNames ?? []) ],
      attributes: props.attributes,
      testId: props.wrapperTestId,
      children: [
        {
          type: 'span',
          classNames: [ 'mynah-form-input-label' ],
          children: [
            ...(props.label !== undefined ? [ props.label ] : []),
          ]
        },
        ...(props.description !== undefined ? [ props.description ] : []),
        this.wrapper
      ]
    });

    // Initialize with existing value
    if (props.value != null) {
      this.setValue(props.value);
    }

    // Set initial disabled state
    if (props.disabled === true) {
      this.setEnabled(false);
    }
  }

  private addPill (text: string): void {
    if (text === '' || this.pills.includes(text)) {
      return;
    }

    this.pills.push(text);

    const pill = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-form-item-pill' ],
      children: [
        {
          type: 'span',
          classNames: [ 'mynah-form-item-pill-text' ],
          children: [ text ]
        },
        {
          type: 'span',
          classNames: [ 'mynah-form-item-pill-remove' ],
          children: [ '×' ],
          events: {
            click: (e) => {
              cancelEvent(e);
              pill.remove();
              this.pills = this.pills.filter(p => p !== text);
              this.notifyChange();
            }
          }
        }
      ]
    });

    this.pillsContainer.appendChild(pill);
  }

  private notifyChange (): void {
    if (this.props.onChange != null) {
      this.props.onChange(this.getValue());
    }
  }

  setValue = (value: string): void => {
    // Clear existing pills
    this.pillsContainer.innerHTML = '';
    this.pills = [];

    // Add new pills
    if (value !== '') {
      const pillValues = value.split(/[,\n]+/).map(v => v.trim()).filter(v => v);
      pillValues.forEach(pill => this.addPill(pill));
    }
  };

  getValue = (): string => {
    return this.pills.join(',');
  };

  setEnabled = (enabled: boolean): void => {
    if (enabled) {
      this.render.removeAttribute('disabled');
      (this.input as unknown as HTMLTextAreaElement).disabled = false;
    } else {
      this.render.setAttribute('disabled', 'disabled');
      (this.input as unknown as HTMLTextAreaElement).disabled = true;
    }
  };
}

export class FormItemPillBox extends FormItemPillBoxAbstract {
  render: ExtendedHTMLElement;
  private readonly instance: FormItemPillBoxAbstract;

  constructor (props: FormItemPillBoxProps) {
    super();
    const InternalClass = Config.getInstance().config.componentClasses.FormItemPillBox ?? FormItemPillBoxInternal;
    this.instance = new InternalClass(props);
    this.render = this.instance.render;
  }

  setValue = (value: string): void => {
    this.instance.setValue(value);
  };

  getValue = (): string => {
    return this.instance.getValue();
  };

  setEnabled = (enabled: boolean): void => {
    this.instance.setEnabled(enabled);
  };
}
