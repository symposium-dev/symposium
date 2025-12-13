/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { DomBuilder, ExtendedHTMLElement } from '../../helper/dom';
import { MynahUIGlobalEvents } from '../../helper/events';
import testIds from '../../helper/test-ids';
import { MynahEventNames, SourceLink } from '../../static';
import { Button } from '../button';
import { Card } from '../card/card';
import { Icon, MynahIcons } from '../icon';
import { SourceLinkHeader } from '../source-link/source-link-header';

const MAX_ITEMS = 1;
export interface ChatItemSourceLinksContainerProps {
  tabId: string;
  messageId: string;
  title?: string;
  relatedContent?: SourceLink[];
}
export class ChatItemSourceLinksContainer {
  private readonly props: ChatItemSourceLinksContainerProps;
  private readonly showMoreButtonBlock: Button;
  render: ExtendedHTMLElement;
  chatAvatar: ExtendedHTMLElement;
  constructor (props: ChatItemSourceLinksContainerProps) {
    this.props = props;
    this.showMoreButtonBlock = new Button({
      testId: testIds.chatItem.relatedLinks.showMore,
      classNames: [ 'mynah-chat-item-card-related-content-show-more' ],
      primary: false,
      icon: new Icon({ icon: MynahIcons.DOWN_OPEN }).render,
      onClick: () => {
        MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.SHOW_MORE_WEB_RESULTS_CLICK, { messageId: this.props.messageId });
        this.showMoreButtonBlock.render.remove();
        (this.render).addClass('expanded');
      },
      label: 'Show more',
    });

    if (this.props.relatedContent !== undefined) {
      this.render = DomBuilder.getInstance().build({
        type: 'div',
        testId: testIds.chatItem.relatedLinks.wrapper,
        classNames: [ 'mynah-chat-item-card-related-content',
          this.props.relatedContent !== undefined && this.props.relatedContent.length <= MAX_ITEMS ? 'expanded' : '' ],
        children: [
          ...(this.props.title !== undefined
            ? [ {
                type: 'span',
                testId: testIds.chatItem.relatedLinks.title,
                classNames: [ 'mynah-chat-item-card-related-content-title' ],
                children: [ this.props.title ],
              } ]
            : []),
          ...this.props.relatedContent.map(sourceLink => DomBuilder.getInstance().build({
            type: 'div',
            classNames: [ 'mynah-chat-item-card-related-content-item' ],
            children: [
              new Card({
                background: false,
                border: false,
                padding: 'none',
                children: [
                  new SourceLinkHeader({
                    sourceLink,
                    showCardOnHover: true,
                    onClick: (e) => {
                      MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.SOURCE_LINK_CLICK, {
                        messageId: this.props.messageId,
                        link: sourceLink.url,
                        event: e
                      });
                    }
                  }).render
                ]
              }).render
            ]
          })),
          this.showMoreButtonBlock.render
        ]
      });
    }
  }
}
