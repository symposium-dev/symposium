/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { FeedbackPayload, MynahEventNames, RelevancyVoteType } from '../../static';
import { DomBuilder, ExtendedHTMLElement } from '../../helper/dom';
import { Icon, MynahIcons } from '../icon';
import { MynahUIGlobalEvents } from '../../helper/events';
import { Button } from '../button';
import { Config } from '../../helper/config';
import testIds from '../../helper/test-ids';

const THANKS_REMOVAL_DURATION = 3500;
export interface ChatItemRelevanceVoteProps {
  tabId: string;
  classNames?: string[];
  messageId: string;
}
export class ChatItemRelevanceVote {
  private readonly votingId: string;
  private sendFeedbackListenerId: string | undefined;
  render: ExtendedHTMLElement;
  props: ChatItemRelevanceVoteProps;
  constructor (props: ChatItemRelevanceVoteProps) {
    this.props = props;
    this.votingId = `${this.props.tabId}-${this.props.messageId}`;
    this.render = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-card-votes-wrapper', ...(this.props.classNames ?? []) ],
      testId: testIds.chatItem.vote.wrapper,
      children: [
        {
          type: 'div',
          classNames: [ 'mynah-card-vote' ],
          children: [
            {
              type: 'input',
              testId: testIds.chatItem.vote.upvote,
              events: {
                change: (e: Event) => {
                  this.handleVoteChange(RelevancyVoteType.UP);
                },
              },
              attributes: {
                type: 'radio',
                id: `${this.votingId}-vote-up`,
                name: `${this.votingId}-vote`,
                value: 'up',
              },
              classNames: [ 'mynah-vote-radio', 'mynah-vote-up-radio' ],
            },
            {
              type: 'input',
              testId: testIds.chatItem.vote.downvote,
              events: {
                change: (e: Event) => {
                  this.handleVoteChange(RelevancyVoteType.DOWN);
                },
              },
              attributes: {
                type: 'radio',
                id: `${this.votingId}-vote-down`,
                name: `${this.votingId}-vote`,
                value: 'down',
              },
              classNames: [ 'mynah-vote-radio', 'mynah-vote-down-radio' ],
            },
            {
              type: 'label',
              testId: testIds.chatItem.vote.upvoteLabel,
              attributes: { for: `${this.votingId}-vote-up` },
              classNames: [ 'mynah-vote-label', 'mynah-vote-up' ],
              children: [ new Icon({ icon: MynahIcons.THUMBS_UP }).render ],
            },
            {
              type: 'label',
              testId: testIds.chatItem.vote.downvoteLabel,
              attributes: { for: `${this.votingId}-vote-down` },
              classNames: [ 'mynah-vote-label', 'mynah-vote-down' ],
              children: [ new Icon({ icon: MynahIcons.THUMBS_DOWN }).render ],
            },
          ],
        },
      ],
    });
  }

  private readonly handleVoteChange = (vote: RelevancyVoteType): void => {
    MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.CARD_VOTE, { messageId: this.props.messageId, tabId: this.props.tabId, vote });
    const newChildren = [
      DomBuilder.getInstance().build({
        type: 'span',
        testId: testIds.chatItem.vote.thanks,
        innerHTML: Config.getInstance().config.texts.feedbackThanks,
      }),
      ...(vote === RelevancyVoteType.DOWN
        ? [
            new Button({
              testId: testIds.chatItem.vote.reportButton,
              label: Config.getInstance().config.texts.feedbackReportButtonLabel,
              onClick: () => {
                if (this.sendFeedbackListenerId === undefined) {
                  this.sendFeedbackListenerId = MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.FEEDBACK_SET, (data: FeedbackPayload) => {
                    if (data.messageId === this.props.messageId && data.tabId === this.props.tabId) {
                      MynahUIGlobalEvents.getInstance().removeListener(MynahEventNames.FEEDBACK_SET, this.sendFeedbackListenerId as string);
                      this.render.remove();
                    }
                  });
                }
                MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.SHOW_FEEDBACK_FORM, { tabId: this.props.tabId, messageId: this.props.messageId });
              },
              primary: false
            }).render
          ]
        : [])
    ];
    this.render.replaceChildren(...newChildren);

    if (vote === RelevancyVoteType.UP) {
      setTimeout(() => {
        this.render.remove();
      }, THANKS_REMOVAL_DURATION);
    }
  };
}
