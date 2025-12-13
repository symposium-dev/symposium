/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// eslint-disable @typescript-eslint/restrict-template-expressions
import { DomBuilder, ExtendedHTMLElement } from '../helper/dom';
import { Icon, MynahIcons, MynahIconsType } from './icon';
import { Button } from './button';
import { StyleLoader } from '../helper/style-loader';

interface MoreContentIndicatorProps {
  icon?: MynahIcons | MynahIconsType;
  border?: boolean;
  testId?: string;
  onClick: () => void;
}
export class MoreContentIndicator {
  render: ExtendedHTMLElement;
  private props: MoreContentIndicatorProps;
  private button: Button;
  private readonly uid: string;
  private readonly icon: ExtendedHTMLElement;
  constructor (props: MoreContentIndicatorProps) {
    StyleLoader.getInstance().load('components/_more-content-indicator.scss');
    this.props = props;
    this.button = this.getButton();
    this.render = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'more-content-indicator' ],
      testId: props.testId,
      children: [
        this.button.render
      ]
    });
  }

  private readonly getButton = (): Button => {
    return new Button({
      icon: new Icon({ icon: this.props.icon ?? MynahIcons.SCROLL_DOWN }).render,
      primary: false,
      fillState: 'hover',
      border: this.props.border !== false,
      onClick: this.props.onClick
    });
  };

  public update = (props: Partial<MoreContentIndicatorProps>): void => {
    this.props = {
      ...this.props,
      ...props
    };
    const newButton = this.getButton();
    this.button.render.replaceWith(newButton.render);
    this.button = newButton;
  };
}
