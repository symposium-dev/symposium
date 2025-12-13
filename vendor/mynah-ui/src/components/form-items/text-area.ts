/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { Config } from '../../helper/config';
import { DomBuilder, ExtendedHTMLElement } from '../../helper/dom';
import { StyleLoader } from '../../helper/style-loader';
import { checkTextElementValidation } from '../../helper/validator';
import { ValidationPattern } from '../../static';

export interface TextAreaProps {
  classNames?: string[];
  attributes?: Record<string, string>;
  label?: HTMLElement | ExtendedHTMLElement | string;
  autoFocus?: boolean;
  description?: ExtendedHTMLElement;
  mandatory?: boolean;
  fireModifierAndEnterKeyPress?: () => void;
  placeholder?: string;
  validationPatterns?: {
    operator?: 'and' | 'or';
    patterns: ValidationPattern[];
  };
  value?: string;
  onChange?: (value: string) => void;
  onKeyPress?: (event: KeyboardEvent) => void;
  testId?: string;
}

export abstract class TextAreaAbstract {
  render: ExtendedHTMLElement;
  setValue = (value: string): void => {};
  getValue = (): string => '';
  setEnabled = (enabled: boolean): void => {};
  checkValidation = (): void => {};
}
export class TextAreaInternal extends TextAreaAbstract {
  private readonly inputElement: ExtendedHTMLElement;
  private readonly validationErrorBlock: ExtendedHTMLElement;
  private readonly props: TextAreaProps;
  private readyToValidate: boolean = false;
  render: ExtendedHTMLElement;
  constructor (props: TextAreaProps) {
    StyleLoader.getInstance().load('components/_form-input.scss');
    super();
    this.props = props;
    this.validationErrorBlock = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-form-input-validation-error-block' ],
    });
    this.inputElement = DomBuilder.getInstance().build({
      type: 'textarea',
      testId: this.props.testId,
      classNames: [ 'mynah-form-input', ...(this.props.classNames ?? []) ],
      attributes: {
        ...(this.props.placeholder !== undefined
          ? {
              placeholder: this.props.placeholder
            }
          : {}),
        ...(this.props.autoFocus === true
          ? {
              autofocus: 'autofocus'
            }
          : {}),
      },
      events: {
        blur: (e) => {
          this.readyToValidate = true;
          this.checkValidation();
        },
        // TODO: change this to 'input' event?
        keyup: (e) => {
          if (this.props.onChange !== undefined) {
            this.props.onChange((e.currentTarget as HTMLTextAreaElement).value);
          }
          this.checkValidation();
        },
        keydown: (e: KeyboardEvent) => {
          if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
            this.props.fireModifierAndEnterKeyPress?.();
          }
        },
        keypress: (e: KeyboardEvent) => {
          this.props.onKeyPress?.(e);
        }
      },
    });
    this.inputElement.value = props.value ?? '';
    this.render = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-form-input-wrapper' ],
      children: [
        {
          type: 'span',
          classNames: [ 'mynah-form-input-label' ],
          children: [ ...(props.label !== undefined ? [ props.label ] : []) ]
        },
        ...[ props.description !== undefined ? props.description : '' ],
        {
          type: 'div',
          classNames: [ 'mynah-form-input-container' ],
          ...(props.attributes !== undefined ? { attributes: props.attributes } : {}),
          children: [
            this.inputElement,
          ]
        },
        this.validationErrorBlock
      ]
    });

    if (this.props.autoFocus === true) {
      setTimeout(() => {
        this.inputElement.focus();
      }, 250);
    }
  }

  setValue = (value: string): void => {
    this.inputElement.value = value;
  };

  getValue = (): string => {
    return this.inputElement.value;
  };

  setEnabled = (enabled: boolean): void => {
    if (enabled) {
      this.inputElement.removeAttribute('disabled');
    } else {
      this.inputElement.setAttribute('disabled', 'disabled');
    }
  };

  checkValidation = (): void => checkTextElementValidation(this.inputElement, this.props.validationPatterns, this.validationErrorBlock, this.readyToValidate, this.props.mandatory);
}

export class TextArea extends TextAreaAbstract {
  render: ExtendedHTMLElement;

  constructor (props: TextAreaProps) {
    super();
    return new (Config.getInstance().config.componentClasses.TextArea ?? TextAreaInternal)(props);
  }

  setValue = (value: string): void => {};
  getValue = (): string => '';
  setEnabled = (enabled: boolean): void => {};
  checkValidation = (): void => {};
}
