/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { DomBuilder, ExtendedHTMLElement } from '../../helper/dom';
import { MynahUITabsStore } from '../../helper/tabs-store';
import testIds from '../../helper/test-ids';
import { ChatItemType } from '../../static';
import { ChatItemCard } from './chat-item-card';

export interface ChatPromptInputStickyCardProps{
  tabId: string;
}
export class ChatPromptInputStickyCard {
  render: ExtendedHTMLElement;
  constructor (props: ChatPromptInputStickyCardProps) {
    MynahUITabsStore.getInstance().addListenerToDataStore(props.tabId, 'promptInputStickyCard', (newChatItem) => {
      if (newChatItem === null) {
        this.render.clear();
      } else {
        this.render.update({
          children: [ new ChatItemCard({
            inline: true,
            small: true,
            chatItem: {
              ...newChatItem,
              messageId: newChatItem.messageId ?? 'sticky-card',
              type: ChatItemType.ANSWER
            },
            tabId: props.tabId
          }).render ]
        });
      }
    });

    const initChatItemForStickyCard = MynahUITabsStore.getInstance().getTabDataStore(props.tabId)?.getValue('promptInputStickyCard');
    this.render = DomBuilder.getInstance().build({
      type: 'div',
      testId: testIds.prompt.stickyCard,
      classNames: [ 'mynah-chat-prompt-input-sticky-card' ],
      children: initChatItemForStickyCard !== null
        ? [
            new ChatItemCard({
              inline: true,
              small: true,
              chatItem: {
                ...initChatItemForStickyCard,
                messageId: initChatItemForStickyCard.messageId ?? 'sticky-card',
                type: ChatItemType.ANSWER
              },
              tabId: props.tabId
            }).render
          ]
        : []
    });
  }
}
