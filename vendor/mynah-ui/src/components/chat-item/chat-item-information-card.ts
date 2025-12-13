import { DomBuilder, ExtendedHTMLElement } from '../../helper/dom';
import { ChatItemContent, ChatItemType } from '../../static';
import { ChatItemCard } from './chat-item-card';
import { TitleDescriptionWithIcon } from '../title-description-with-icon';
import { StyleLoader } from '../../helper/style-loader';

export interface ChatItemInformationCardProps {
  tabId: string;
  testId?: string;
  messageId: string | undefined;
  classNames?: string[];
  informationCard: NonNullable<Required<ChatItemContent>['informationCard']>;
}

export class ChatItemInformationCard {
  render: ExtendedHTMLElement;

  constructor (props: ChatItemInformationCardProps) {
    StyleLoader.getInstance().load('components/chat/_chat-item-card-information-card.scss');

    const mainContent = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-chat-item-information-card-main' ],
      children: [
        new TitleDescriptionWithIcon({
          classNames: [ 'mynah-chat-item-information-card-header-container' ],
          icon: props.informationCard.icon,
          title: props.informationCard.title,
          description: props.informationCard.description,
          testId: `${props.testId ?? ''}-header`,
        }).render
      ]
    });

    this.render = DomBuilder.getInstance().build({
      type: 'div',
      testId: props.testId,
      classNames: [ 'mynah-chat-item-information-card', ...(props.classNames ?? []), Object.keys(props.informationCard.status ?? {}).length > 0 ? 'has-footer' : '' ],
      children: [
        mainContent
      ]
    });

    mainContent.insertChild('beforeend', new ChatItemCard({
      tabId: props.tabId,
      small: true,
      inline: true,
      chatItem: {
        ...props.informationCard.content,
        type: ChatItemType.ANSWER,
        messageId: props.messageId,
      }
    }).render);

    if (props.informationCard.status != null) {
      const statusFooter = new TitleDescriptionWithIcon({
        testId: `${props.testId ?? ''}-footer`,
        classNames: [
          'mynah-chat-item-information-card-footer',
          ...(props.informationCard.status.status != null ? [ `status-${props.informationCard.status.status}` ] : []),
        ],
        icon: props.informationCard.status.icon,
        description: props.informationCard.status.body
      }).render;
      this.render.insertChild('beforeend', statusFooter);
    }
  }
}
