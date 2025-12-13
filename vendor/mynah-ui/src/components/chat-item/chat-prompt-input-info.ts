/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { DomBuilder, ExtendedHTMLElement } from '../../helper/dom';
import { MynahUITabsStore } from '../../helper/tabs-store';
import { CardBody } from '../card/card-body';
import { MynahUIGlobalEvents } from '../../helper/events';
import { MynahEventNames } from '../../static';
import testIds from '../../helper/test-ids';

export interface ChatPromptInputInfoProps{
  tabId: string;
}
export class ChatPromptInputInfo {
  render: ExtendedHTMLElement;
  constructor (props: ChatPromptInputInfoProps) {
    MynahUITabsStore.getInstance().addListenerToDataStore(props.tabId, 'promptInputInfo', (newInfo: string) => {
      if (newInfo != null && newInfo.trim() !== '') {
        this.render.update({
          children: [
            new CardBody({
              testId: testIds.prompt.footerInfoBody,
              onLinkClick: this.linkClick,
              body: newInfo ?? ''
            }).render
          ]
        });
      } else {
        this.render.clear();
      }
    });

    const footerInfo = MynahUITabsStore.getInstance().getTabDataStore(props.tabId)?.getValue('promptInputInfo');
    this.render = DomBuilder.getInstance().build({
      type: 'div',
      testId: testIds.prompt.footerInfo,
      classNames: [ 'mynah-chat-prompt-input-info' ],
      children: footerInfo != null && footerInfo.trim() !== ''
        ? [
            new CardBody({
              testId: testIds.prompt.footerInfoBody,
              onLinkClick: this.linkClick,
              body: MynahUITabsStore.getInstance().getTabDataStore(props.tabId)?.getValue('promptInputInfo') ?? ''
            }).render
          ]
        : []
    });
  }

  private readonly linkClick = (url: string, e: MouseEvent): void => {
    MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.INFO_LINK_CLICK, {
      link: url,
      event: e,
    });
  };
}
