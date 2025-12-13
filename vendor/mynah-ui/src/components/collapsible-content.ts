/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// eslint-disable @typescript-eslint/restrict-template-expressions
import { DomBuilder, DomBuilderObject, ExtendedHTMLElement } from '../helper/dom';
import { generateUID } from '../helper/guid';
import { StyleLoader } from '../helper/style-loader';
import { Icon, MynahIcons } from './icon';

interface CollapsibleContentProps {
  title: string | ExtendedHTMLElement | HTMLElement | DomBuilderObject;
  testId?: string;
  children: Array<string | ExtendedHTMLElement | HTMLElement | DomBuilderObject>;
  classNames?: string[];
  initialCollapsedState?: boolean;
  onCollapseStateChange?: (collapsed: boolean) => void;
}
export class CollapsibleContent {
  render: ExtendedHTMLElement;
  private readonly props: Required<CollapsibleContentProps>;
  private readonly uid: string;
  private icon: ExtendedHTMLElement;
  constructor (props: CollapsibleContentProps) {
    StyleLoader.getInstance().load('components/_collapsible-content.scss');
    this.uid = generateUID();
    this.props = {
      initialCollapsedState: true,
      onCollapseStateChange: () => {},
      testId: 'mynah-ui-collapsible-content',
      classNames: [],
      ...props
    };
    this.icon = new Icon({ icon: this.props.initialCollapsedState ? MynahIcons.RIGHT_OPEN : MynahIcons.DOWN_OPEN }).render;
    this.render = DomBuilder.getInstance().build({
      type: 'div',
      testId: this.props.testId,
      classNames: [ 'mynah-collapsible-content-wrapper', ...this.props.classNames ],
      children: [
        {
          type: 'input',
          classNames: [ 'mynah-collapsible-content-checkbox' ],
          attributes: {
            type: 'checkbox',
            name: this.uid,
            id: this.uid,
            ...(this.props.initialCollapsedState ? { checked: 'checked' } : {})
          },
          events: {
            change: (e) => {
              const val = e.currentTarget.checked;
              const newIcon = new Icon({
                icon: val === true ? MynahIcons.RIGHT_OPEN : MynahIcons.DOWN_OPEN
              }).render;
              this.icon.replaceWith(newIcon);
              this.icon = newIcon;
              this.props.onCollapseStateChange(val);
            }
          },
        },
        {
          type: 'label',
          classNames: [ 'mynah-collapsible-content-label' ],
          attributes: {
            for: this.uid,
          },
          children: [
            {
              type: 'div',
              classNames: [ 'mynah-collapsible-content-label-title-wrapper' ],
              children: [
                this.icon,
                {
                  type: 'span',
                  classNames: [ 'mynah-collapsible-content-label-title-text' ],
                  children: [ this.props.title ]
                }
              ]
            },
            {
              type: 'div',
              classNames: [ 'mynah-collapsible-content-label-content-wrapper' ],
              children: this.props.children
            },
          ],
        },
      ],
    });
  }
}
