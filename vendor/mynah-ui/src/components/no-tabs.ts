/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { Config } from '../helper/config';
import { DomBuilder, DomBuilderObject, ExtendedHTMLElement } from '../helper/dom';
import { cancelEvent } from '../helper/events';
import { MynahUITabsStore } from '../helper/tabs-store';
import { Button } from './button';
import { Icon, MynahIcons } from './icon';
import testIds from '../helper/test-ids';
import { parseMarkdown } from '../helper/marked';
import { StyleLoader } from '../helper/style-loader';

export class NoTabs {
  render: ExtendedHTMLElement;
  constructor () {
    StyleLoader.getInstance().load('components/_no-tabs.scss');

    // Determine what to show in the icon wrapper:
    // - If noTabsImage is set, render an actual <img> element
    // - Otherwise, use the Icon component (CSS mask) with noTabsIcon or default TABS icon
    const config = Config.getInstance().config;
    let iconWrapperChild: DomBuilderObject | ExtendedHTMLElement;

    if (config.noTabsImage != null) {
      const opacity = config.noTabsImageOpacity ?? 0.25;
      const background = config.noTabsImageBackground ?? '';
      const borderRadius = config.noTabsImageBorderRadius ?? '';
      const padding = config.noTabsImagePadding ?? '';
      const filter = config.noTabsImageFilter ?? '';

      // Build style string for container
      const containerStyles = [
        background !== '' ? `background-color: ${background}` : '',
        borderRadius !== '' ? `border-radius: ${borderRadius}` : '',
        padding !== '' ? `padding: ${padding}` : ''
      ].filter(s => s !== '').join('; ');

      // Build style string for image
      const imageStyles = [
        `opacity: ${opacity}`,
        filter !== '' ? `filter: ${filter}` : ''
      ].filter(s => s !== '').join('; ');

      iconWrapperChild = {
        type: 'div',
        classNames: [ 'mynah-no-tabs-image-container' ],
        attributes: containerStyles !== '' ? { style: containerStyles } : {},
        children: [
          {
            type: 'img',
            classNames: [ 'mynah-no-tabs-image' ],
            attributes: {
              src: config.noTabsImage,
              alt: '',
              style: imageStyles
            }
          }
        ]
      };
    } else {
      iconWrapperChild = new Icon({ icon: config.noTabsIcon ?? MynahIcons.TABS }).render;
    }

    this.render = DomBuilder.getInstance().build({
      type: 'div',
      testId: testIds.noTabs.wrapper,
      persistent: true,
      classNames: [ 'mynah-no-tabs-wrapper', ...(MynahUITabsStore.getInstance().tabsLength() > 0 ? [ 'hidden' ] : []) ],
      children: [
        {
          type: 'div',
          classNames: [ 'mynah-no-tabs-icon-wrapper' ],
          children: [ iconWrapperChild ]
        },
        {
          type: 'div',
          classNames: [ 'mynah-no-tabs-info' ],
          innerHTML: parseMarkdown(Config.getInstance().config.texts.noTabsOpen ?? '')
        },
        {
          type: 'div',
          classNames: [ 'mynah-no-tabs-buttons-wrapper' ],
          children: [
            new Button({
              testId: testIds.noTabs.newTabButton,
              onClick: (e) => {
                cancelEvent(e);
                if (MynahUITabsStore.getInstance().tabsLength() < Config.getInstance().config.maxTabs) {
                  MynahUITabsStore.getInstance().addTab();
                }
              },
              status: 'main',
              icon: new Icon({ icon: MynahIcons.PLUS }).render,
              label: Config.getInstance().config.texts.openNewTab
            }).render
          ]
        }
      ],
    });

    MynahUITabsStore.getInstance().addListener('add', () => {
      this.render.addClass('hidden');
    });

    MynahUITabsStore.getInstance().addListener('remove', () => {
      if (MynahUITabsStore.getInstance().tabsLength() === 0) {
        this.render.removeClass('hidden');
      }
    });
  }
}
