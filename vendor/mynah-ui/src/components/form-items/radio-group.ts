/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { Config } from '../../helper/config';
import { DomBuilder, DomBuilderObject, ExtendedHTMLElement } from '../../helper/dom';
import { cancelEvent } from '../../helper/events';
import { generateUID } from '../../helper/guid';
import { StyleLoader } from '../../helper/style-loader';
import { Icon, MynahIcons, MynahIconsType } from '../icon';

interface SelectOption {
  value: string;
  label?: string;
  icon?: MynahIcons | MynahIconsType;
}

export interface RadioGroupProps {
  type?: 'radiogroup' | 'toggle';
  classNames?: string[];
  attributes?: Record<string, string>;
  label?: HTMLElement | ExtendedHTMLElement | string;
  description?: ExtendedHTMLElement;
  value?: string;
  optional?: boolean;
  options?: SelectOption[];
  onChange?: (value: string) => void;
  wrapperTestId?: string;
  optionTestId?: string;
}

export abstract class RadioGroupAbstract {
  render: ExtendedHTMLElement;
  setValue = (value: string): void => {};
  getValue = (): string => '';
  setEnabled = (enabled: boolean): void => {};
}
export class RadioGroupInternal extends RadioGroupAbstract {
  private readonly radioGroupElement: ExtendedHTMLElement;
  private readonly groupName: string = generateUID();
  render: ExtendedHTMLElement;
  constructor (props: RadioGroupProps) {
    StyleLoader.getInstance().load('components/_form-input.scss');
    super();
    // Only add vertical classes for radiogroup type
    const isRadioGroup = props.type === 'radiogroup';
    this.radioGroupElement = DomBuilder.getInstance().build({
      type: 'div',
      testId: props.wrapperTestId,
      classNames: [
        'mynah-form-input',
        ...(props.classNames ?? []),
        ...(isRadioGroup ? [ 'mynah-form-input-vertical' ] : [])
      ],
      children:
        props.options?.map((option, index) => ({
          type: 'div',
          classNames: [
            'mynah-form-input-radio-wrapper',
            ...(isRadioGroup ? [ 'mynah-form-input-radio-wrapper-vertical' ] : [])
          ],
          children: [ {
            type: 'label',
            testId: props.optionTestId,
            classNames: [ 'mynah-form-input-radio-label' ],
            events: {
              click: (e) => {
                cancelEvent(e);
                e.currentTarget.querySelector('input').checked = true;
                this.setValue(option.value);
                props.onChange?.(option.value);
              }
            },
            children: [
              {
                type: 'input',
                attributes: {
                  type: 'radio',
                  id: `${this.groupName}_${option.value}`,
                  name: this.groupName,
                  value: option.value,
                  ...(
                    (props.value !== undefined && props.value === option.value) || (props.optional !== true && props.value === undefined && index === 0) ? { checked: 'checked' } : {}
                  )
                },
              },
              {
                type: 'span',
                classNames: [ 'mynah-form-input-radio-check' ],
                children: [
                  new Icon({ icon: option.icon ?? MynahIcons.DOT }).render
                ]
              },
              ...(option.label != null
                ? [ {
                    type: 'span',
                    children: [ option.label ]
                  } ]
                : [])
            ]
          } ]
        })) as DomBuilderObject[]
    });
    this.render = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-form-input-wrapper' ],
      children: [
        {
          type: 'span',
          classNames: [ 'mynah-form-input-label' ],
          children: [ ...(props.label != null ? [ props.label ] : []) ]
        },
        ...[ props.description !== undefined ? props.description : '' ],
        {
          type: 'div',
          classNames: [ 'mynah-form-input-container', `mynah-form-input-${props.type === 'radiogroup' ? 'radio' : 'toggle'}-group`, 'no-border' ],
          ...(props.attributes !== undefined ? { attributes: props.attributes } : {}),
          children: [
            this.radioGroupElement,
          ]
        }
      ]
    });
  }

  setValue = (value: string): void => {
    this.radioGroupElement.querySelector('[checked]')?.removeAttribute('checked');
    this.radioGroupElement.querySelector(`[id="${this.groupName}_${value}"]`)?.setAttribute('checked', 'checked');
  };

  getValue = (): string => {
    return this.radioGroupElement.querySelector('[checked]')?.getAttribute('id')?.replace(`${this.groupName}_`, '') ?? '';
  };

  setEnabled = (enabled: boolean): void => {
    if (enabled) {
      this.radioGroupElement.removeAttribute('disabled');
      this.radioGroupElement.querySelectorAll('input').forEach(inputElm => inputElm.removeAttribute('disabled'));
    } else {
      this.radioGroupElement.setAttribute('disabled', 'disabled');
      this.radioGroupElement.querySelectorAll('input').forEach(inputElm => inputElm.setAttribute('disabled', 'disabled'));
    }
  };
}

export class RadioGroup extends RadioGroupAbstract {
  render: ExtendedHTMLElement;

  constructor (props: RadioGroupProps) {
    super();
    return new (Config.getInstance().config.componentClasses.RadioGroup ?? RadioGroupInternal)(props);
  }

  setValue = (value: string): void => {};
  getValue = (): string => '';
  setEnabled = (enabled: boolean): void => {};
}
