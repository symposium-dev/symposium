/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { ChatItem, ChatItemButton, ChatItemFormItem, FeedbackPayload, MynahEventNames } from '../../static';
import { DomBuilder, DomBuilderObject, ExtendedHTMLElement } from '../../helper/dom';
import { Button } from '../button';
import { FeedbackFormComment } from './feedback-form-comment';
import { cancelEvent, MynahUIGlobalEvents } from '../../helper/events';
import { Config } from '../../helper/config';
import { Select } from '../form-items/select';
import testIds from '../../helper/test-ids';
import { MynahUITabsStore } from '../../helper/tabs-store';
import { ChatItemFormItemsWrapper } from '../chat-item/chat-item-form-items';
import { ChatItemButtonsWrapper } from '../chat-item/chat-item-buttons';

export interface FeedbackFormProps {
  initPayload?: FeedbackPayload;
}
export class FeedbackForm {
  private readonly feedbackOptionsWrapper: Select;
  private readonly feedbackComment: FeedbackFormComment;
  private readonly feedbackSubmitButton: Button;
  public readonly defaultFeedbackFormItems: ExtendedHTMLElement[];
  private feedbackPayload: FeedbackPayload = { messageId: '', selectedOption: '', tabId: '', comment: '' };
  private chatFormItems: ChatItemFormItemsWrapper | null = null;
  private chatButtons: ChatItemButtonsWrapper | null = null;

  constructor (props?: FeedbackFormProps) {
    this.feedbackPayload = {
      selectedOption: Config.getInstance().config.feedbackOptions[0].value,
      messageId: '',
      tabId: '',
      comment: '',
      ...props?.initPayload
    };

    this.feedbackOptionsWrapper = new Select({
      wrapperTestId: testIds.feedbackForm.optionsSelectWrapper,
      optionTestId: testIds.feedbackForm.optionsSelect,
      options: Config.getInstance().config.feedbackOptions,
      onChange: (val) => {
        this.feedbackPayload.selectedOption = val;
      },
      label: Config.getInstance().config.texts.feedbackFormOptionsLabel,
    });

    this.feedbackComment = new FeedbackFormComment({
      onChange: (comment: string) => {
        this.feedbackPayload.comment = comment;
      },
      initComment: this.feedbackPayload?.comment,
    });

    this.feedbackSubmitButton = new Button({
      testId: testIds.feedbackForm.submitButton,
      label: Config.getInstance().config.texts.submit,
      primary: true,
      onClick: () => {
        this.onFeedbackSet(this.feedbackPayload);
        this.close();
      },
    });

    this.defaultFeedbackFormItems = [
      this.feedbackOptionsWrapper.render,
      DomBuilder.getInstance().build({
        type: 'span',
        children: [ Config.getInstance().config.texts.feedbackFormCommentLabel ],
      }),
      this.feedbackComment.render,
      DomBuilder.getInstance().build({
        type: 'div',
        classNames: [ 'mynah-feedback-form-buttons-container' ],
        children: [
          new Button({
            testId: testIds.feedbackForm.cancelButton,
            primary: false,
            label: Config.getInstance().config.texts.cancel,
            onClick: () => {
              this.close();
            }
          }).render,
          this.feedbackSubmitButton.render
        ]
      }),
    ];

    MynahUIGlobalEvents.getInstance().addListener(MynahEventNames.SHOW_FEEDBACK_FORM, (data: {
      messageId?: string; tabId: string; customFormData?: {
        title?: string;
        description?: string;
        buttons?: ChatItemButton[];
        formItems?: ChatItemFormItem[];
      };
    }) => {
      const title = data.messageId !== undefined
        ? Config.getInstance().config.texts.feedbackFormTitle
        : data.customFormData !== undefined
          ? data.customFormData.title
          : undefined;

      const description = data.messageId !== undefined
        ? Config.getInstance().config.texts.feedbackFormDescription
        : data.customFormData !== undefined
          ? data.customFormData.description
          : undefined;

      const defaultOrCustomChatItems = data.messageId !== undefined
        ? this.defaultFeedbackFormItems
        : data.customFormData !== undefined
          ? this.getFormItems({
            tabId: data.tabId,
            title: data.customFormData?.title,
            description: data.customFormData?.description,
            onFormDisabled: () => {
              this.close();
            },
            onFormAction: () => {
              this.close();
            },
            onCloseButtonClick: (e) => {
              cancelEvent(e);
              this.close();
            },
            chatItem: data.customFormData
          })
          : [];

      MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.OPEN_SHEET, {
        tabId: data.tabId,
        title,
        description,
        children: defaultOrCustomChatItems
      });
      if (data.messageId !== undefined) {
        this.feedbackPayload.messageId = data.messageId;
      }
      this.feedbackPayload.tabId = data.tabId;
    });
  }

  private readonly onFeedbackSet = (feedbackData: FeedbackPayload): void => {
    MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.FEEDBACK_SET, feedbackData);
  };

  close = (): void => {
    this.feedbackComment.clear();
    this.feedbackOptionsWrapper.setValue(Config.getInstance().config.feedbackOptions[0].value);
    this.feedbackPayload = {
      messageId: '',
      selectedOption: Config.getInstance().config.feedbackOptions[0].value,
      tabId: '',
      comment: ''
    };
    MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.CLOSE_SHEET, {});
  };

  private readonly getFormItems = (data: {
    tabId: string;
    chatItem: Partial<ChatItem>;
    title?: string;
    description?: string;
    onFormAction?: (actionName: string, formData: Record<string, string | Array<Record<string, string>>>) => void;
    onFormDisabled?: () => void;
    onCloseButtonClick?: (e: Event) => void;
  }): Array<ExtendedHTMLElement | HTMLElement | string | DomBuilderObject> => {
    if (MynahUITabsStore.getInstance().getTabDataStore(data.tabId) === undefined) {
      return [];
    }
    if (this.chatFormItems !== null) {
      this.chatFormItems.render.remove();
      this.chatFormItems = null;
    }
    if (data.chatItem.formItems !== undefined) {
      this.chatFormItems = new ChatItemFormItemsWrapper({
        tabId: data.tabId,
        chatItem: data.chatItem,
        onModifierEnterPress (formData, tabId) {
          MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.FORM_MODIFIER_ENTER_PRESS, { formData, tabId });
        },
        onTextualItemKeyPress (event, itemId, formData, tabId, disableAllCallback) {
          MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.FORM_TEXTUAL_ITEM_KEYPRESS, {
            event,
            formData,
            itemId,
            tabId,
            callback: (disableAll?: boolean) => {
              if (disableAll === true) {
                disableAllCallback();
              }
            }
          });
        },
        onFormChange (formData, isValid, tabId) {
          MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.FORM_CHANGE, { formData, isValid, tabId });
        },
      });
    }

    if (this.chatButtons !== null) {
      this.chatButtons.render.remove();
      this.chatButtons = null;
    }
    if (data.chatItem.buttons !== undefined) {
      this.chatButtons = new ChatItemButtonsWrapper({
        tabId: data.tabId,
        formItems: this.chatFormItems,
        buttons: data.chatItem.buttons,
        onAllButtonsDisabled: data.onFormDisabled,
        onActionClick: (action, e) => {
          if (e !== undefined) {
            cancelEvent(e);
          }

          MynahUIGlobalEvents.getInstance().dispatch(MynahEventNames.CUSTOM_FORM_ACTION_CLICK, {
            tabId: data.tabId,
            id: action.id,
            text: action.text,
            ...(this.chatFormItems !== null ? { formItemValues: this.chatFormItems.getAllValues() } : {})
          });

          if (data.onFormAction !== undefined) {
            data.onFormAction(action.id, this.chatFormItems !== null ? this.chatFormItems.getAllValues() : {});
          }
        }
      });
    }
    return [
      ...(this.chatFormItems !== null
        ? [ (this.chatFormItems).render ]
        : []),
      ...(this.chatButtons !== null
        ? [ (this.chatButtons).render ]
        : []),
    ];
  };
}
