/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { Config } from '../../helper/config';
import { DomBuilder, ExtendedHTMLElement } from '../../helper/dom';
import { cancelEvent } from '../../helper/events';
import testIds from '../../helper/test-ids';
import { isMandatoryItemValid, isTextualFormItemValid } from '../../helper/validator';
import { ChatItem, ChatItemFormItem, TextBasedFormItem } from '../../static';
import { Card } from '../card/card';
import { CardBody } from '../card/card-body';
import { Checkbox } from '../form-items/checkbox';
import { FormItemList } from '../form-items/form-item-list';
import { FormItemPillBox } from '../form-items/form-item-pill-box';
import { RadioGroup } from '../form-items/radio-group';
import { Select } from '../form-items/select';
import { Stars } from '../form-items/stars';
import { Switch } from '../form-items/switch';
import { TextArea } from '../form-items/text-area';
import { TextInput } from '../form-items/text-input';
import { Icon, MynahIcons } from '../icon';
import { Overlay, OverlayHorizontalDirection, OverlayVerticalDirection } from '../overlay';
const TOOLTIP_DELAY = 350;
export interface ChatItemFormItemsWrapperProps {
  tabId: string;
  chatItem: Partial<ChatItem>;
  classNames?: string[];
  onModifierEnterPress?: (formData: Record<string, any>, tabId: string) => void;
  onTextualItemKeyPress?: (event: KeyboardEvent, itemId: string, formData: Record<string, any>, tabId: string, disableAllCallback: () => void) => void;
  onFormChange?: (formData: Record<string, any>, isValid: boolean, tabId: string) => void;
}
export class ChatItemFormItemsWrapper {
  private readonly props: ChatItemFormItemsWrapperProps;
  private readonly options: Record<string, Select | TextArea | TextInput | RadioGroup | Stars | Checkbox | FormItemList | FormItemPillBox> = {};
  private readonly validationItems: Record<string, boolean> = {};
  private isValid: boolean = false;
  private tooltipOverlay: Overlay | null;
  private tooltipTimeout: ReturnType<typeof setTimeout>;
  onValidationChange?: (isValid: boolean) => void;
  onAllFormItemsDisabled?: () => void;

  render: ExtendedHTMLElement;
  constructor (props: ChatItemFormItemsWrapperProps) {
    this.props = props;
    this.render = DomBuilder.getInstance().build({
      type: 'div',
      testId: testIds.chatItem.chatItemForm.wrapper,
      classNames: [ 'mynah-chat-item-form-items-container', ...(this.props.classNames ?? []) ],
      children: this.props.chatItem.formItems?.map(chatItemOption => {
        const title = `${chatItemOption.mandatory === true && chatItemOption.hideMandatoryIcon !== true ? '* ' : ''}${chatItemOption.title ?? ''}`;
        let chatOption: Select | RadioGroup | TextArea | Stars | TextInput | Checkbox | FormItemList | undefined;
        const label: ExtendedHTMLElement = DomBuilder.getInstance().build({
          type: 'div',
          children: [ title ]
        });
        if (chatItemOption.boldTitle === true) {
          label.addClass('.mynah-ui-form-item-bold-label');
        }
        if (chatItemOption.mandatory === true) {
          if (chatItemOption.hideMandatoryIcon !== true) {
            // Add the mandatory class to the existing label
            label.addClass('mynah-ui-form-item-mandatory-title');
            label.update({
              testId: testIds.chatItem.chatItemForm.title,
              children: [
                new Icon({ icon: MynahIcons.ASTERISK }).render,
                chatItemOption.title ?? '',
              ]
            });
          }
          // Since the field is mandatory, default the selected value to the first option
          if (chatItemOption.type === 'select' && chatItemOption.value === undefined) {
            chatItemOption.value = chatItemOption.options?.[0]?.value;
          }
        }
        let description;
        if (chatItemOption.description != null) {
          description = DomBuilder.getInstance().build({
            type: 'span',
            testId: testIds.chatItem.chatItemForm.description,
            classNames: [ 'mynah-ui-form-item-description' ],
            children: [
              chatItemOption.description,
            ]
          });
        }
        const fireModifierAndEnterKeyPress = (): void => {
          if ((chatItemOption as TextBasedFormItem).checkModifierEnterKeyPress === true && this.isFormValid()) {
            this.props.onModifierEnterPress?.(this.getAllValues(), props.tabId);
          }
        };
        const value = chatItemOption.value?.toString();
        switch (chatItemOption.type) {
          case 'list':
            chatOption = new FormItemList({
              wrapperTestId: testIds.chatItem.chatItemForm.itemList,
              label,
              description,
              items: chatItemOption.items,
              value: chatItemOption.value,
              ...(this.getHandlers(chatItemOption))
            });
            if (chatItemOption.disabled === true) {
              chatOption.setEnabled(false);
            }
            break;
          case 'select':
            chatOption = new Select({
              wrapperTestId: testIds.chatItem.chatItemForm.itemSelectWrapper,
              optionTestId: testIds.chatItem.chatItemForm.itemSelect,
              label,
              border: chatItemOption.border,
              autoWidth: chatItemOption.autoWidth,
              description,
              value,
              icon: chatItemOption.icon,
              options: chatItemOption.options,
              optional: chatItemOption.mandatory !== true,
              placeholder: chatItemOption.placeholder ?? Config.getInstance().config.texts.pleaseSelect,
              tooltip: chatItemOption.selectTooltip ?? '',
              ...(this.getHandlers(chatItemOption))
            });
            if (chatItemOption.disabled === true) {
              chatOption.setEnabled(false);
            }
            break;
          case 'radiogroup':
          case 'toggle':
            chatOption = new RadioGroup({
              type: chatItemOption.type,
              wrapperTestId: testIds.chatItem.chatItemForm.itemRadioWrapper,
              optionTestId: testIds.chatItem.chatItemForm.itemRadio,
              label,
              description,
              value,
              options: chatItemOption.options,
              optional: chatItemOption.mandatory !== true,
              ...(this.getHandlers(chatItemOption))
            });
            if (chatItemOption.disabled === true) {
              chatOption.setEnabled(false);
            }
            break;
          case 'checkbox':
            chatOption = new Checkbox({
              wrapperTestId: testIds.chatItem.chatItemForm.itemToggleWrapper,
              optionTestId: testIds.chatItem.chatItemForm.itemToggleOption,
              title: label,
              label: chatItemOption.label,
              icon: chatItemOption.icon,
              description,
              value: value as 'true' | 'false',
              optional: chatItemOption.mandatory !== true,
              ...(this.getHandlers(chatItemOption))
            });
            break;
          case 'switch':
            chatOption = new Switch({
              testId: testIds.chatItem.chatItemForm.itemSwitch,
              title: label,
              label: chatItemOption.label,
              icon: chatItemOption.icon,
              description,
              value: value as 'true' | 'false',
              optional: chatItemOption.mandatory !== true,
              ...(this.getHandlers(chatItemOption))
            });
            break;
          case 'textarea':
            chatOption = new TextArea({
              testId: testIds.chatItem.chatItemForm.itemTextArea,
              label,
              autoFocus: chatItemOption.autoFocus,
              description,
              fireModifierAndEnterKeyPress,
              onKeyPress: (event) => {
                this.handleTextualItemKeyPressEvent(event, chatItemOption.id);
              },
              value,
              mandatory: chatItemOption.mandatory,
              validationPatterns: chatItemOption.validationPatterns,
              placeholder: chatItemOption.placeholder,
              ...(this.getHandlers(chatItemOption))
            });
            break;
          case 'textinput':
            chatOption = new TextInput({
              testId: testIds.chatItem.chatItemForm.itemInput,
              label,
              autoFocus: chatItemOption.autoFocus,
              description,
              icon: chatItemOption.icon,
              fireModifierAndEnterKeyPress,
              onKeyPress: (event) => {
                this.handleTextualItemKeyPressEvent(event, chatItemOption.id);
              },
              value,
              mandatory: chatItemOption.mandatory,
              validationPatterns: chatItemOption.validationPatterns,
              validateOnChange: chatItemOption.validateOnChange,
              placeholder: chatItemOption.placeholder,
              ...(this.getHandlers(chatItemOption))
            });
            if (chatItemOption.disabled === true) {
              chatOption.setEnabled(false);
            }
            break;
          case 'numericinput':
            chatOption = new TextInput({
              testId: testIds.chatItem.chatItemForm.itemInput,
              label,
              autoFocus: chatItemOption.autoFocus,
              description,
              icon: chatItemOption.icon,
              fireModifierAndEnterKeyPress,
              onKeyPress: (event) => {
                this.handleTextualItemKeyPressEvent(event, chatItemOption.id);
              },
              value,
              mandatory: chatItemOption.mandatory,
              validationPatterns: chatItemOption.validationPatterns,
              type: 'number',
              placeholder: chatItemOption.placeholder,
              ...(this.getHandlers(chatItemOption))
            });
            if (chatItemOption.disabled === true) {
              chatOption.setEnabled(false);
            }
            break;
          case 'email':
            chatOption = new TextInput({
              testId: testIds.chatItem.chatItemForm.itemInput,
              label,
              autoFocus: chatItemOption.autoFocus,
              description,
              icon: chatItemOption.icon,
              fireModifierAndEnterKeyPress,
              onKeyPress: (event) => {
                this.handleTextualItemKeyPressEvent(event, chatItemOption.id);
              },
              value,
              mandatory: chatItemOption.mandatory,
              validationPatterns: chatItemOption.validationPatterns,
              type: 'email',
              placeholder: chatItemOption.placeholder,
              ...(this.getHandlers(chatItemOption))
            });
            if (chatItemOption.disabled === true) {
              chatOption.setEnabled(false);
            }
            break;
          case 'pillbox':
            chatOption = new FormItemPillBox({
              id: chatItemOption.id,
              wrapperTestId: testIds.chatItem.chatItemForm.itemInput,
              label,
              description,
              value,
              placeholder: chatItemOption.placeholder,
              ...(this.getHandlers(chatItemOption))
            });
            break;
          case 'stars':
            chatOption = new Stars({
              wrapperTestId: testIds.chatItem.chatItemForm.itemStarsWrapper,
              optionTestId: testIds.chatItem.chatItemForm.itemStars,
              label,
              description,
              value,
              ...(this.getHandlers(chatItemOption))
            });
            break;
          default:
            break;
        }

        if (chatOption != null) {
          this.options[chatItemOption.id] = chatOption;
          if (chatItemOption.tooltip != null) {
            chatOption.render.update({
              events: {
                mouseover: (e) => {
                  cancelEvent(e);
                  if (chatItemOption.tooltip != null && chatOption?.render != null) {
                    let tooltipToShow = chatItemOption.tooltip;
                    if ((chatItemOption.type === 'checkbox' || chatItemOption.type === 'switch') && chatItemOption.alternateTooltip != null && chatOption.getValue() === 'false') {
                      tooltipToShow = chatItemOption.alternateTooltip;
                    }
                    this.showTooltip(tooltipToShow, chatOption.render);
                  }
                },
                mouseleave: this.hideTooltip
              }
            });
          }
          return chatOption.render;
        }
        return null;
      }) as ExtendedHTMLElement[]
    });
    this.isFormValid();
  }

  private readonly showTooltip = (content: string, elm: HTMLElement | ExtendedHTMLElement): void => {
    if (content.trim() !== undefined) {
      clearTimeout(this.tooltipTimeout);
      this.tooltipTimeout = setTimeout(() => {
        this.tooltipOverlay = new Overlay({
          background: true,
          closeOnOutsideClick: false,
          referenceElement: elm,
          dimOutside: false,
          removeOtherOverlays: true,
          verticalDirection: OverlayVerticalDirection.TO_TOP,
          horizontalDirection: OverlayHorizontalDirection.START_TO_RIGHT,
          children: [
            new Card({
              border: false,
              children: [
                new CardBody({
                  body: content
                }).render
              ]
            }).render
          ],
        });
      }, TOOLTIP_DELAY);
    }
  };

  public readonly hideTooltip = (): void => {
    clearTimeout(this.tooltipTimeout);
    if (this.tooltipOverlay !== null) {
      this.tooltipOverlay?.close();
      this.tooltipOverlay = null;
    }
  };

  private readonly getHandlers = (chatItemOption: ChatItemFormItem): Object => {
    if (chatItemOption.mandatory === true ||
      ([ 'textarea', 'textinput', 'numericinput', 'email', 'pillbox' ].includes(chatItemOption.type) && (chatItemOption as TextBasedFormItem).validationPatterns != null)) {
      // Set initial validation status
      this.validationItems[chatItemOption.id] = this.isItemValid(chatItemOption.value as string ?? '', chatItemOption);
      return {
        onChange: (value: string | number) => {
          this.props.onFormChange?.(this.getAllValues(), this.isFormValid(), this.props.tabId);
          this.validationItems[chatItemOption.id] = this.isItemValid(value.toString(), chatItemOption);
          this.isFormValid();
        }
      };
    }
    return {
      onChange: () => {
        this.props.onFormChange?.(this.getAllValues(), this.isFormValid(), this.props.tabId);
      }
    };
  };

  private readonly handleTextualItemKeyPressEvent = (event: KeyboardEvent, itemId: string): void => {
    this.isFormValid() && this.props.onTextualItemKeyPress?.(event, itemId, this.getAllValues(), this.props.tabId, this.disableAll);
  };

  private readonly isItemValid = (value: string, chatItemOption: ChatItemFormItem): boolean => {
    let validationState = true;
    if (chatItemOption.mandatory === true) {
      validationState = isMandatoryItemValid(value ?? '');
    }
    if (((chatItemOption.type === 'textarea' || chatItemOption.type === 'textinput') && chatItemOption.validationPatterns != null)) {
      validationState = validationState && isTextualFormItemValid(value ?? '', chatItemOption.validationPatterns ?? { patterns: [] }, chatItemOption.mandatory).isValid;
    }

    return validationState;
  };

  isFormValid = (): boolean => {
    const currentValidationStatus = Object.keys(this.validationItems).reduce((prev, curr) => {
      return prev && this.validationItems[curr];
    }, true);

    if (this.isValid !== currentValidationStatus && this.onValidationChange !== undefined) {
      this.onValidationChange(currentValidationStatus);
    }
    this.isValid = currentValidationStatus;
    return currentValidationStatus;
  };

  disableAll = (): void => {
    Object.keys(this.options).forEach(chatOptionId => this.options[chatOptionId].setEnabled(false));
    this.onAllFormItemsDisabled?.();
  };

  enableAll = (): void => {
    Object.keys(this.options).forEach(chatOptionId => this.options[chatOptionId].setEnabled(true));
  };

  getAllValues = (): Record<string, string | Array<Record<string, string>>> => {
    const valueMap: Record<string, string | Array<Record<string, string>>> = {};
    Object.keys(this.options).forEach(chatOptionId => {
      valueMap[chatOptionId] = this.options[chatOptionId].getValue();
    });
    return valueMap;
  };
}
