import { DomBuilder, ExtendedHTMLElement } from '../../helper/dom';
import { ChatItemContent, ChatItemType, MynahEventNames } from '../../static';
import { Tab, ToggleOption } from '../tabs';
import { ChatItemCard } from './chat-item-card';
import testIds from '../../helper/test-ids';
import { emptyChatItemContent } from '../../helper/chat-item';
import { MynahUIGlobalEvents } from '../../helper/events';
import { StyleLoader } from '../../helper/style-loader';

export interface ChatItemTabbedCardProps {
  tabId: string;
  messageId: string | undefined;
  tabbedCard: NonNullable<Required<ChatItemContent>['tabbedContent']>;
  classNames?: string[];
}

export class ChatItemTabbedCard {
  contentCard: ChatItemCard;
  render: ExtendedHTMLElement;
  props: ChatItemTabbedCardProps;

  constructor (props: ChatItemTabbedCardProps) {
    StyleLoader.getInstance().load('components/chat/_chat-item-card-tabbed-card.scss');
    this.props = props;
    const toggleGroup = new Tab({
      options: props.tabbedCard,
      direction: 'horizontal',
      name: `tabbed-card-toggle-${props.messageId ?? props.tabId}`,
      value: this.getTabOfSelectedOrGivenValue().value,
      testId: testIds.chatItem.tabbedCard.tabs,
      onChange: (value) => {
        MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.TABBED_CONTENT_SWITCH, {
          tabId: this.props.tabId,
          messageId: this.props.messageId,
          contentTabId: value,
        });
        this.contentCard.clearContent();
        this.contentCard.updateCardStack({
          ...emptyChatItemContent,
          ...this.getTabOfSelectedOrGivenValue(value).content
        });
      }
    });

    const selectedTabContent = (props.tabbedCard.find((tab) => tab.selected) ?? props.tabbedCard[0])?.content;
    this.contentCard = new ChatItemCard({
      tabId: props.tabId,
      chatItem: {
        messageId: props.messageId,
        type: ChatItemType.ANSWER,
        ...selectedTabContent
      }
    });

    this.render = DomBuilder.getInstance().build({
      type: 'div',
      classNames: [ 'mynah-tabbed-card-wrapper', ...props.classNames ?? '' ],
      children: [
        {
          type: 'div',
          classNames: [ 'mynah-tabbed-card-contents' ],
          children: [ this.contentCard.render ]
        },
        ...(props.tabbedCard.length > 1
          ? [ {
              type: 'div',
              classNames: [ 'mynah-tabbed-card-tabs' ],
              children: [
                toggleGroup.render
              ]
            } ]
          : [])
      ]
    });
  }

  private readonly getTabOfSelectedOrGivenValue = (value?: string): (ToggleOption & {
    content: ChatItemContent;
  }) => {
    return this.props.tabbedCard.find((tab) => value != null ? tab.value === value : tab.selected) ?? this.props.tabbedCard[0];
  };
}
