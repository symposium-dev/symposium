/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { DomBuilder, ExtendedHTMLElement } from '../../helper/dom';
import { MynahUIGlobalEvents } from '../../helper/events';
import testIds from '../../helper/test-ids';
import { ChatItem, MynahEventNames } from '../../static';
import { Button } from '../button';
import { Icon } from '../icon';

export interface ChatItemFollowUpProps {tabId: string; chatItem: ChatItem}
export class ChatItemFollowUpContainer {
  private readonly props: ChatItemFollowUpProps;
  render: ExtendedHTMLElement;
  private readonly itemAddListenerId: string;
  private followupOptions: Button[];
  constructor (props: ChatItemFollowUpProps) {
    this.props = props;
    this.props.chatItem = props.chatItem;
    this.followupOptions = (this.props.chatItem.followUp?.options ?? []).map(followUpOption => (
      new Button({
        testId: testIds.chatItem.chatItemFollowup.optionButton,
        classNames: [ 'mynah-chat-item-followup-question-option' ],
        status: followUpOption.status,
        label: followUpOption.pillText,
        tooltip: followUpOption.description,
        disabled: followUpOption.disabled,
        border: true,
        primary: false,
        ...(followUpOption.icon != null ? { icon: new Icon({ icon: followUpOption.icon }).render } : {}),
        onClick: () => {
          MynahUIGlobalEvents.getInstance().removeListener(MynahEventNames.CHAT_ITEM_ADD, this.itemAddListenerId);
          MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.FOLLOW_UP_CLICKED, {
            tabId: this.props.tabId,
            messageId: this.props.chatItem.messageId,
            followUpOption
          });
          if ((this.render.parentElement as ExtendedHTMLElement)?.hasClass('mynah-chat-item-empty')) {
            this.render.parentElement?.remove();
          } else {
            this.render.remove();
          };
        }
      })
    ));
    this.itemAddListenerId = MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.CHAT_ITEM_ADD, (data) => {
      if (data.tabId === this.props.tabId) {
        this.render.remove();
        this.followupOptions.forEach(option => option.hideTooltip());
        this.followupOptions = [];
        MynahUIGlobalEvents.getInstance().removeListener(MynahEventNames.CHAT_ITEM_ADD, this.itemAddListenerId);
      }
    });
    this.render = DomBuilder.getInstance().build({
      type: 'div',
      testId: testIds.chatItem.chatItemFollowup.wrapper,
      classNames: [ 'mynah-chat-item-followup-question' ],
      children: [
        {
          type: 'div',
          testId: testIds.chatItem.chatItemFollowup.title,
          classNames: [ 'mynah-chat-item-followup-question-text' ],
          children: [ this.props.chatItem.followUp?.text ?? '' ]
        },
        {
          type: 'div',
          testId: testIds.chatItem.chatItemFollowup.optionsWrapper,
          classNames: [ 'mynah-chat-item-followup-question-options-wrapper' ],
          children: this.followupOptions.map(option => option.render)
        },
      ]
    });

    Array.from(this.render.getElementsByTagName('a')).forEach(a => {
      const url = a.href;

      a.onclick = (event: MouseEvent) => {
        this.handleLinkClick(url, event);
      };
      a.onauxclick = (event: MouseEvent) => {
        this.handleLinkClick(url, event);
      };
    });
  }

  private readonly handleLinkClick = (url: string, event?: MouseEvent): void => {
    MynahUIGlobalEvents
      .getInstance()
      .dispatch(MynahEventNames.LINK_CLICK, {
        tabId: this.props.tabId,
        messageId: this.props.chatItem.messageId,
        link: url,
        event,
      });
  };
}
