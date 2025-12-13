/* eslint-disable @typescript-eslint/no-extraneous-class */
/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { DomBuilder, ExtendedHTMLElement } from '../../helper/dom';
import { StyleLoader } from '../../helper/style-loader';
import LOGO_BASE from './logo-base.svg';
import LOGO_TEXT from './logo-text.svg';

export class Spinner {
  render: ExtendedHTMLElement;
  constructor () {
    StyleLoader.getInstance().load('components/_spinner.scss');
    const portal = DomBuilder.getInstance().getPortal('mynah-ui-icons') ?? DomBuilder.getInstance().createPortal('mynah-ui-icons', {
      type: 'style',
      attributes: {
        type: 'text/css'
      }
    }, 'beforebegin');
    portal.insertAdjacentText('beforeend', `
      :root{
          --mynah-ui-spinner-base: url(${LOGO_BASE});
          --mynah-ui-spinner-text: url(${LOGO_TEXT});
        }
      `);

    this.render = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-ui-spinner-container' ],
      children: [
        {
          type: 'span',
          classNames: [ 'mynah-ui-spinner-logo-part', 'backdrop' ],
          children: [
            {
              type: 'span',
              classNames: [ 'mynah-ui-spinner-logo-mask', 'base' ]
            }
          ]
        },
        {
          type: 'span',
          classNames: [ 'mynah-ui-spinner-logo-part', 'semi-backdrop' ],
          children: [
            {
              type: 'span',
              classNames: [ 'mynah-ui-spinner-logo-mask', 'base' ]
            }
          ]
        },
        {
          type: 'span',
          classNames: [ 'mynah-ui-spinner-logo-part' ],
          children: [
            {
              type: 'span',
              classNames: [ 'mynah-ui-spinner-logo-mask', 'base' ]
            }
          ]
        },
        {
          type: 'span',
          classNames: [ 'mynah-ui-spinner-logo-part' ],
          children: [
            {
              type: 'span',
              classNames: [ 'mynah-ui-spinner-logo-mask', 'text' ]
            }
          ]
        }
      ]
    });
  }
}
