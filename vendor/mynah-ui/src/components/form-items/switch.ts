/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { Config } from '../../helper/config';
import { DomBuilder, ExtendedHTMLElement } from '../../helper/dom';
import { cancelEvent } from '../../helper/events';
import { StyleLoader } from '../../helper/style-loader';
import { Icon, MynahIcons, MynahIconsType } from '../icon';

export interface SwitchProps {
  classNames?: string[];
  attributes?: Record<string, string>;
  title?: HTMLElement | ExtendedHTMLElement | string;
  label?: string;
  description?: ExtendedHTMLElement;
  value?: 'true' | 'false';
  optional?: boolean;
  icon?: MynahIcons | MynahIconsType;
  onChange?: (value: 'true' | 'false') => void;
  testId?: string;
}

export abstract class SwitchAbstract {
  render: ExtendedHTMLElement;
  setValue = (value: 'true' | 'false'): void => {};
  getValue = (): 'true' | 'false' => 'false';
  setEnabled = (enabled: boolean): void => {};
}
export class SwitchInternal extends SwitchAbstract {
  private readonly checkboxWrapper: ExtendedHTMLElement;
  private readonly checkboxItem: ExtendedHTMLElement;
  render: ExtendedHTMLElement;
  constructor (props: SwitchProps) {
    StyleLoader.getInstance().load('components/_form-input.scss');
    StyleLoader.getInstance().load('components/form-items/_switch.scss');
    super();
    this.checkboxItem = DomBuilder.getInstance().build({
      type: 'input',
      attributes: {
        type: 'checkbox',
      },
    });
    this.checkboxItem.checked = props.value === 'true';

    this.checkboxWrapper = DomBuilder.getInstance().build({
      type: 'div',
      testId: props.testId,
      classNames: [ 'mynah-form-input', ...(props.classNames ?? []) ],
      children: [
        {
          type: 'div',
          classNames: [ 'mynah-form-input-switch-wrapper' ],
          children: [ {
            type: 'label',
            classNames: [ 'mynah-form-input-switch-label' ],
            events: {
              click: (e) => {
                cancelEvent(e);
                const checkState = (!this.checkboxItem.checked).toString() as 'true' | 'false';
                this.setValue(checkState);
                props.onChange?.(checkState);
              }
            },
            children: [
              this.checkboxItem,
              {
                type: 'span',
                classNames: [ 'mynah-form-input-switch-check' ],
                children: [
                  new Icon({ icon: props.icon ?? MynahIcons.OK }).render,
                ]
              },
              { type: 'div', classNames: [ 'mynah-form-input-switch-check-bg' ] }
            ]
          },
          ...(props.label != null
            ? [ {
                type: 'span',
                children: [ props.label ]
              } ]
            : [])
          ]
        } ]
    });
    this.render = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-form-input-wrapper' ],
      children: [
        {
          type: 'span',
          classNames: [ 'mynah-form-input-label' ],
          children: [ ...(props.title != null ? [ props.title ] : []) ]
        },
        ...[ props.description !== undefined ? props.description : '' ],
        {
          type: 'div',
          classNames: [ 'mynah-form-input-container', 'no-border' ],
          ...(props.attributes !== undefined ? { attributes: props.attributes } : {}),
          children: [
            this.checkboxWrapper,
          ]
        }
      ]
    });
  }

  setValue = (value: 'true' | 'false'): void => {
    this.checkboxItem.checked = value === 'true';
  };

  getValue = (): 'true' | 'false' => {
    return this.checkboxItem.checked.toString() as 'true' | 'false';
  };

  setEnabled = (enabled: boolean): void => {
    if (enabled) {
      this.checkboxWrapper.removeAttribute('disabled');
      this.checkboxItem.removeAttribute('disabled');
    } else {
      this.checkboxWrapper.setAttribute('disabled', 'disabled');
      this.checkboxItem.setAttribute('disabled', 'disabled');
    }
  };
}

export class Switch extends SwitchAbstract {
  render: ExtendedHTMLElement;

  constructor (props: SwitchProps) {
    super();
    return new (Config.getInstance().config.componentClasses.Switch ?? SwitchInternal)(props);
  }

  setValue = (value: 'true' | 'false'): void => {};
  getValue = (): 'true' | 'false' => 'false';
  setEnabled = (enabled: boolean): void => {};
}
