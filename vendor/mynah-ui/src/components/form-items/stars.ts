/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { DomBuilder, ExtendedHTMLElement } from '../../helper/dom';
import { StyleLoader } from '../../helper/style-loader';
import { Icon, MynahIcons } from '../icon';

export type StarValues = 1 | 2 | 3 | 4 | 5;
export interface StarsProps {
  classNames?: string[];
  attributes?: Record<string, string>;
  label?: HTMLElement | ExtendedHTMLElement | string;
  description?: ExtendedHTMLElement;
  value?: string;
  onChange?: (value: string) => void;
  initStar?: StarValues;
  wrapperTestId?: string;
  optionTestId?: string;
}
export class Stars {
  private readonly starsContainer: ExtendedHTMLElement;
  render: ExtendedHTMLElement;

  constructor (props: StarsProps) {
    StyleLoader.getInstance().load('components/_form-input.scss');
    this.starsContainer = DomBuilder.getInstance().build({
      type: 'div',
      testId: props.wrapperTestId,
      classNames: [ 'mynah-feedback-form-stars-container' ],
      attributes: { ...(props.value !== undefined && { 'selected-star': props.value?.toString() ?? '1' }) },
      children: Array(5)
        .fill(undefined)
        .map((n, index) =>
          DomBuilder.getInstance().build({
            type: 'div',
            testId: props.optionTestId,
            classNames: [ 'mynah-feedback-form-star', ...(props.value === (index + 1).toString() ? [ 'selected' ] : []) ],
            events: {
              click: (e: MouseEvent) => {
                (this.starsContainer.querySelector('.selected') as ExtendedHTMLElement)?.removeClass(
                  'selected'
                );
                (e.currentTarget as ExtendedHTMLElement).addClass('selected');
                if (props.onChange !== undefined) {
                  props.onChange((index + 1).toString());
                }
                this.setValue((index + 1) as StarValues);
              },
            },
            attributes: { star: (index + 1).toString() },
            children: [ new Icon({ icon: MynahIcons.STAR }).render ],
          })
        ),
    });

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
          classNames: [ 'mynah-form-input-container', 'no-border' ],
          ...(props.attributes !== undefined ? { attributes: props.attributes } : {}),
          children: [
            {
              type: 'div',
              classNames: [ 'mynah-form-input', ...(props.classNames ?? []) ],
              children: [
                this.starsContainer
              ]
            },
          ]
        }
      ]
    });
  }

  setValue = (star: StarValues): void => {
    this.starsContainer.setAttribute('selected-star', star.toString());
  };

  getValue = (): string => {
    return this.starsContainer.getAttribute('selected-star') ?? '';
  };

  setEnabled = (enabled: boolean): void => {
    if (enabled) {
      this.starsContainer.parentElement?.removeAttribute('disabled');
    } else {
      this.starsContainer.parentElement?.setAttribute('disabled', 'disabled');
    }
  };
}
