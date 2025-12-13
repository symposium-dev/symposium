/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// eslint-disable @typescript-eslint/restrict-template-expressions
import { DomBuilder, ExtendedHTMLElement } from '../helper/dom';
import { ChatItemButton, ProgressField } from '../static';
import { ChatItemButtonsWrapper } from './chat-item/chat-item-buttons';
import { StyleLoader } from '../helper/style-loader';

interface ProgressIndicatorProps extends ProgressField{
  testId?: string;
  classNames?: string[];
  onClick?: () => void;
  onActionClick?: (actionName: ChatItemButton, e?: Event) => void;
}
export class ProgressIndicator {
  render: ExtendedHTMLElement;
  private readonly wrapper: ExtendedHTMLElement;
  private readonly text: ExtendedHTMLElement;
  private readonly valueText: ExtendedHTMLElement;
  private readonly valueBar: ExtendedHTMLElement;
  private buttonsWrapper: ChatItemButtonsWrapper;
  private props: ProgressIndicatorProps;
  constructor (props: ProgressIndicatorProps) {
    StyleLoader.getInstance().load('components/_progress.scss');
    this.props = props;
    this.wrapper = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-progress-indicator-wrapper' ],
    });
    this.text = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-progress-indicator-text' ]
    });
    this.valueText = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-progress-indicator-value-text' ]
    });
    this.valueBar = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-progress-indicator-value-bar' ]
    });
    this.buttonsWrapper = this.getButtonsWrapper();
    this.wrapper.update({
      children: [
        this.valueBar,
        this.text,
        this.valueText,
        this.buttonsWrapper.render
      ]
    });

    this.render = DomBuilder.getInstance().build({
      type: 'div',
      testId: this.props.testId,
      classNames: [ 'mynah-progress-indicator', ...(this.props.classNames ?? []), ...(this.isEmpty() ? [ 'no-content' ] : []) ],
      children: [ this.wrapper ],
    });

    this.update(props);
  }

  private readonly getButtonsWrapper = (): ChatItemButtonsWrapper => {
    const newButtons = new ChatItemButtonsWrapper({
      buttons: this.props.actions ?? [],
      onActionClick: this.props.onActionClick ?? ((action) => {}),
    });
    if (this.buttonsWrapper != null) {
      this.buttonsWrapper.render.replaceWith(newButtons.render);
    }
    return newButtons;
  };

  public isEmpty = (): boolean => this.props.actions == null && this.props.text == null && this.props.valueText == null;

  public update = (props: Partial<ProgressField> | null): void => {
    if (props === null) {
      this.props.actions = undefined;
      this.props.status = undefined;
      this.props.text = undefined;
      this.props.value = undefined;
      this.props.valueText = undefined;
    }
    this.props = {
      ...this.props,
      ...props
    };
    this.valueBar.update({
      attributes: {
        style: `width: ${this.props.value === -1 ? 100 : Math.min(100, this.props.value ?? 0)}%;`
      }
    });
    this.text.update({
      children: [ this.props.text ?? '' ]
    });
    this.valueText.update({
      children: [ this.props.valueText ?? '' ]
    });
    if ((props?.actions) !== undefined) {
      this.buttonsWrapper = this.getButtonsWrapper();
    }
    this.wrapper.update({
      attributes: {
        ...(this.props.value === -1 ? { indeterminate: 'true' } : {}),
        'progress-status': this.props.status ?? 'default'
      }
    });
    if (this.isEmpty()) {
      this.render?.addClass('no-content');
    } else {
      this.render?.removeClass('no-content');
    }
  };
}
