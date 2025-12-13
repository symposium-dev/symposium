/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// eslint-disable @typescript-eslint/restrict-template-expressions
import { DomBuilder, DomBuilderObject, ExtendedHTMLElement } from '../helper/dom';
import { StyleLoader } from '../helper/style-loader';
import { Icon, MynahIcons, MynahIconsType } from './icon';

interface TitleDescriptionWithIconProps {
  title?: string | ExtendedHTMLElement | HTMLElement | DomBuilderObject;
  description?: string | ExtendedHTMLElement | HTMLElement | DomBuilderObject;
  icon?: MynahIcons | MynahIconsType;
  testId?: string;
  classNames?: string[];
}
export class TitleDescriptionWithIcon {
  render: ExtendedHTMLElement;
  private readonly props: TitleDescriptionWithIconProps;
  constructor (props: TitleDescriptionWithIconProps) {
    StyleLoader.getInstance().load('components/_title-description-icon.scss');
    this.props = props;
    this.render = DomBuilder.getInstance().build({
      type: 'div',
      testId: props.testId,
      // Apply icon wrapper styles only if icon is provided
      classNames: [
        ...(this.props.icon !== undefined ? [ 'mynah-ui-title-description-icon-wrapper' ] : []),
        ...(this.props.classNames ?? [])
      ],
      children: [
        ...(this.props.icon !== undefined
          ? [ {
              type: 'div',
              testId: `${props.testId ?? ''}-icon`,
              classNames: [ 'mynah-ui-title-description-icon-icon' ],
              children: [ new Icon({
                icon: this.props.icon
              }).render ]
            } ]
          : []),
        ...(this.props.title !== undefined
          ? [ {
              type: 'div',
              testId: `${props.testId ?? ''}-title`,
              classNames: [ 'mynah-ui-title-description-icon-title' ],
              children: [ this.props.title ]
            } ]
          : []),
        ...(this.props.description !== undefined
          ? [ {
              type: 'div',
              testId: `${props.testId ?? ''}-description`,
              classNames: [ 'mynah-ui-title-description-icon-description' ],
              children: [ this.props.description ]
            } ]
          : [])
      ]
    }); ;
  }
}
